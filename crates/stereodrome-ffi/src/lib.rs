//! Mobile FFI boundary for Stereodrome.
//!
//! The first mobile implementation uses JSON-over-FFI so the Swift/Kotlin Expo
//! module can remain thin while the Rust API stabilizes. The crate is isolated
//! so a `UniFFI` surface can be generated here without touching the desktop
//! Tauri adapter.

use std::ffi::{CStr, CString, c_char, c_void};
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, Once};
use std::thread;
use std::time::Duration;

use log::{Level, LevelFilter, Metadata, Record};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stereodrome_core::queue::{QueueItem, QueueState, RepeatMode};
use stereodrome_core::{
    CORE_PROTOCOL_VERSION, CacheStateEvent, CommandId, CommandStatus, ConnectParams,
    ConnectivitySettings, CoreCommand, CoreCommandRequest, CoreCommandResult, CoreEvent,
    CoreEventKind, CoreSnapshot, PlaybackOutputState, PlaybackPhase, PlaybackProgress,
    PlaybackProjection, ProtocolError, ProtocolErrorCode, ServerSettingsUpdate, StereodromeCore,
    StereodromeRuntimeHandle, SyncKind, SyncSettings,
};

static MOBILE_LOGGER: MobileLogger = MobileLogger;
static INIT_LOGGER: Once = Once::new();
static INIT_PANIC_HOOK: Once = Once::new();
static LOG_CALLBACK: Mutex<Option<MobileLogCallback>> = Mutex::new(None);
static NEXT_EVENT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

type MobileLogCallback = extern "C" fn(*const c_char);
type MobileEventCallback = extern "C" fn(*const c_char, *mut c_void);

const MOBILE_CACHE_RECONCILE_RETRY_INTERVAL: Duration = Duration::from_secs(1);

struct MobileLogger;

impl log::Log for MobileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            let message = format!(
                "[stereodrome-rust][{}][{}] {}",
                record.level(),
                record.target(),
                record.args()
            );
            if let Some(callback) = LOG_CALLBACK.lock().ok().and_then(|guard| *guard)
                && let Ok(message) = CString::new(message.as_str())
            {
                callback(message.as_ptr());
                return;
            }
            let mut stderr = std::io::stderr().lock();
            let _ = stderr.write_all(message.as_bytes());
            let _ = stderr.write_all(b"\n");
        }
    }

    fn flush(&self) {}
}

fn init_mobile_logging() {
    INIT_LOGGER.call_once(|| {
        if log::set_logger(&MOBILE_LOGGER).is_ok() {
            log::set_max_level(LevelFilter::Debug);
        }
    });
    INIT_PANIC_HOOK.call_once(|| {
        panic::set_hook(Box::new(|panic_info| {
            let location = panic_info.location().map_or_else(
                || "unknown location".to_string(),
                |location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                },
            );
            log::error!(
                target: "stereodrome_ffi",
                "Rust panic at {location}: {}",
                panic_payload_message(panic_info.payload())
            );
        }));
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_set_log_callback(callback: Option<MobileLogCallback>) {
    init_mobile_logging();
    if let Ok(mut current) = LOG_CALLBACK.lock() {
        *current = callback;
    }
}

#[derive(Clone, Copy)]
struct InstanceEventCallback {
    callback: MobileEventCallback,
    context: usize,
}

pub struct MobileCore {
    core: Arc<StereodromeCore>,
    core_runtime: StereodromeRuntimeHandle,
    announcer: PlaybackAnnouncer,
    event_emitter: MobileEventEmitter,
    runtime: tokio::runtime::Runtime,
    monitor_running: Arc<AtomicBool>,
    monitor_thread: Option<thread::JoinHandle<()>>,
    runtime_event_thread: Option<thread::JoinHandle<()>>,
}

#[derive(Clone)]
struct PlaybackAnnouncer {
    sequencer: Arc<Mutex<PlaybackSnapshotSequencer>>,
    file_state: Arc<Mutex<MobileFileStateSnapshot>>,
    event_emitter: MobileEventEmitter,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
struct MobileFileStateSnapshot {
    seq: u64,
    downloaded_song_ids: Vec<String>,
    downloading_song_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "kebab-case")]
enum MobileCoreEvent {
    Runtime(Box<CoreEvent>),
    PlatformProjection(Box<MobilePlatformProjection>),
}

#[derive(Serialize)]
struct MobileCoreEventEnvelope {
    stream_id: u64,
    seq: u64,
    #[serde(flatten)]
    event: MobileCoreEvent,
}

#[derive(Serialize)]
struct MobilePlatformProjection {
    protocol_version: u32,
    revision: u64,
    projection: PlaybackProjection,
}

#[derive(Clone)]
struct MobileEventEmitter {
    stream_id: u64,
    next_seq: Arc<Mutex<u64>>,
    callback: Arc<Mutex<Option<InstanceEventCallback>>>,
}

impl MobileEventEmitter {
    fn new() -> Self {
        Self {
            stream_id: NEXT_EVENT_STREAM_ID.fetch_add(1, Ordering::Relaxed),
            next_seq: Arc::new(Mutex::new(1)),
            callback: Arc::new(Mutex::new(None)),
        }
    }

    fn set_callback(&self, callback: Option<MobileEventCallback>, context: *mut c_void) {
        if let Ok(mut current) = self.callback.lock() {
            *current = callback.map(|callback| InstanceEventCallback {
                callback,
                context: context as usize,
            });
        }
    }

    fn emit(&self, build: impl FnOnce() -> Result<MobileCoreEvent, String>) -> bool {
        let Ok(mut next_seq) = self.next_seq.lock() else {
            log::warn!(target: "stereodrome_ffi", "Mobile event sequencer lock is poisoned");
            return false;
        };
        let event = match build() {
            Ok(event) => event,
            Err(error) => {
                log::warn!(target: "stereodrome_ffi", "Failed to build mobile event: {error}");
                return false;
            }
        };
        let envelope = MobileCoreEventEnvelope {
            stream_id: self.stream_id,
            seq: *next_seq,
            event,
        };
        *next_seq += 1;
        let message = match serde_json::to_string(&envelope)
            .map_err(|error| error.to_string())
            .and_then(|json| CString::new(json).map_err(|error| error.to_string()))
        {
            Ok(message) => message,
            Err(error) => {
                log::warn!(target: "stereodrome_ffi", "Failed to serialize mobile event: {error}");
                return false;
            }
        };
        // Keep the callback guard through invocation so clearing a callback is a
        // synchronous lifetime barrier for its context pointer.
        if let Ok(current) = self.callback.lock()
            && let Some(callback) = *current
        {
            (callback.callback)(message.as_ptr(), callback.context as *mut c_void);
        }
        true
    }
}

struct PlaybackSnapshotSequencer {
    next_seq: u64,
    last_emitted_revision: Option<u64>,
}

impl PlaybackSnapshotSequencer {
    fn new() -> Self {
        Self {
            next_seq: 1,
            last_emitted_revision: None,
        }
    }

    fn sequence<T>(&mut self, build: impl FnOnce(u64) -> T) -> T {
        let seq = self.next_seq;
        self.next_seq += 1;
        build(seq)
    }
}

impl PlaybackAnnouncer {
    fn new(core: &StereodromeCore, event_emitter: MobileEventEmitter) -> (Self, bool) {
        let announcer = Self {
            sequencer: Arc::new(Mutex::new(PlaybackSnapshotSequencer::new())),
            file_state: Arc::new(Mutex::new(MobileFileStateSnapshot::default())),
            event_emitter,
        };
        let initialized = match announcer.refresh_file_state(core) {
            Ok(_) => true,
            Err(error) => {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Failed to initialize mobile file state: {error}"
                );
                false
            }
        };
        (announcer, initialized)
    }

    fn refresh_file_state(&self, core: &StereodromeCore) -> Result<bool, String> {
        let mut downloaded_song_ids = core
            .get_offline_song_ids()
            .map_err(|error| error.to_string())?;
        downloaded_song_ids.sort_unstable();
        let downloading_song_ids = core.get_downloading_song_ids();
        self.file_state
            .lock()
            .map_err(|_| "mobile file state lock is poisoned".to_string())
            .map(|mut current| {
                if current.downloaded_song_ids == downloaded_song_ids
                    && current.downloading_song_ids == downloading_song_ids
                {
                    false
                } else {
                    current.seq += 1;
                    current.downloaded_song_ids = downloaded_song_ids;
                    current.downloading_song_ids = downloading_song_ids;
                    true
                }
            })
    }

    fn apply_cache_state_event(
        &self,
        core: &StereodromeCore,
        event: CacheStateEvent,
    ) -> Result<bool, String> {
        match event {
            CacheStateEvent::DownloadingChanged {
                song_id,
                downloading,
            } => self
                .file_state
                .lock()
                .map_err(|_| "mobile file state lock is poisoned".to_string())
                .map(|mut current| {
                    let changed = update_sorted_song_ids(
                        &mut current.downloading_song_ids,
                        song_id,
                        downloading,
                    );
                    current.seq += u64::from(changed);
                    changed
                }),
            CacheStateEvent::CachedChanged { song_id, cached } => {
                let cached = if cached {
                    core.has_library_song(&song_id)
                        .map_err(|error| error.to_string())?
                } else {
                    false
                };
                self.file_state
                    .lock()
                    .map_err(|_| "mobile file state lock is poisoned".to_string())
                    .map(|mut current| {
                        let changed = update_sorted_song_ids(
                            &mut current.downloaded_song_ids,
                            song_id,
                            cached,
                        );
                        current.seq += u64::from(changed);
                        changed
                    })
            }
            CacheStateEvent::Reconcile => self.refresh_file_state(core),
        }
    }

    fn file_state_snapshot(&self) -> MobileFileStateSnapshot {
        self.file_state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default()
    }

    fn snapshot(&self, runtime: &StereodromeRuntimeHandle) -> Result<PlaybackSnapshot, String> {
        let value = runtime_command_value(runtime.snapshot())?;
        let snapshot = serde_json::from_value::<CoreSnapshot>(value).map_err(|e| e.to_string())?;
        let mut sequencer = self
            .sequencer
            .lock()
            .map_err(|_| "playback snapshot sequencer lock poisoned".to_string())?;
        sequencer.last_emitted_revision = Some(
            sequencer
                .last_emitted_revision
                .map_or(snapshot.revision, |revision| {
                    revision.max(snapshot.revision)
                }),
        );
        Ok(sequencer.sequence(|seq| PlaybackSnapshot::from_projection(seq, snapshot.playback)))
    }

    fn emit(&self, revision: u64, projection: PlaybackProjection) -> bool {
        let result: Result<bool, String> = (|| {
            let mut sequencer = self
                .sequencer
                .lock()
                .map_err(|_| "playback snapshot sequencer lock poisoned".to_string())?;
            if sequencer
                .last_emitted_revision
                .is_some_and(|last_revision| revision <= last_revision)
            {
                return Ok(false);
            }
            sequencer.last_emitted_revision = Some(revision);
            Ok(self.event_emitter.emit(|| {
                Ok(MobileCoreEvent::PlatformProjection(Box::new(
                    MobilePlatformProjection {
                        protocol_version: CORE_PROTOCOL_VERSION,
                        revision,
                        projection,
                    },
                )))
            }))
        })();

        match result {
            Ok(emitted) => emitted,
            Err(error) => {
                log::warn!(target: "stereodrome_ffi", "Failed to emit platform projection: {error}");
                false
            }
        }
    }

    fn emit_current(&self, revision: u64, projection: PlaybackProjection) -> bool {
        let result: Result<bool, String> = (|| {
            let mut sequencer = self
                .sequencer
                .lock()
                .map_err(|_| "playback snapshot sequencer lock poisoned".to_string())?;
            sequencer.last_emitted_revision = Some(
                sequencer
                    .last_emitted_revision
                    .map_or(revision, |last_revision| last_revision.max(revision)),
            );
            Ok(self.event_emitter.emit(|| {
                Ok(MobileCoreEvent::PlatformProjection(Box::new(
                    MobilePlatformProjection {
                        protocol_version: CORE_PROTOCOL_VERSION,
                        revision,
                        projection,
                    },
                )))
            }))
        })();

        match result {
            Ok(emitted) => emitted,
            Err(error) => {
                log::warn!(target: "stereodrome_ffi", "Failed to emit current platform projection: {error}");
                false
            }
        }
    }
}

fn update_sorted_song_ids(song_ids: &mut Vec<String>, song_id: String, present: bool) -> bool {
    match (song_ids.binary_search(&song_id), present) {
        (Ok(_), true) | (Err(_), false) => false,
        (Ok(index), false) => {
            song_ids.remove(index);
            true
        }
        (Err(index), true) => {
            song_ids.insert(index, song_id);
            true
        }
    }
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct PlaybackSnapshot {
    seq: u64,
    state: &'static str,
    is_playing: bool,
    audio_loaded: bool,
    output_state: String,
    song: Option<PlaybackSnapshotSong>,
    position_seconds: f64,
    duration_seconds: f64,
    volume: f32,
    queue: QueueState,
    queue_index: Option<usize>,
    queue_length: usize,
    can_play: bool,
    can_next: bool,
    can_previous: bool,
    can_seek: bool,
}

#[derive(Debug, Serialize)]
struct PlaybackSnapshotSong {
    id: String,
    title: String,
    artist: String,
    album: String,
    duration_seconds: f64,
    artwork_uri: Option<String>,
}

impl PlaybackSnapshot {
    fn from_projection(seq: u64, projection: PlaybackProjection) -> Self {
        Self {
            seq,
            state: match projection.state {
                PlaybackPhase::Playing => "playing",
                PlaybackPhase::Paused => "paused",
                PlaybackPhase::Stalled => "stalled",
                PlaybackPhase::Stopped => "stopped",
            },
            is_playing: projection.is_playing,
            audio_loaded: projection.audio_loaded,
            output_state: match projection.output_state {
                PlaybackOutputState::Closed => "closed",
                PlaybackOutputState::Ready => "ready",
                PlaybackOutputState::Failed => "failed",
                PlaybackOutputState::Unavailable => "unavailable",
            }
            .to_string(),
            song: projection.song.map(|song| PlaybackSnapshotSong {
                id: song.id,
                title: song.title,
                artist: song.artist,
                album: song.album,
                duration_seconds: song.duration_seconds,
                artwork_uri: song.artwork_uri,
            }),
            position_seconds: projection.position_seconds,
            duration_seconds: projection.duration_seconds,
            volume: projection.volume,
            queue: projection.queue,
            queue_index: projection.queue_index,
            queue_length: projection.queue_length,
            can_play: projection.can_play,
            can_next: projection.can_next,
            can_previous: projection.can_previous,
            can_seek: projection.can_seek,
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `value` must be a pointer returned by this library from a previous FFI call
/// and must not have been freed already.
pub unsafe extern "C" fn stereodrome_core_free_string(value: *mut c_char) {
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        if value.is_null() {
            return;
        }

        unsafe {
            let _ = CString::from_raw(value);
        }
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_new(data_dir: *const c_char) -> *mut MobileCore {
    init_mobile_logging();

    match panic::catch_unwind(AssertUnwindSafe(|| {
        let Some(data_dir) = read_c_string(data_dir) else {
            return ptr::null_mut();
        };

        let data_dir = PathBuf::from(data_dir);
        let (cache_event_sender, cache_event_receiver) = std::sync::mpsc::channel();
        match (
            StereodromeCore::new_with_cache_events(&data_dir, cache_event_sender.clone()),
            tokio::runtime::Runtime::new(),
        ) {
            (Ok(core), Ok(runtime)) => {
                let core = Arc::new(core);
                let Ok(core_runtime) =
                    StereodromeRuntimeHandle::start_with_core(&data_dir, Arc::clone(&core))
                else {
                    return ptr::null_mut();
                };
                let event_emitter = MobileEventEmitter::new();
                let (announcer, file_state_initialized) =
                    PlaybackAnnouncer::new(&core, event_emitter.clone());
                let monitor_running = Arc::new(AtomicBool::new(true));
                let runtime_event_thread = start_runtime_event_bridge(
                    core_runtime.subscribe(),
                    event_emitter.clone(),
                    announcer.clone(),
                    Arc::clone(&monitor_running),
                );
                let monitor_thread = start_mobile_cache_monitor(
                    Arc::clone(&core),
                    cache_event_receiver,
                    announcer.clone(),
                    Arc::clone(&monitor_running),
                    !file_state_initialized,
                );

                Box::into_raw(Box::new(MobileCore {
                    core,
                    core_runtime,
                    announcer,
                    event_emitter,
                    runtime,
                    monitor_running,
                    monitor_thread: Some(monitor_thread),
                    runtime_event_thread: Some(runtime_event_thread),
                }))
            }
            _ => ptr::null_mut(),
        }
    })) {
        Ok(core) => core,
        Err(payload) => {
            log::error!(
                target: "stereodrome_ffi",
                "Rust panic while initializing mobile core: {}",
                panic_payload_message(payload.as_ref())
            );
            ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `core` must be a pointer returned by `stereodrome_core_new` and must not be
/// used after this function returns.
pub unsafe extern "C" fn stereodrome_core_destroy(core: *mut MobileCore) {
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        if core.is_null() {
            return;
        }

        unsafe {
            let mut mobile = Box::from_raw(core);
            mobile.event_emitter.set_callback(None, ptr::null_mut());
            mobile.monitor_running.store(false, Ordering::SeqCst);
            if let Some(monitor_thread) = mobile.monitor_thread.take()
                && monitor_thread.join().is_err()
            {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Mobile playback monitor panicked during shutdown"
                );
            }
            if let Some(runtime_event_thread) = mobile.runtime_event_thread.take()
                && runtime_event_thread.join().is_err()
            {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Mobile runtime event bridge panicked during shutdown"
                );
            }
            mobile.core_runtime.shutdown();
        }
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_get_connection_status(core: *mut MobileCore) -> *mut c_char {
    catch_json_response(|| stereodrome_core_get_connection_status_inner(core))
}

fn stereodrome_core_get_connection_status_inner(core: *mut MobileCore) -> *mut c_char {
    let Some(mobile) = mobile_ref(core) else {
        return json_error("core is not initialized");
    };

    match mobile.core.get_connection_status() {
        Ok(status) => json_ok(status),
        Err(error) => json_error(&error.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_get_stream_uri(
    core: *mut MobileCore,
    song_id: *const c_char,
) -> *mut c_char {
    catch_json_response(|| stereodrome_core_get_stream_uri_inner(core, song_id))
}

fn stereodrome_core_get_stream_uri_inner(
    core: *mut MobileCore,
    song_id: *const c_char,
) -> *mut c_char {
    let Some(mobile) = mobile_ref(core) else {
        return json_error("core is not initialized");
    };
    let Some(song_id) = read_c_string(song_id) else {
        return json_error("song_id is required");
    };

    match mobile.core.get_stream_uri(song_id) {
        Ok(uri) => json_ok(uri),
        Err(error) => json_error(&error.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_call(
    core: *mut MobileCore,
    method: *const c_char,
    payload: *const c_char,
) -> *mut c_char {
    catch_json_response(|| stereodrome_core_call_inner(core, method, payload))
}

/// Creates the phase-one runtime boundary. This is an ABI-compatible alias for
/// the existing mobile constructor while adapters migrate to typed dispatch.
#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_runtime_new(data_dir: *const c_char) -> *mut MobileCore {
    stereodrome_core_new(data_dir)
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_runtime_set_event_callback(
    runtime: *mut MobileCore,
    callback: Option<MobileEventCallback>,
    context: *mut c_void,
) {
    if let Some(mobile) = mobile_ref(runtime) {
        mobile.event_emitter.set_callback(callback, context);
        if callback.is_some()
            && let Ok(snapshot) = runtime_snapshot(&mobile.core_runtime)
        {
            mobile
                .announcer
                .emit_current(snapshot.revision, snapshot.playback);
        }
    }
}

/// # Safety
///
/// `runtime` must be a pointer returned by `stereodrome_runtime_new` and must
/// not be used after this function returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stereodrome_runtime_destroy(runtime: *mut MobileCore) {
    unsafe { stereodrome_core_destroy(runtime) };
}

/// Dispatches one versioned [`CoreCommandRequest`] through the runtime mailbox.
#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_runtime_dispatch(
    runtime: *mut MobileCore,
    command_json: *const c_char,
) -> *mut c_char {
    catch_json_response(|| stereodrome_runtime_dispatch_inner(runtime, command_json))
}

fn stereodrome_runtime_dispatch_inner(
    runtime: *mut MobileCore,
    command_json: *const c_char,
) -> *mut c_char {
    let Some(mobile) = mobile_ref(runtime) else {
        return into_c_string(serialize_protocol_result(&protocol_input_error(
            "runtime is not initialized",
        )));
    };
    let Some(command_json) = read_c_string(command_json) else {
        return into_c_string(serialize_protocol_result(&protocol_input_error(
            "command JSON is required",
        )));
    };
    let request = match serde_json::from_str::<CoreCommandRequest>(&command_json) {
        Ok(request) => request,
        Err(error) => {
            return into_c_string(serialize_protocol_result(&protocol_input_error(&format!(
                "invalid command JSON: {error}"
            ))));
        }
    };
    let changes_playback = request.command.changes_playback_projection();
    let result = mobile.core_runtime.dispatch(request);
    if result.status == CommandStatus::Succeeded
        && changes_playback
        && let Ok(snapshot) = runtime_snapshot(&mobile.core_runtime)
    {
        mobile.announcer.emit(snapshot.revision, snapshot.playback);
    }
    into_c_string(serialize_protocol_result(&result))
}

/// Reads an authoritative snapshot through the runtime mailbox.
#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_runtime_snapshot(runtime: *mut MobileCore) -> *mut c_char {
    catch_json_response(|| {
        let Some(mobile) = mobile_ref(runtime) else {
            return into_c_string(serialize_protocol_result(&protocol_input_error(
                "runtime is not initialized",
            )));
        };
        into_c_string(serialize_protocol_result(&mobile.core_runtime.snapshot()))
    })
}

/// # Safety
///
/// `value` must have been returned by this library and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stereodrome_runtime_string_free(value: *mut c_char) {
    unsafe { stereodrome_core_free_string(value) };
}

fn serialize_protocol_result(result: &CoreCommandResult) -> String {
    serde_json::to_string(result).unwrap_or_else(|error| {
        serde_json::json!({
            "protocol_version": CORE_PROTOCOL_VERSION,
            "command_id": 0,
            "accepted_revision": 0,
            "operation_id": null,
            "status": "failed",
            "error": {
                "code": "internal",
                "message": format!("failed to serialize command result: {error}"),
                "retryable": false
            }
        })
        .to_string()
    })
}

fn protocol_input_error(message: &str) -> CoreCommandResult {
    CoreCommandResult::failed(
        CommandId(0),
        0,
        None,
        ProtocolError::new(ProtocolErrorCode::InvalidInput, message, false),
    )
}

fn stereodrome_core_call_inner(
    core: *mut MobileCore,
    method: *const c_char,
    payload: *const c_char,
) -> *mut c_char {
    let Some(mobile) = mobile_ref(core) else {
        return json_error("core is not initialized");
    };
    let Some(method) = read_c_string(method) else {
        return json_error("method is required");
    };
    let payload = read_c_string(payload).unwrap_or_else(|| "null".to_string());
    let payload = match serde_json::from_str::<Value>(&payload) {
        Ok(payload) => payload,
        Err(error) => return json_error(&format!("invalid payload: {error}")),
    };

    match dispatch(mobile, &method, payload) {
        Ok(value) => into_c_string(value),
        Err(error) => json_error(&error),
    }
}

#[allow(clippy::too_many_lines)]
fn dispatch(mobile: &MobileCore, method: &str, payload: Value) -> Result<String, String> {
    let runtime = &mobile.runtime;
    let core = &mobile.core;

    if let Some(command) = legacy_runtime_command(method, payload.clone())? {
        let changes_playback = command.changes_playback_projection();
        let response = legacy_runtime_result_for_method(
            method,
            mobile.core_runtime.dispatch_command(command),
            &mobile.core_runtime,
        );
        return finish_dispatch(mobile, changes_playback, response);
    }

    let response = match method {
        "connectServer" => {
            let params = parse_payload::<ConnectParams>(payload)?;
            json_result(runtime.block_on(async { core.connect_server(params).await }))
        }
        "updateServerSettings" => {
            let update = parse_payload::<ServerSettingsUpdate>(payload)?;
            json_result(runtime.block_on(async { core.update_server_settings(update).await }))
        }
        "restoreSession" => json_result(runtime.block_on(async { core.restore_session().await })),
        "disconnectServer" => {
            json_result(runtime.block_on(async { core.disconnect_server().await }))
        }
        "getConnectionStatus" => json_result(core.get_connection_status()),
        "getSyncSettings" => json_result(core.get_sync_settings()),
        "setSyncSettings" => {
            let settings = parse_payload::<SyncSettings>(payload)?;
            json_result(core.set_sync_settings(settings))
        }
        "getConnectivitySettings" => json_result(core.get_connectivity_settings()),
        "setConnectivitySettings" => {
            let settings = parse_payload::<ConnectivitySettings>(payload)?;
            json_result(core.set_connectivity_settings(settings))
        }
        "getScanStatus" => json_result(runtime.block_on(async { core.get_scan_status().await })),
        "startScan" => json_result(runtime.block_on(async { core.start_scan().await })),
        "getArtists" => json_result(core.get_artists()),
        "getAlbums" => {
            let artist_id = parse_optional_string(payload)?;
            json_result(core.get_albums(artist_id))
        }
        "getSongs" => {
            let args = parse_payload::<TwoOptionalStrings>(payload)?;
            json_result(core.get_songs(args.first, args.second))
        }
        "getAlbumList" => {
            let args = parse_payload::<AlbumListPayload>(payload)?;
            json_result(runtime.block_on(async {
                core.get_album_list(args.list_type, args.size, args.offset)
                    .await
            }))
        }
        "searchLibrary" => {
            let args = parse_payload::<SearchPayload>(payload)?;
            json_result(core.search_library(args.query, args.limit))
        }
        "getPlaylists" => json_result(runtime.block_on(async { core.get_playlists().await })),
        "getPlaylistSongs" => {
            let playlist_id = parse_payload::<String>(payload)?;
            json_result(runtime.block_on(async { core.get_playlist_songs(playlist_id).await }))
        }
        "createPlaylist" => {
            let args = parse_payload::<CreatePlaylistPayload>(payload)?;
            json_result(
                runtime.block_on(async { core.create_playlist(args.name, args.song_ids).await }),
            )
        }
        "renamePlaylist" => {
            let args = parse_payload::<RenamePlaylistPayload>(payload)?;
            json_result(
                runtime.block_on(async { core.rename_playlist(args.playlist_id, args.name).await }),
            )
        }
        "deletePlaylist" => {
            let playlist_id = parse_payload::<String>(payload)?;
            json_result(runtime.block_on(async { core.delete_playlist(playlist_id).await }))
        }
        "addSongsToPlaylist" => {
            let args = parse_payload::<PlaylistSongIdsPayload>(payload)?;
            json_result(runtime.block_on(async {
                core.add_songs_to_playlist(args.playlist_id, args.song_ids)
                    .await
            }))
        }
        "removeSongsFromPlaylist" => {
            let args = parse_payload::<PlaylistSongIndexesPayload>(payload)?;
            json_result(runtime.block_on(async {
                core.remove_songs_from_playlist(args.playlist_id, args.song_indexes)
                    .await
            }))
        }
        "getCoverArtUri" => {
            let args = parse_payload::<IdSizePayload>(payload)?;
            json_result(
                runtime.block_on(async { core.get_cover_art_uri(args.id, args.size).await }),
            )
        }
        "getSongCoverArtUri" => {
            let args = parse_payload::<IdSizePayload>(payload)?;
            json_result(
                runtime.block_on(async { core.get_song_cover_art_uri(args.id, args.size).await }),
            )
        }
        "getStreamUri" => {
            let song_id = parse_payload::<String>(payload)?;
            json_result(core.get_stream_uri(song_id))
        }
        "getAudioCacheStats" => json_result(core.get_audio_cache_stats()),
        "getOfflineSongIds" => json_result(core.get_offline_song_ids()),
        "setMaxCacheSize" => {
            let max_size = parse_payload::<u64>(payload)?;
            json_result(core.set_max_cache_size(max_size))
        }
        "clearAudioCache" => json_result(core.clear_audio_cache()),
        "isSongCached" => {
            let song_id = parse_payload::<String>(payload)?;
            json_result(core.is_song_cached(song_id))
        }
        "downloadSong" => {
            let song_id = parse_payload::<String>(payload)?;
            json_result(runtime.block_on(async { core.download_song(song_id).await }))
        }
        "removeCachedSong" => {
            let song_id = parse_payload::<String>(payload)?;
            json_result(core.remove_cached_song(song_id))
        }
        "downloadAlbum" => {
            let album_id = parse_payload::<String>(payload)?;
            json_result(runtime.block_on(async { core.download_album(album_id).await }))
        }
        "downloadPlaylist" => {
            let playlist_id = parse_payload::<String>(payload)?;
            json_result(runtime.block_on(async { core.download_playlist(playlist_id).await }))
        }
        "getPlaybackState" => json_result(core.get_playback_state()),
        "getPlaybackSnapshot" => json_result(mobile.announcer.snapshot(&mobile.core_runtime)),
        "getEventStreamId" => json_result(Ok::<_, String>(mobile.event_emitter.stream_id)),
        "getFileStateSnapshot" => {
            json_result(Ok::<_, String>(mobile.announcer.file_state_snapshot()))
        }
        "savePlaybackPosition" => {
            let progress = parse_payload::<PlaybackProgress>(payload)?;
            json_result(core.save_playback_position(progress))
        }
        "getLastfmStatus" => json_result(Ok::<_, String>(core.get_lastfm_status())),
        "beginLastfmAuth" => {
            json_result(runtime.block_on(async { core.begin_lastfm_auth().await }))
        }
        "completeLastfmAuth" => {
            json_result(runtime.block_on(async { core.complete_lastfm_auth().await }))
        }
        "disconnectLastfm" => json_result(core.disconnect_lastfm()),
        "getLastfmQueue" => json_result(core.get_lastfm_queue()),
        "retryLastfmQueue" => {
            json_result(runtime.block_on(async { core.retry_lastfm_queue().await }))
        }
        "getAudioProcessingSettings" => json_result(core.get_audio_processing_settings()),
        "playSongWithQueue" => {
            let args = parse_payload::<PlaySongWithQueuePayload>(payload)?;
            let result = core.play_song_with_queue(args.song_id, args.song_ids);
            json_result(result)
        }
        "addToQueue" => {
            let item = parse_payload::<QueueItem>(payload)?;
            let result = core.add_to_queue(item);
            json_result(result)
        }
        "addSongsToQueue" => {
            let items = parse_payload::<Vec<QueueItem>>(payload)?;
            let result = core.add_songs_to_queue(items);
            json_result(result)
        }
        "insertNext" => {
            let item = parse_payload::<QueueItem>(payload)?;
            let result = core.insert_next(item);
            json_result(result)
        }
        "insertNextSongs" => {
            let items = parse_payload::<Vec<QueueItem>>(payload)?;
            let result = core.insert_next_songs(items);
            json_result(result)
        }
        "removeFromQueue" => {
            let index = parse_payload::<usize>(payload)?;
            let result = core.remove_from_queue(index);
            json_result(result)
        }
        "clearQueue" => {
            let result = core.clear_queue();
            json_result(result)
        }
        "moveQueueItem" => {
            let args = parse_payload::<MoveQueueItemPayload>(payload)?;
            let result = core.move_queue_item(args.from, args.to);
            json_result(result)
        }
        "playQueueItem" => {
            let index = parse_payload::<usize>(payload)?;
            let result = core.play_queue_item(index);
            json_result(result)
        }
        "playNext" => {
            let force = parse_payload::<Option<bool>>(payload)?;
            let result = core.play_next(force);
            json_result(result)
        }
        "playPrevious" => {
            let result = core.play_previous();
            json_result(result)
        }
        "toggleShuffle" => {
            let result = core.toggle_shuffle();
            json_result(result)
        }
        "setRepeatMode" => {
            let mode = parse_payload::<RepeatMode>(payload)?;
            let result = core.set_repeat_mode(mode);
            json_result(result)
        }
        "cycleRepeatMode" => {
            let result = core.cycle_repeat_mode();
            json_result(result)
        }
        "rerollNext" => {
            let result = core.reroll_next();
            json_result(result)
        }
        other => Err(format!("unknown method: {other}")),
    };

    finish_dispatch(mobile, false, response)
}

fn finish_dispatch(
    mobile: &MobileCore,
    changes_playback: bool,
    response: Result<String, String>,
) -> Result<String, String> {
    if response.is_ok()
        && changes_playback
        && let Ok(snapshot) =
            runtime_command_value(mobile.core_runtime.snapshot()).and_then(|value| {
                serde_json::from_value::<CoreSnapshot>(value).map_err(|e| e.to_string())
            })
    {
        mobile.announcer.emit(snapshot.revision, snapshot.playback);
    }

    response
}

fn legacy_runtime_result_for_method(
    method: &str,
    result: CoreCommandResult,
    runtime: &StereodromeRuntimeHandle,
) -> Result<String, String> {
    let value = runtime_command_value(result)?;
    if matches!(
        method,
        "audioPlayCurrent"
            | "audioPlayQueueItem"
            | "audioPlayNext"
            | "audioPlayPrevious"
            | "audioApplySettings"
    ) {
        let snapshot = runtime_snapshot(runtime)?;
        let playback = snapshot.playback;
        return Ok(json_ok_string(serde_json::json!({
            "state": playback.state,
            "is_playing": playback.is_playing,
            "current_song_id": playback.song.map(|song| song.id),
            "position": playback.position_seconds,
            "duration": playback.duration_seconds,
            "volume": playback.volume,
            "output_state": playback.output_state,
        })));
    }
    if method == "clearQueue" {
        return Ok(json_ok_string(runtime_snapshot(runtime)?.queue));
    }
    Ok(json_ok_string(value))
}

fn runtime_snapshot(runtime: &StereodromeRuntimeHandle) -> Result<CoreSnapshot, String> {
    let value = runtime_command_value(runtime.snapshot())?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn runtime_command_value(result: CoreCommandResult) -> Result<Value, String> {
    match result.status {
        CommandStatus::Succeeded => Ok(result.value.unwrap_or(Value::Null)),
        CommandStatus::Failed => Err(result.error.map_or_else(
            || "runtime command failed".to_string(),
            |error| error.message,
        )),
    }
}

#[allow(clippy::too_many_lines)]
fn legacy_runtime_command(method: &str, payload: Value) -> Result<Option<CoreCommand>, String> {
    let command = match method {
        "connectServer" => CoreCommand::Connect {
            params: parse_payload(payload)?,
        },
        "updateServerSettings" => CoreCommand::UpdateServerSettings {
            update: parse_payload(payload)?,
        },
        "restoreSession" => CoreCommand::RestoreSession,
        "exportPortableBackup" => CoreCommand::ExportPortableBackup {
            path: parse_payload(payload)?,
        },
        "importPortableBackup" => CoreCommand::ImportPortableBackup {
            path: parse_payload(payload)?,
        },
        "syncLibrary" => CoreCommand::StartSync {
            kind: SyncKind::FullReconcile,
        },
        "syncLibraryIncremental" => CoreCommand::StartSync {
            kind: SyncKind::Incremental,
        },
        "disconnectServer" => CoreCommand::Disconnect,
        "getConnectionStatus" => CoreCommand::GetConnectionStatus,
        "getSyncSettings" => CoreCommand::GetSyncSettings,
        "setSyncSettings" => CoreCommand::SetSyncSettings {
            settings: parse_payload(payload)?,
        },
        "getConnectivitySettings" => CoreCommand::GetConnectivitySettings,
        "setConnectivitySettings" => CoreCommand::SetConnectivity {
            settings: parse_payload(payload)?,
        },
        "runDueLibrarySync" => CoreCommand::RunBackgroundTick,
        "getScanStatus" => CoreCommand::GetScanStatus,
        "startScan" => CoreCommand::StartScan,
        "getLibrarySyncStatus" => CoreCommand::GetLibrarySyncStatus,
        "getArtists" => CoreCommand::GetArtists,
        "getAlbums" => CoreCommand::GetAlbums {
            artist_id: parse_optional_string(payload)?,
        },
        "getSongs" => {
            let args = parse_payload::<TwoOptionalStrings>(payload)?;
            CoreCommand::GetSongs {
                album_id: args.first,
                artist_id: args.second,
            }
        }
        "getAlbumList" => {
            let args = parse_payload::<AlbumListPayload>(payload)?;
            CoreCommand::GetAlbumList {
                list_type: args.list_type,
                size: args.size,
                offset: args.offset,
            }
        }
        "searchLibrary" => {
            let args = parse_payload::<SearchPayload>(payload)?;
            CoreCommand::SearchLibrary {
                query: args.query,
                limit: args.limit,
            }
        }
        "getPlaylists" => CoreCommand::GetPlaylists,
        "getPlaylistSongs" => CoreCommand::GetPlaylistSongs {
            playlist_id: parse_payload(payload)?,
        },
        "createPlaylist" => {
            let args = parse_payload::<CreatePlaylistPayload>(payload)?;
            CoreCommand::CreatePlaylist {
                name: args.name,
                song_ids: args.song_ids,
            }
        }
        "renamePlaylist" => {
            let args = parse_payload::<RenamePlaylistPayload>(payload)?;
            CoreCommand::RenamePlaylist {
                playlist_id: args.playlist_id,
                name: args.name,
            }
        }
        "deletePlaylist" => CoreCommand::DeletePlaylist {
            playlist_id: parse_payload(payload)?,
        },
        "addSongsToPlaylist" => {
            let args = parse_payload::<PlaylistSongIdsPayload>(payload)?;
            CoreCommand::AddSongsToPlaylist {
                playlist_id: args.playlist_id,
                song_ids: args.song_ids,
            }
        }
        "removeSongsFromPlaylist" => {
            let args = parse_payload::<PlaylistSongIndexesPayload>(payload)?;
            CoreCommand::RemoveSongsFromPlaylist {
                playlist_id: args.playlist_id,
                song_indexes: args.song_indexes,
            }
        }
        "getCoverArtUri" => {
            let args = parse_payload::<IdSizePayload>(payload)?;
            CoreCommand::GetCoverArtUri {
                id: args.id,
                size: args.size,
            }
        }
        "getSongCoverArtUri" => {
            let args = parse_payload::<IdSizePayload>(payload)?;
            CoreCommand::GetSongCoverArtUri {
                id: args.id,
                size: args.size,
            }
        }
        "getStreamUri" => CoreCommand::GetStreamUri {
            song_id: parse_payload(payload)?,
        },
        "getAudioCacheStats" => CoreCommand::GetAudioCacheStats,
        "getOfflineSongIds" => CoreCommand::GetOfflineSongIds,
        "setMaxCacheSize" => CoreCommand::SetMaxCacheSize {
            max_size: parse_payload(payload)?,
        },
        "clearAudioCache" => CoreCommand::ClearAudioCache,
        "isSongCached" => CoreCommand::IsSongCached {
            song_id: parse_payload(payload)?,
        },
        "downloadSong" => CoreCommand::DownloadSong {
            song_id: parse_payload(payload)?,
        },
        "removeCachedSong" => CoreCommand::RemoveCachedSong {
            song_id: parse_payload(payload)?,
        },
        "downloadAlbum" => CoreCommand::DownloadAlbum {
            album_id: parse_payload(payload)?,
        },
        "downloadPlaylist" => CoreCommand::DownloadPlaylist {
            playlist_id: parse_payload(payload)?,
        },
        "setPlaylistSavedOffline" => {
            let args = parse_payload::<SetPlaylistSavedOfflinePayload>(payload)?;
            CoreCommand::SetPlaylistSavedOffline {
                playlist_id: args.playlist_id,
                saved_offline: args.saved_offline,
            }
        }
        "reconcileSavedPlaylistsOffline" => CoreCommand::ReconcileSavedPlaylistsOffline,
        "startSavedPlaylistsOfflineReconcile" => CoreCommand::StartSavedPlaylistsOfflineReconcile,
        "getSavedPlaylistsOfflineReconcileStatus" => CoreCommand::GetSavedPlaylistsOfflineStatus,
        "prefetchNext" => {
            let args = if payload.is_null() {
                PrefetchPayload::default()
            } else {
                parse_payload::<PrefetchPayload>(payload)?
            };
            CoreCommand::StartQueuePrefetch {
                reserve_first: args.reserve_first,
            }
        }
        "getPlaybackState" => CoreCommand::GetPlaybackState,
        "savePlaybackPosition" => CoreCommand::SavePlaybackPosition {
            progress: parse_payload(payload)?,
        },
        "getLastfmStatus" => CoreCommand::GetLastfmStatus,
        "beginLastfmAuth" => CoreCommand::BeginLastfmAuth,
        "completeLastfmAuth" => CoreCommand::CompleteLastfmAuth,
        "disconnectLastfm" => CoreCommand::DisconnectLastfm,
        "getLastfmQueue" => CoreCommand::GetLastfmQueue,
        "retryLastfmQueue" => CoreCommand::RetryLastfmQueue,
        "getAudioProcessingSettings" => CoreCommand::GetAudioProcessingSettings,
        "setAudioProcessingSettings" => CoreCommand::SetAudioProcessing {
            settings: parse_payload(payload)?,
        },
        "audioPlayCurrent" | "audioResume" => CoreCommand::ResumePlayback,
        "audioPlayQueueItem" => CoreCommand::NavigatePlayback {
            navigation: stereodrome_core::PlaybackNavigation::Index {
                index: parse_payload(payload)?,
            },
        },
        "audioPlayNext" => CoreCommand::NavigatePlayback {
            navigation: stereodrome_core::PlaybackNavigation::Next {
                force: parse_payload::<Option<bool>>(payload)?.unwrap_or(false),
            },
        },
        "audioPlayPrevious" => CoreCommand::NavigatePlayback {
            navigation: stereodrome_core::PlaybackNavigation::Previous,
        },
        "audioApplySettings" => CoreCommand::ApplyAudioSettings,
        "audioPrepareNextTransition" => CoreCommand::PrepareNextTransition,
        "audioPause" => CoreCommand::PausePlayback,
        "audioRebuildOutput" => CoreCommand::RebuildAudioOutput,
        "audioStop" => CoreCommand::StopPlayback,
        "audioSeek" => CoreCommand::SeekTo {
            seconds: parse_payload(payload)?,
        },
        "audioSetVolume" => CoreCommand::SetPlaybackVolume {
            volume: parse_payload(payload)?,
        },
        "reportPlatformPlayback" => CoreCommand::ReportPlatformPlayback {
            event: parse_payload(payload)?,
        },
        "playSongWithQueue" => {
            let args = parse_payload::<PlaySongWithQueuePayload>(payload)?;
            CoreCommand::PlaySelection {
                song_id: args.song_id,
                song_ids: args.song_ids,
            }
        }
        "addToQueue" => CoreCommand::AddToQueue {
            item: parse_payload(payload)?,
        },
        "addSongsToQueue" => CoreCommand::AddSongsToQueue {
            items: parse_payload(payload)?,
        },
        "insertNext" => CoreCommand::InsertNext {
            item: parse_payload(payload)?,
        },
        "insertNextSongs" => CoreCommand::InsertNextSongs {
            items: parse_payload(payload)?,
        },
        "removeFromQueue" => CoreCommand::RemoveFromQueue {
            index: parse_payload(payload)?,
        },
        "clearQueue" => CoreCommand::ClearPlayback,
        "moveQueueItem" => {
            let args = parse_payload::<MoveQueueItemPayload>(payload)?;
            CoreCommand::MoveQueueItem {
                from: args.from,
                to: args.to,
            }
        }
        "playQueueItem" => CoreCommand::PlayQueueItem {
            index: parse_payload(payload)?,
        },
        "playNext" => CoreCommand::PlayNext {
            force: parse_payload(payload)?,
        },
        "playPrevious" => CoreCommand::PlayPrevious,
        "toggleShuffle" => CoreCommand::ToggleShuffle,
        "setRepeatMode" => CoreCommand::SetRepeatMode {
            mode: parse_payload(payload)?,
        },
        "cycleRepeatMode" => CoreCommand::CycleRepeatMode,
        "rerollNext" => CoreCommand::RerollNext,
        _ => return Ok(None),
    };
    Ok(Some(command))
}

#[derive(Default, Deserialize)]
struct PrefetchPayload {
    #[serde(default)]
    reserve_first: bool,
}

#[derive(Deserialize)]
struct TwoOptionalStrings {
    first: Option<String>,
    second: Option<String>,
}

#[derive(Deserialize)]
struct AlbumListPayload {
    list_type: String,
    size: Option<usize>,
    offset: Option<usize>,
}

#[derive(Deserialize)]
struct SearchPayload {
    query: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct IdSizePayload {
    id: String,
    size: Option<i32>,
}

#[derive(Deserialize)]
struct PlaySongWithQueuePayload {
    song_id: String,
    song_ids: Vec<String>,
}

#[derive(Deserialize)]
struct MoveQueueItemPayload {
    from: usize,
    to: usize,
}

#[derive(Deserialize)]
struct CreatePlaylistPayload {
    name: String,
    song_ids: Vec<String>,
}

#[derive(Deserialize)]
struct RenamePlaylistPayload {
    playlist_id: String,
    name: String,
}

#[derive(Deserialize)]
struct PlaylistSongIdsPayload {
    playlist_id: String,
    song_ids: Vec<String>,
}

#[derive(Deserialize)]
struct PlaylistSongIndexesPayload {
    playlist_id: String,
    song_indexes: Vec<i64>,
}

#[derive(Deserialize)]
struct SetPlaylistSavedOfflinePayload {
    playlist_id: String,
    saved_offline: bool,
}

fn parse_payload<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, String> {
    serde_json::from_value(payload).map_err(|e| e.to_string())
}

fn parse_optional_string(payload: Value) -> Result<Option<String>, String> {
    if payload.is_null() {
        Ok(None)
    } else {
        parse_payload(payload).map(Some)
    }
}

fn json_result<T: serde::Serialize, E: ToString>(result: Result<T, E>) -> Result<String, String> {
    result.map(json_ok_string).map_err(|e| e.to_string())
}

fn catch_json_response(operation: impl FnOnce() -> *mut c_char) -> *mut c_char {
    match panic::catch_unwind(AssertUnwindSafe(operation)) {
        Ok(response) => response,
        Err(payload) => {
            let message = panic_payload_message(payload.as_ref());
            log::error!(
                target: "stereodrome_ffi",
                "Rust panic while handling mobile FFI call: {message}"
            );
            json_error(&format!("Rust panic: {message}"))
        }
    }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "non-string panic payload".to_string()
}

fn start_mobile_cache_monitor(
    core: Arc<StereodromeCore>,
    receiver: Receiver<CacheStateEvent>,
    announcer: PlaybackAnnouncer,
    running: Arc<AtomicBool>,
    mut reconcile_pending: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while running.load(Ordering::SeqCst) {
            let event = receiver.recv_timeout(MOBILE_CACHE_RECONCILE_RETRY_INTERVAL);
            let _changed = match event {
                Ok(event) => match announcer.apply_cache_state_event(&core, event) {
                    Ok(changed) => {
                        reconcile_pending = false;
                        changed
                    }
                    Err(error) => {
                        reconcile_pending = true;
                        log::warn!(target: "stereodrome_ffi", "Failed to apply cache event: {error}");
                        false
                    }
                },
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if reconcile_pending => {
                    match announcer.refresh_file_state(&core) {
                        Ok(changed) => {
                            reconcile_pending = false;
                            changed
                        }
                        Err(error) => {
                            log::warn!(target: "stereodrome_ffi", "Failed to reconcile cache state: {error}");
                            false
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => false,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };
        }
    })
}

fn start_runtime_event_bridge(
    mut events: tokio::sync::broadcast::Receiver<stereodrome_core::CoreEvent>,
    event_emitter: MobileEventEmitter,
    announcer: PlaybackAnnouncer,
    running: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while running.load(Ordering::SeqCst) {
            match events.try_recv() {
                Ok(event) => {
                    event_emitter.emit(|| Ok(MobileCoreEvent::Runtime(Box::new(event.clone()))));
                    if let CoreEventKind::SnapshotChanged { snapshot } = event.kind {
                        announcer.emit(event.revision, snapshot.playback.clone());
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }
    })
}

fn mobile_ref<'a>(core: *mut MobileCore) -> Option<&'a MobileCore> {
    if core.is_null() {
        None
    } else {
        unsafe { core.as_ref() }
    }
}

fn read_c_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }

    unsafe { CStr::from_ptr(value).to_str().ok().map(ToOwned::to_owned) }
}

fn json_ok(value: impl serde::Serialize) -> *mut c_char {
    into_c_string(json_ok_string(value))
}

fn json_ok_string(value: impl serde::Serialize) -> String {
    let envelope = serde_json::json!({
        "ok": true,
        "value": value,
    });
    envelope.to_string()
}

fn json_error(message: &str) -> *mut c_char {
    let envelope = serde_json::json!({
        "ok": false,
        "error": message,
    });
    into_c_string(envelope.to_string())
}

fn into_c_string(value: String) -> *mut c_char {
    CString::new(value).map_or(ptr::null_mut(), CString::into_raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
    use std::sync::mpsc::{self, RecvTimeoutError};

    static INSTANCE_CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);
    static INITIAL_PROJECTION_CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);

    extern "C" fn instance_event_callback(message: *const c_char, context: *mut c_void) {
        assert!(!message.is_null());
        assert_eq!(context as usize, 41);
        let event = unsafe { CStr::from_ptr(message) }
            .to_str()
            .expect("callback event is UTF-8");
        assert!(event.contains("runtime-shutting-down"));
        INSTANCE_CALLBACK_COUNT.fetch_add(1, AtomicOrdering::SeqCst);
    }

    extern "C" fn initial_projection_callback(message: *const c_char, context: *mut c_void) {
        assert!(!message.is_null());
        assert_eq!(context as usize, 42);
        let event: Value = serde_json::from_str(
            unsafe { CStr::from_ptr(message) }
                .to_str()
                .expect("callback event is UTF-8"),
        )
        .expect("callback event is JSON");
        assert_eq!(event["type"], "platform-projection");
        assert_eq!(event["payload"]["revision"], 0);
        INITIAL_PROJECTION_CALLBACK_COUNT.fetch_add(1, AtomicOrdering::SeqCst);
    }

    #[derive(Deserialize)]
    struct LegacyCommandFixture {
        name: String,
        method: String,
        payload: Value,
        expected_value: Value,
    }

    struct TestMobileCore {
        pointer: *mut MobileCore,
        data_dir: PathBuf,
    }

    impl TestMobileCore {
        fn new(name: &str) -> Self {
            static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

            let test_id = NEXT_TEST_ID.fetch_add(1, AtomicOrdering::Relaxed);
            let data_dir = std::env::temp_dir().join(format!(
                "stereodrome-ffi-{name}-{}-{test_id}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&data_dir);
            let data_dir_string = CString::new(data_dir.to_string_lossy().as_bytes())
                .expect("test data directory has no null bytes");
            let pointer = stereodrome_core_new(data_dir_string.as_ptr());
            assert!(!pointer.is_null(), "mobile core initializes");
            Self { pointer, data_dir }
        }

        fn call(&self, method: &str, payload: &Value) -> Value {
            let method = CString::new(method).expect("fixture method has no null bytes");
            let payload = CString::new(payload.to_string()).expect("payload has no null bytes");
            let response = stereodrome_core_call(self.pointer, method.as_ptr(), payload.as_ptr());
            assert!(!response.is_null(), "FFI returns a response");
            let response_json = unsafe { CStr::from_ptr(response) }
                .to_str()
                .expect("FFI response is UTF-8")
                .to_string();
            unsafe { stereodrome_core_free_string(response) };
            serde_json::from_str(&response_json).expect("FFI response is JSON")
        }

        fn dispatch_runtime(&self, request: &CoreCommandRequest) -> Value {
            let request =
                CString::new(serde_json::to_string(request).expect("runtime request serializes"))
                    .expect("runtime request has no null bytes");
            let response = stereodrome_runtime_dispatch(self.pointer, request.as_ptr());
            assert!(!response.is_null(), "runtime FFI returns a response");
            let response_json = unsafe { CStr::from_ptr(response) }
                .to_str()
                .expect("runtime response is UTF-8")
                .to_string();
            unsafe { stereodrome_runtime_string_free(response) };
            serde_json::from_str(&response_json).expect("runtime response is JSON")
        }

        fn mobile(&self) -> &MobileCore {
            unsafe { &*self.pointer }
        }
    }

    impl Drop for TestMobileCore {
        fn drop(&mut self) {
            unsafe { stereodrome_core_destroy(self.pointer) };
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }

    #[test]
    fn legacy_command_fixtures_round_trip_through_the_c_abi() {
        let fixtures: Vec<LegacyCommandFixture> = serde_json::from_str(include_str!(
            "../tests/fixtures/legacy-command-contract.json"
        ))
        .expect("legacy command fixtures parse");
        let core = TestMobileCore::new("legacy-command-contract");

        for fixture in fixtures {
            let response = core.call(&fixture.method, &fixture.payload);
            assert_eq!(
                response["ok"], true,
                "fixture failed: {} ({response})",
                fixture.name
            );
            assert_eq!(
                response["value"], fixture.expected_value,
                "fixture changed: {}",
                fixture.name
            );
        }

        let unknown = core.call("notARealMethod", &Value::Null);
        assert_eq!(unknown["ok"], false);
        assert_eq!(unknown["error"], "unknown method: notARealMethod");
    }

    #[test]
    fn typed_and_legacy_fixture_paths_produce_equivalent_values() {
        let fixtures: Vec<LegacyCommandFixture> = serde_json::from_str(include_str!(
            "../tests/fixtures/legacy-command-contract.json"
        ))
        .expect("legacy command fixtures parse");
        let legacy = TestMobileCore::new("legacy-equivalence");
        let typed = TestMobileCore::new("typed-equivalence");
        let mut last_revision = 0_u64;

        for (index, fixture) in fixtures.into_iter().enumerate() {
            let legacy_response = legacy.call(&fixture.method, &fixture.payload);
            let command = match fixture.method.as_str() {
                "setConnectivitySettings" => CoreCommand::SetConnectivity {
                    settings: serde_json::from_value(fixture.payload.clone())
                        .expect("connectivity fixture parses"),
                },
                "setSyncSettings" => CoreCommand::SetSyncSettings {
                    settings: serde_json::from_value(fixture.payload.clone())
                        .expect("sync fixture parses"),
                },
                "addToQueue" => CoreCommand::AddToQueue {
                    item: serde_json::from_value(fixture.payload.clone())
                        .expect("queue fixture parses"),
                },
                "setAudioProcessingSettings" => CoreCommand::SetAudioProcessing {
                    settings: serde_json::from_value(fixture.payload.clone())
                        .expect("audio settings fixture parses"),
                },
                "clearQueue" => CoreCommand::ClearPlayback,
                method => panic!("fixture {method} needs a typed compatibility mapping"),
            };
            let typed_response = typed.dispatch_runtime(&CoreCommandRequest {
                protocol_version: CORE_PROTOCOL_VERSION,
                command_id: CommandId(u64::try_from(index).expect("index fits u64") + 1),
                command,
            });

            assert_eq!(legacy_response["ok"], true, "{} legacy", fixture.name);
            assert_eq!(
                typed_response["status"], "succeeded",
                "{} typed",
                fixture.name
            );
            assert_eq!(
                typed_response["value"], legacy_response["value"],
                "{} differs between compatibility paths",
                fixture.name
            );
            assert_eq!(typed_response["protocol_version"], CORE_PROTOCOL_VERSION);
            let revision = typed_response["accepted_revision"]
                .as_u64()
                .expect("typed result has a revision");
            assert!(revision > last_revision);
            last_revision = revision;
        }

        let snapshot = typed.mobile().core_runtime.snapshot();
        assert_eq!(snapshot.status, CommandStatus::Succeeded);
        assert_eq!(snapshot.accepted_revision, last_revision);
        assert_eq!(snapshot.value.unwrap()["revision"], last_revision);
    }

    #[test]
    fn typed_ffi_rejects_protocol_mismatches_with_structured_errors() {
        let core = TestMobileCore::new("typed-protocol-error");
        let response = core.dispatch_runtime(&CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION + 1,
            command_id: CommandId(9),
            command: CoreCommand::GetSnapshot,
        });

        assert_eq!(response["status"], "failed");
        assert_eq!(response["command_id"], 9);
        assert_eq!(response["error"]["code"], "unsupported_protocol_version");
        assert_eq!(response["error"]["retryable"], false);
    }

    #[test]
    fn shared_playback_snapshot_fixture_matches_the_rust_contract() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../mobile/modules/stereodrome-core/fixtures/playback-snapshot.json"
        ))
        .expect("playback snapshot fixture parses");
        let queue: QueueState = serde_json::from_value(fixture["queue"].clone())
            .expect("fixture queue matches shared queue contract");

        assert_eq!(fixture["seq"], 42);
        assert_eq!(fixture["state"], "playing");
        assert_eq!(fixture["output_state"], "ready");
        assert_eq!(fixture["song"]["id"], "song-b");
        assert_eq!(fixture["queue_index"], 1);
        assert_eq!(fixture["queue_length"], 2);
        assert_eq!(queue.current_index, Some(1));
        assert_eq!(queue.items[1].song_id, "song-b");
        assert_eq!(queue.repeat_mode, RepeatMode::All);

        for capability in ["can_play", "can_next", "can_previous", "can_seek"] {
            assert!(
                fixture[capability].is_boolean(),
                "{capability} remains a boolean"
            );
        }
    }

    #[test]
    fn shared_platform_projection_event_matches_the_rust_contract() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../mobile/modules/stereodrome-core/fixtures/platform-projection-event.json"
        ))
        .expect("platform projection fixture parses");
        let projection: PlaybackProjection =
            serde_json::from_value(fixture["payload"]["projection"].clone())
                .expect("fixture projection matches the runtime contract");

        assert_eq!(fixture["type"], "platform-projection");
        assert_eq!(
            fixture["payload"]["protocol_version"],
            CORE_PROTOCOL_VERSION
        );
        assert_eq!(fixture["payload"]["revision"], 42);
        assert_eq!(projection.song.expect("fixture has a song").id, "song-b");
        assert_eq!(projection.queue_index, Some(1));
        assert!(projection.can_seek);
    }

    #[test]
    fn cache_events_keep_mobile_song_ids_sorted_and_deduplicated() {
        let mut song_ids = vec!["b".to_string(), "d".to_string()];

        assert!(update_sorted_song_ids(&mut song_ids, "c".to_string(), true));
        assert!(update_sorted_song_ids(&mut song_ids, "a".to_string(), true));
        assert!(!update_sorted_song_ids(
            &mut song_ids,
            "c".to_string(),
            true
        ));
        assert_eq!(song_ids, ["a", "b", "c", "d"]);

        assert!(update_sorted_song_ids(
            &mut song_ids,
            "b".to_string(),
            false
        ));
        assert!(!update_sorted_song_ids(
            &mut song_ids,
            "missing".to_string(),
            false
        ));
        assert_eq!(song_ids, ["a", "c", "d"]);
    }

    #[test]
    fn runtime_event_serialization_preserves_the_versioned_envelope() {
        let event = serde_json::to_value(MobileCoreEventEnvelope {
            stream_id: 3,
            seq: 12,
            event: MobileCoreEvent::Runtime(Box::new(CoreEvent {
                protocol_version: CORE_PROTOCOL_VERSION,
                stream_id: 8,
                event_id: 21,
                revision: 13,
                cause_command_id: CommandId(5),
                operation_id: None,
                kind: CoreEventKind::RuntimeShuttingDown,
            })),
        })
        .expect("runtime event serializes");

        assert_eq!(event["stream_id"], 3);
        assert_eq!(event["seq"], 12);
        assert_eq!(event["type"], "runtime");
        assert_eq!(event["payload"]["protocol_version"], CORE_PROTOCOL_VERSION);
        assert_eq!(event["payload"]["stream_id"], 8);
        assert_eq!(event["payload"]["event_id"], 21);
        assert_eq!(event["payload"]["revision"], 13);
        assert_eq!(event["payload"]["kind"]["type"], "runtime-shutting-down");
    }

    #[test]
    fn event_callbacks_are_instance_bound_and_can_be_cleared() {
        INSTANCE_CALLBACK_COUNT.store(0, AtomicOrdering::SeqCst);
        let emitter = MobileEventEmitter::new();
        emitter.set_callback(Some(instance_event_callback), 41_usize as *mut c_void);
        assert!(emitter.emit(|| {
            Ok(MobileCoreEvent::Runtime(Box::new(CoreEvent {
                protocol_version: CORE_PROTOCOL_VERSION,
                stream_id: 8,
                event_id: 21,
                revision: 13,
                cause_command_id: CommandId(5),
                operation_id: None,
                kind: CoreEventKind::RuntimeShuttingDown,
            })))
        }));
        assert_eq!(INSTANCE_CALLBACK_COUNT.load(AtomicOrdering::SeqCst), 1);

        emitter.set_callback(None, ptr::null_mut());
        assert!(emitter.emit(|| {
            Ok(MobileCoreEvent::Runtime(Box::new(CoreEvent {
                protocol_version: CORE_PROTOCOL_VERSION,
                stream_id: 8,
                event_id: 22,
                revision: 14,
                cause_command_id: CommandId(5),
                operation_id: None,
                kind: CoreEventKind::RuntimeShuttingDown,
            })))
        }));
        assert_eq!(INSTANCE_CALLBACK_COUNT.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn registering_an_event_callback_emits_the_initial_revision_zero_projection() {
        INITIAL_PROJECTION_CALLBACK_COUNT.store(0, AtomicOrdering::SeqCst);
        let core = TestMobileCore::new("initial-platform-projection");

        stereodrome_runtime_set_event_callback(
            core.pointer,
            Some(initial_projection_callback),
            42_usize as *mut c_void,
        );
        assert_eq!(
            INITIAL_PROJECTION_CALLBACK_COUNT.load(AtomicOrdering::SeqCst),
            1
        );
        stereodrome_runtime_set_event_callback(
            core.pointer,
            Some(initial_projection_callback),
            42_usize as *mut c_void,
        );
        assert_eq!(
            INITIAL_PROJECTION_CALLBACK_COUNT.load(AtomicOrdering::SeqCst),
            2
        );
        stereodrome_runtime_set_event_callback(core.pointer, None, ptr::null_mut());
    }

    #[test]
    fn playback_snapshot_sequencer_serializes_seq_allocation_and_snapshot_build() {
        let sequencer = Arc::new(Mutex::new(PlaybackSnapshotSequencer::new()));
        let state = Arc::new(AtomicU64::new(0));
        let first_entered = Arc::new(Barrier::new(2));
        let second_started = Arc::new(AtomicBool::new(false));
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let (second_captured_tx, second_captured_rx) = mpsc::channel();

        let first_handle = {
            let sequencer = Arc::clone(&sequencer);
            let state = Arc::clone(&state);
            let first_entered = Arc::clone(&first_entered);
            thread::spawn(move || {
                let mut sequencer = sequencer.lock().expect("sequencer lock should not poison");
                sequencer.sequence(|seq| {
                    first_entered.wait();
                    release_first_rx
                        .recv()
                        .expect("first snapshot should be released");
                    (seq, state.load(AtomicOrdering::SeqCst))
                })
            })
        };

        first_entered.wait();

        let second_handle = {
            let sequencer = Arc::clone(&sequencer);
            let state = Arc::clone(&state);
            let second_started = Arc::clone(&second_started);
            thread::spawn(move || {
                second_started.store(true, AtomicOrdering::SeqCst);
                let mut sequencer = sequencer.lock().expect("sequencer lock should not poison");
                let snapshot = sequencer.sequence(|seq| (seq, state.load(AtomicOrdering::SeqCst)));
                second_captured_tx
                    .send(snapshot)
                    .expect("second snapshot should be observable");
                snapshot
            })
        };

        while !second_started.load(AtomicOrdering::SeqCst) {
            thread::yield_now();
        }

        let early_snapshot = second_captured_rx.recv_timeout(Duration::from_millis(25));
        assert!(
            matches!(early_snapshot, Err(RecvTimeoutError::Timeout)),
            "second snapshot completed unexpectedly: {early_snapshot:?}"
        );

        state.store(1, AtomicOrdering::SeqCst);
        release_first_tx
            .send(())
            .expect("first snapshot thread should be waiting");

        let first_snapshot = first_handle
            .join()
            .expect("first snapshot thread should not panic");
        let second_snapshot = second_handle
            .join()
            .expect("second snapshot thread should not panic");
        let observed_second_snapshot = second_captured_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second snapshot should be delivered");

        assert_eq!(first_snapshot, (1, 1));
        assert_eq!(second_snapshot, (2, 1));
        assert_eq!(observed_second_snapshot, second_snapshot);
    }
}
