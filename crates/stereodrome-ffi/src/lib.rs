//! Mobile FFI boundary for Stereodrome.
//!
//! The first mobile implementation uses JSON-over-FFI so the Swift/Kotlin Expo
//! module can remain thin while the Rust API stabilizes. The crate is isolated
//! so a `UniFFI` surface can be generated here without touching the desktop
//! Tauri adapter.

use std::ffi::{CStr, CString, c_char};
use std::future::Future;
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

use log::{Level, LevelFilter, Metadata, Record};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stereodrome_audio::{
    AudioError, AudioNotification, AudioOutputState, AudioPlayer, AudioStateHandle, BinauralPreset,
    CrossfadePlayRequest, DynamicsPreset, EqualizerSettings, PlaybackIdentity,
    PlaybackLifecycleState, SongMetadata,
};
use stereodrome_core::queue::{QueueItem, QueueState, RepeatMode};
use stereodrome_core::{
    AudioProcessingSettings, CORE_PROTOCOL_VERSION, CacheStateEvent, CommandId, CommandStatus,
    ConnectParams, ConnectivitySettings, CoreCommand, CoreCommandRequest, CoreCommandResult,
    DueSyncJob, LibrarySyncStatus, PlaybackProgress, PrefetchCancellationToken, ProtocolError,
    ProtocolErrorCode, QueuePrefetchPlan, ServerSettingsUpdate, StereodromeCore,
    StereodromeRuntimeHandle, SyncSettings,
};
use url::Url;

static MOBILE_LOGGER: MobileLogger = MobileLogger;
static INIT_LOGGER: Once = Once::new();
static INIT_PANIC_HOOK: Once = Once::new();
static LOG_CALLBACK: Mutex<Option<MobileLogCallback>> = Mutex::new(None);
static PLAYBACK_CALLBACK: Mutex<Option<MobilePlaybackCallback>> = Mutex::new(None);
static EVENT_CALLBACK: Mutex<Option<MobileEventCallback>> = Mutex::new(None);
static NEXT_EVENT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

type MobileLogCallback = extern "C" fn(*const c_char);
type MobilePlaybackCallback = extern "C" fn(*const c_char);
type MobileEventCallback = extern "C" fn(*const c_char);

const MOBILE_CACHE_RECONCILE_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const MOBILE_PROGRESS_RETRY_INTERVAL: Duration = Duration::from_secs(1);

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

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_set_playback_callback(callback: Option<MobilePlaybackCallback>) {
    init_mobile_logging();
    if let Ok(mut current) = PLAYBACK_CALLBACK.lock() {
        *current = callback;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_set_event_callback(callback: Option<MobileEventCallback>) {
    init_mobile_logging();
    if let Ok(mut current) = EVENT_CALLBACK.lock() {
        *current = callback;
    }
}

pub struct MobileCore {
    core: Arc<StereodromeCore>,
    core_runtime: StereodromeRuntimeHandle,
    audio: Arc<AudioPlayer>,
    announcer: PlaybackAnnouncer,
    event_emitter: MobileEventEmitter,
    runtime: tokio::runtime::Runtime,
    data_dir: PathBuf,
    sync_state: Arc<Mutex<MobileSyncState>>,
    saved_playlist_offline_state: Arc<Mutex<SavedPlaylistOfflineState>>,
    prefetch_state: Arc<Mutex<BackgroundPrefetchState>>,
    cache_event_sender: Sender<CacheStateEvent>,
    monitor_running: Arc<AtomicBool>,
    monitor_event_sender: Sender<MobileMonitorEvent>,
    monitor_thread: Option<thread::JoinHandle<()>>,
}

enum MobileMonitorEvent {
    Audio(AudioNotification),
    Cache(CacheStateEvent),
    RecalculateDeadlines,
    Shutdown,
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
    FileState(MobileFileStateSnapshot),
    SyncStatus(Box<LibrarySyncStatus>),
    SavedPlaylistOfflineStatus(SavedPlaylistOfflineStatus),
}

#[derive(Serialize)]
struct MobileCoreEventEnvelope {
    stream_id: u64,
    seq: u64,
    #[serde(flatten)]
    event: MobileCoreEvent,
}

#[derive(Clone)]
struct MobileEventEmitter {
    stream_id: u64,
    next_seq: Arc<Mutex<u64>>,
}

impl MobileEventEmitter {
    fn new() -> Self {
        Self {
            stream_id: NEXT_EVENT_STREAM_ID.fetch_add(1, Ordering::Relaxed),
            next_seq: Arc::new(Mutex::new(1)),
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
        if let Some(callback) = EVENT_CALLBACK.lock().ok().and_then(|guard| *guard) {
            callback(message.as_ptr());
        }
        true
    }
}

struct PlaybackSnapshotSequencer {
    next_seq: u64,
}

impl PlaybackSnapshotSequencer {
    fn new() -> Self {
        Self { next_seq: 1 }
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

    fn snapshot(
        &self,
        core: &StereodromeCore,
        audio: &AudioPlayer,
    ) -> Result<PlaybackSnapshot, String> {
        self.sequence_snapshot(|seq| build_playback_snapshot(seq, core, audio))
    }

    fn emit(&self, core: &StereodromeCore, audio: &AudioPlayer) -> bool {
        let result = self.sequence_snapshot(|seq| {
            let snapshot = build_playback_snapshot(seq, core, audio)?;
            let json = serde_json::to_string(&snapshot)
                .map_err(|_| "Failed to serialize playback snapshot".to_string())?;

            if let Some(callback) = PLAYBACK_CALLBACK.lock().ok().and_then(|guard| *guard) {
                let message = CString::new(json).map_err(|_| {
                    "Failed to build playback snapshot callback payload".to_string()
                })?;
                callback(message.as_ptr());
            }

            Ok(())
        });

        if let Err(error) = result {
            match error.as_str() {
                "Failed to serialize playback snapshot" => {
                    log::warn!(
                        target: "stereodrome_ffi",
                        "Failed to serialize playback snapshot"
                    );
                }
                "Failed to build playback snapshot callback payload" => {
                    log::warn!(
                        target: "stereodrome_ffi",
                        "Failed to build playback snapshot callback payload"
                    );
                }
                _ => {
                    log::warn!(
                        target: "stereodrome_ffi",
                        "Failed to build playback snapshot: {error}"
                    );
                }
            }
            false
        } else {
            true
        }
    }

    fn emit_file_state(&self) -> bool {
        self.event_emitter
            .emit(|| Ok(MobileCoreEvent::FileState(self.file_state_snapshot())))
    }

    fn sequence_snapshot<T>(
        &self,
        build: impl FnOnce(u64) -> Result<T, String>,
    ) -> Result<T, String> {
        // Keep seq assignment, snapshot capture, and emitted callback delivery in one
        // critical section so higher seq values cannot describe older captured state.
        let mut sequencer = self
            .sequencer
            .lock()
            .map_err(|_| "playback snapshot sequencer lock poisoned".to_string())?;
        sequencer.sequence(build)
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
    output_state: AudioOutputState,
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

fn build_playback_snapshot(
    seq: u64,
    core: &StereodromeCore,
    audio: &AudioPlayer,
) -> Result<PlaybackSnapshot, String> {
    let queue = core.get_queue().map_err(|e| e.to_string())?;
    let audio_state = audio.get_playback_state();
    let audio_loaded = audio_state.song.is_some();
    let persisted = core.get_playback_state().map_err(|e| e.to_string())?;

    let (song, position_seconds, duration_seconds) = if let Some(song) = audio_state.song {
        let duration = if audio_state.duration > 0.0 {
            audio_state.duration
        } else {
            queue
                .items
                .iter()
                .find(|item| item.song_id == song.id)
                .map_or(0.0, |item| duration_seconds(item.duration))
        };
        (
            Some(snapshot_song_from_audio(core, song, duration)),
            audio_state.position,
            duration,
        )
    } else {
        let persisted_song_id = persisted.current_song_id.as_deref();
        let queue_item = persisted_song_id
            .and_then(|song_id| queue.items.iter().find(|item| item.song_id == song_id))
            .or_else(|| queue.current_index.and_then(|index| queue.items.get(index)));

        if let Some(item) = queue_item {
            let duration = if persisted.current_song_id.as_deref() == Some(item.song_id.as_str())
                && persisted.duration_seconds > 0.0
            {
                persisted.duration_seconds
            } else {
                duration_seconds(item.duration)
            };
            let position = if persisted.current_song_id.as_deref() == Some(item.song_id.as_str()) {
                persisted.position_seconds.max(0.0)
            } else {
                0.0
            };
            (
                Some(snapshot_song_from_queue_item(core, item, duration)),
                position,
                duration,
            )
        } else {
            (None, 0.0, 0.0)
        }
    };

    let state = match audio_state.state {
        PlaybackLifecycleState::Playing => "playing",
        PlaybackLifecycleState::Paused => "paused",
        PlaybackLifecycleState::Stopped => "stopped",
        PlaybackLifecycleState::Stalled => "stalled",
    };
    let queue_index = queue.current_index;
    let queue_length = queue.items.len();
    let can_play = song.is_some();
    let can_next = next_queue_item_exists(&queue);
    let can_previous = queue_length > 1 && queue_index.is_some();
    let can_seek = duration_seconds > 0.0;

    Ok(PlaybackSnapshot {
        seq,
        state,
        is_playing: audio_state.is_playing,
        audio_loaded,
        output_state: audio_state.output_state,
        song,
        position_seconds,
        duration_seconds,
        volume: audio_state.volume,
        queue,
        queue_index,
        queue_length,
        can_play,
        can_next,
        can_previous,
        can_seek,
    })
}

fn snapshot_song_from_audio(
    core: &StereodromeCore,
    song: SongMetadata,
    duration_seconds: f64,
) -> PlaybackSnapshotSong {
    let artwork_uri = cached_artwork_uri(core, &song.id);
    PlaybackSnapshotSong {
        id: song.id,
        title: song.title,
        artist: song.artist,
        album: song.album,
        duration_seconds,
        artwork_uri,
    }
}

fn snapshot_song_from_queue_item(
    core: &StereodromeCore,
    item: &QueueItem,
    duration_seconds: f64,
) -> PlaybackSnapshotSong {
    PlaybackSnapshotSong {
        id: item.song_id.clone(),
        title: item.title.clone(),
        artist: item.artist.clone(),
        album: item.album.clone(),
        duration_seconds,
        artwork_uri: cached_artwork_uri(core, &item.song_id),
    }
}

fn cached_artwork_uri(core: &StereodromeCore, song_id: &str) -> Option<String> {
    match core.cached_song_cover_art_uri(song_id, Some(512)) {
        Ok(uri) => uri,
        Err(error) => {
            log::debug!(
                target: "stereodrome_ffi",
                "Failed to read cached artwork for {song_id}: {error}"
            );
            None
        }
    }
}

fn next_queue_item_exists(queue: &QueueState) -> bool {
    if queue.items.is_empty() || queue.prepared_next_item.is_some() {
        return queue.prepared_next_item.is_some();
    }
    if queue.repeat_mode == RepeatMode::One && queue.current_index.is_some() {
        return true;
    }
    match queue.current_index {
        None => true,
        Some(index) if index + 1 < queue.items.len() => true,
        Some(_) => queue.repeat_mode == RepeatMode::All,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MobileSyncJob {
    Full,
    Incremental,
}

impl MobileSyncJob {
    fn active_job(self) -> &'static str {
        match self {
            Self::Full => "full_reconcile",
            Self::Incremental => "incremental",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Full => "full library sync",
            Self::Incremental => "incremental library sync",
        }
    }
}

impl From<DueSyncJob> for MobileSyncJob {
    fn from(job: DueSyncJob) -> Self {
        match job {
            DueSyncJob::FullReconcile => Self::Full,
            DueSyncJob::Incremental => Self::Incremental,
        }
    }
}

#[derive(Debug, Default)]
struct MobileSyncState {
    active_job: Option<MobileSyncJob>,
}

#[derive(Clone, Debug)]
enum SavedPlaylistOfflineTarget {
    All,
    Playlist(String),
}

impl SavedPlaylistOfflineTarget {
    fn display_name(&self) -> String {
        match self {
            Self::All => "saved playlist offline reconcile".to_string(),
            Self::Playlist(playlist_id) => format!("saved playlist offline download {playlist_id}"),
        }
    }
}

#[derive(Debug, Default)]
struct SavedPlaylistOfflineState {
    running: bool,
    last_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct SavedPlaylistOfflineStatus {
    running: bool,
    last_error: Option<String>,
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
        let (monitor_event_sender, monitor_event_receiver) = std::sync::mpsc::channel();
        match (
            StereodromeCore::new_with_cache_events(&data_dir, cache_event_sender.clone()),
            AudioPlayer::new_with_spectrum_and_notifications(false),
            tokio::runtime::Runtime::new(),
        ) {
            (Ok(core), Ok((audio, audio_notifications)), Ok(runtime)) => {
                let core = Arc::new(core);
                let Ok(core_runtime) =
                    StereodromeRuntimeHandle::start_with_core(&data_dir, Arc::clone(&core))
                else {
                    return ptr::null_mut();
                };
                let audio = Arc::new(audio);
                let event_emitter = MobileEventEmitter::new();
                let (announcer, file_state_initialized) =
                    PlaybackAnnouncer::new(&core, event_emitter.clone());
                let prefetch_state = Arc::new(Mutex::new(BackgroundPrefetchState::default()));
                let monitor_running = Arc::new(AtomicBool::new(true));
                start_mobile_monitor_adapter(
                    cache_event_receiver,
                    monitor_event_sender.clone(),
                    MobileMonitorEvent::Cache,
                );
                start_mobile_monitor_adapter(
                    audio_notifications,
                    monitor_event_sender.clone(),
                    MobileMonitorEvent::Audio,
                );
                let monitor_thread = start_mobile_playback_monitor(
                    Arc::clone(&core),
                    Arc::clone(&audio),
                    announcer.clone(),
                    Arc::clone(&prefetch_state),
                    Arc::clone(&monitor_running),
                    monitor_event_receiver,
                    !file_state_initialized,
                );

                Box::into_raw(Box::new(MobileCore {
                    core,
                    core_runtime,
                    audio,
                    announcer,
                    event_emitter,
                    runtime,
                    data_dir,
                    sync_state: Arc::new(Mutex::new(MobileSyncState::default())),
                    saved_playlist_offline_state: Arc::new(Mutex::new(
                        SavedPlaylistOfflineState::default(),
                    )),
                    prefetch_state,
                    cache_event_sender,
                    monitor_running,
                    monitor_event_sender,
                    monitor_thread: Some(monitor_thread),
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
            mobile.monitor_running.store(false, Ordering::SeqCst);
            let _ = mobile
                .monitor_event_sender
                .send(MobileMonitorEvent::Shutdown);
            shutdown_queue_prefetch(&mobile);
            if let Err(error) = mobile.audio.stop() {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Failed to stop mobile audio before monitor shutdown: {error}"
                );
            }
            if let Some(monitor_thread) = mobile.monitor_thread.take()
                && monitor_thread.join().is_err()
            {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Mobile playback monitor panicked during shutdown"
                );
            }
            if let Err(error) = mobile.audio.stop() {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Failed to finalize mobile audio shutdown during core destruction: {error}"
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
    into_c_string(serialize_protocol_result(
        &mobile.core_runtime.dispatch(request),
    ))
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

    if should_cancel_queue_prefetch(method) {
        cancel_queue_prefetch(
            core,
            &mobile.prefetch_state,
            matches!(method, "clearAudioCache" | "removeCachedSong"),
        )?;
    }

    if let Some(command) = legacy_runtime_command(method, payload.clone())? {
        let response = legacy_runtime_result(mobile.core_runtime.dispatch_command(command));
        return finish_dispatch(mobile, method, response);
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
        "exportPortableBackup" => {
            ensure_backup_jobs_idle(mobile)?;
            let path = parse_payload::<String>(payload)?;
            json_result(core.export_portable_backup(path))
        }
        "importPortableBackup" => {
            ensure_backup_jobs_idle(mobile)?;
            let path = parse_payload::<String>(payload)?;
            mobile.audio.stop().map_err(|error| error.to_string())?;
            let result = core.import_portable_backup(path);
            if result.is_ok()
                && let Ok(playback) = core.get_playback_state()
            {
                #[allow(clippy::cast_possible_truncation)]
                let volume = playback.app_volume as f32;
                let _ = mobile.audio.set_volume(volume);
            }
            json_result(result)
        }
        "disconnectServer" => {
            json_result(runtime.block_on(async { core.disconnect_server().await }))
        }
        "getConnectionStatus" => json_result(core.get_connection_status()),
        "syncLibrary" => json_result(start_sync_job(mobile, MobileSyncJob::Full)),
        "syncLibraryIncremental" => json_result(start_sync_job(mobile, MobileSyncJob::Incremental)),
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
        "runDueLibrarySync" => json_result(run_due_sync_job(mobile)),
        "getScanStatus" => json_result(runtime.block_on(async { core.get_scan_status().await })),
        "startScan" => json_result(runtime.block_on(async { core.start_scan().await })),
        "getLibrarySyncStatus" => json_result(get_mobile_library_sync_status(mobile)),
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
        "setPlaylistSavedOffline" => {
            let args = parse_payload::<SetPlaylistSavedOfflinePayload>(payload)?;
            let result =
                core.mark_playlist_saved_offline(args.playlist_id.clone(), args.saved_offline);
            if result.is_ok() && args.saved_offline {
                start_saved_playlist_offline_job(
                    mobile,
                    SavedPlaylistOfflineTarget::Playlist(args.playlist_id),
                )?;
            }
            json_result(result)
        }
        "reconcileSavedPlaylistsOffline" => {
            json_result(runtime.block_on(async { core.reconcile_saved_playlists_offline().await }))
        }
        "startSavedPlaylistsOfflineReconcile" => json_result(start_saved_playlist_offline_job(
            mobile,
            SavedPlaylistOfflineTarget::All,
        )),
        "getSavedPlaylistsOfflineReconcileStatus" => {
            json_result(get_saved_playlist_offline_status(mobile))
        }
        "prefetchNext" => {
            let args = if payload.is_null() {
                PrefetchPayload::default()
            } else {
                parse_payload::<PrefetchPayload>(payload)?
            };
            json_result(start_queue_prefetch(mobile, args.reserve_first))
        }
        "getPlaybackState" => json_result(core.get_playback_state()),
        "getPlaybackSnapshot" => json_result(mobile.announcer.snapshot(core, &mobile.audio)),
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
        "setAudioProcessingSettings" => {
            let settings = parse_payload::<AudioProcessingSettings>(payload)?;
            let result = core
                .set_audio_processing_settings(settings)
                .map_err(|e| e.to_string())
                .and_then(|next_settings| {
                    runtime
                        .block_on(async {
                            apply_audio_settings(mobile).await?;
                            match prepare_next_transition(mobile).await {
                                Ok(prepared) => Ok(prepared),
                                Err(error) => {
                                    log::warn!(
                                        target: "stereodrome_ffi",
                                        "Failed to prepare next transition after audio settings change: {error}"
                                    );
                                    Ok(false)
                                }
                            }
                        })
                        .and_then(|prepared| start_queue_prefetch(mobile, prepared))
                        .map(|()| next_settings)
                });
            json_result(result)
        }
        "audioPlayCurrent" => {
            let result = runtime.block_on(async { play_current_queue_item(mobile, None).await });
            json_result(result)
        }
        "audioPlayQueueItem" => {
            let index = parse_payload::<usize>(payload)?;
            let result = runtime.block_on(async {
                play_queue_navigation(mobile, QueueNavigation::Index(index)).await
            });
            json_result(result)
        }
        "audioPlayNext" => {
            let force = parse_payload::<Option<bool>>(payload)?.unwrap_or(false);
            let result = runtime.block_on(async {
                play_queue_navigation(mobile, QueueNavigation::Next(force)).await
            });
            json_result(result)
        }
        "audioPlayPrevious" => {
            let result = runtime
                .block_on(async { play_queue_navigation(mobile, QueueNavigation::Previous).await });
            json_result(result)
        }
        "audioApplySettings" => {
            let result = runtime.block_on(async { apply_audio_settings(mobile).await });
            json_result(result)
        }
        "audioPrepareNextTransition" => {
            let result = runtime
                .block_on(async { prepare_next_transition(mobile).await })
                .and_then(|prepared| start_queue_prefetch(mobile, prepared));
            json_result(result)
        }
        "audioPause" => {
            let result = mobile.audio.pause();
            json_result(result)
        }
        "audioResume" => {
            let result = runtime
                .block_on(async { resume_current_playback(mobile).await })
                .map(|_| ());
            json_result(result)
        }
        "audioRebuildOutput" => {
            let result = mobile.audio.rebuild_output().map(|()| {
                if let Err(error) =
                    runtime.block_on(async { prepare_next_transition(mobile).await })
                {
                    log::warn!(
                        target: "stereodrome_ffi",
                        "Failed to prepare next transition after rebuilding output: {error}"
                    );
                }
            });
            json_result(result)
        }
        "audioStop" => {
            let result = mobile.audio.stop();
            json_result(result)
        }
        "audioSeek" => {
            let position = parse_payload::<f64>(payload)?;
            let result = mobile.audio.seek(position);
            json_result(result)
        }
        "audioSetVolume" => {
            let volume = parse_payload::<f32>(payload)?;
            let result = mobile.audio.set_volume(volume);
            json_result(result)
        }
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

    finish_dispatch(mobile, method, response)
}

fn finish_dispatch(
    mobile: &MobileCore,
    method: &str,
    response: Result<String, String>,
) -> Result<String, String> {
    if response.is_ok() && should_emit_playback_snapshot(method) {
        mobile.announcer.emit(&mobile.core, &mobile.audio);
        let _ = mobile
            .monitor_event_sender
            .send(MobileMonitorEvent::RecalculateDeadlines);
    }

    response
}

fn legacy_runtime_result(result: CoreCommandResult) -> Result<String, String> {
    match result.status {
        CommandStatus::Succeeded => Ok(json_ok_string(result.value.unwrap_or(Value::Null))),
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
        "getScanStatus" => CoreCommand::GetScanStatus,
        "startScan" => CoreCommand::StartScan,
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
        "reconcileSavedPlaylistsOffline" => CoreCommand::ReconcileSavedPlaylistsOffline,
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
        "clearQueue" => CoreCommand::ClearQueue,
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

fn should_emit_playback_snapshot(method: &str) -> bool {
    matches!(
        method,
        "importPortableBackup"
            | "setAudioProcessingSettings"
            | "audioPlayCurrent"
            | "audioPlayQueueItem"
            | "audioPlayNext"
            | "audioPlayPrevious"
            | "audioApplySettings"
            | "audioPause"
            | "audioResume"
            | "audioRebuildOutput"
            | "audioStop"
            | "audioSeek"
            | "audioSetVolume"
            | "playSongWithQueue"
            | "addToQueue"
            | "addSongsToQueue"
            | "insertNext"
            | "insertNextSongs"
            | "removeFromQueue"
            | "clearQueue"
            | "moveQueueItem"
            | "playQueueItem"
            | "playNext"
            | "playPrevious"
            | "toggleShuffle"
            | "setRepeatMode"
            | "cycleRepeatMode"
            | "rerollNext"
    )
}

fn should_cancel_queue_prefetch(method: &str) -> bool {
    matches!(
        method,
        "importPortableBackup"
            | "disconnectServer"
            | "setConnectivitySettings"
            | "clearAudioCache"
            | "removeCachedSong"
            | "setAudioProcessingSettings"
            | "audioPlayCurrent"
            | "audioPlayQueueItem"
            | "audioPlayNext"
            | "audioPlayPrevious"
            | "audioApplySettings"
            | "audioPrepareNextTransition"
            | "audioResume"
            | "audioRebuildOutput"
            | "audioStop"
            | "playSongWithQueue"
            | "addToQueue"
            | "addSongsToQueue"
            | "insertNext"
            | "insertNextSongs"
            | "removeFromQueue"
            | "clearQueue"
            | "moveQueueItem"
            | "playQueueItem"
            | "playNext"
            | "playPrevious"
            | "toggleShuffle"
            | "setRepeatMode"
            | "cycleRepeatMode"
            | "rerollNext"
    )
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

fn start_sync_job(mobile: &MobileCore, job: MobileSyncJob) -> Result<(), String> {
    {
        let mut state = mobile
            .sync_state
            .lock()
            .map_err(|_| "sync state lock is poisoned".to_string())?;
        if let Some(active_job) = state.active_job {
            return Err(format!("{} is already running", active_job.display_name()));
        }
        state.active_job = Some(job);
    }
    emit_mobile_sync_status(&mobile.event_emitter, &mobile.core, &mobile.sync_state);

    let data_dir = mobile.data_dir.clone();
    let core = Arc::clone(&mobile.core);
    let event_emitter = mobile.event_emitter.clone();
    let sync_state = Arc::clone(&mobile.sync_state);
    let cache_event_sender = mobile.cache_event_sender.clone();
    thread::spawn(move || {
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            run_sync_job(data_dir, job, cache_event_sender)
        }));
        match result {
            Ok(Ok(())) => {
                log::info!(
                    target: "stereodrome_ffi",
                    "Mobile {} finished",
                    job.display_name()
                );
            }
            Ok(Err(error)) => {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Mobile {} failed: {error}",
                    job.display_name()
                );
            }
            Err(payload) => {
                log::error!(
                    target: "stereodrome_ffi",
                    "Mobile {} panicked: {}",
                    job.display_name(),
                    panic_payload_message(payload.as_ref())
                );
            }
        }

        if let Ok(mut state) = sync_state.lock()
            && state.active_job == Some(job)
        {
            state.active_job = None;
        }
        emit_mobile_sync_status(&event_emitter, &core, &sync_state);
    });

    Ok(())
}

fn ensure_backup_jobs_idle(mobile: &MobileCore) -> Result<(), String> {
    let sync_running = mobile
        .sync_state
        .lock()
        .map_err(|_| "sync state lock is poisoned".to_string())?
        .active_job
        .is_some();
    let playlist_job_running = mobile
        .saved_playlist_offline_state
        .lock()
        .map_err(|_| "saved playlist state lock is poisoned".to_string())?
        .running;
    let prefetch_running = {
        let prefetch = mobile
            .prefetch_state
            .lock()
            .map_err(|_| "background prefetch state lock is poisoned".to_string())?;
        prefetch.running || prefetch.requested_plan.is_some()
    };
    let downloads_running = !mobile.core.get_downloading_song_ids().is_empty();
    if sync_running || playlist_job_running || prefetch_running || downloads_running {
        return Err(
            "wait for background library or download jobs to finish before using backups"
                .to_string(),
        );
    }
    Ok(())
}

fn run_sync_job(
    data_dir: PathBuf,
    job: MobileSyncJob,
    cache_event_sender: Sender<CacheStateEvent>,
) -> Result<(), String> {
    log::info!(
        target: "stereodrome_ffi",
        "Starting mobile {} in background",
        job.display_name()
    );
    let core = StereodromeCore::new_with_cache_events(data_dir, cache_event_sender)
        .map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
    runtime
        .block_on(async { core.restore_session().await })
        .map_err(|error| error.to_string())?;

    match job {
        MobileSyncJob::Full => runtime
            .block_on(async { core.reconcile_library().await })
            .map(|_| ())
            .map_err(|error| error.to_string()),
        MobileSyncJob::Incremental => runtime
            .block_on(async { core.sync_library_incremental().await })
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}

#[derive(Default)]
struct BackgroundPrefetchState {
    closed: bool,
    running: bool,
    cancellation_generation: u64,
    worker_generation: u64,
    worker_handle: Option<tokio::task::JoinHandle<()>>,
    active_plan: Option<QueuePrefetchPlan>,
    requested_plan: Option<QueuePrefetchPlan>,
    cancellation: Option<PrefetchCancellationToken>,
    last_completed_plan: Option<QueuePrefetchPlan>,
}

struct BackgroundPrefetchGuard {
    state: Arc<Mutex<BackgroundPrefetchState>>,
    worker_generation: u64,
}

impl Drop for BackgroundPrefetchGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock()
            && state.worker_generation == self.worker_generation
        {
            state.running = false;
            state.active_plan = None;
            state.requested_plan = None;
            state.cancellation = None;
        }
    }
}

fn cancel_queue_prefetch(
    core: &StereodromeCore,
    state: &Arc<Mutex<BackgroundPrefetchState>>,
    invalidate_completed: bool,
) -> Result<(), String> {
    signal_queue_prefetch_cancellation(state, invalidate_completed)?;
    core.cache_mutation_barrier()
        .map_err(|error| error.to_string())
}

fn signal_queue_prefetch_cancellation(
    state: &Arc<Mutex<BackgroundPrefetchState>>,
    invalidate_completed: bool,
) -> Result<(), String> {
    let mut state = state
        .lock()
        .map_err(|_| "background prefetch state lock is poisoned".to_string())?;
    if let Some(cancellation) = state.cancellation.take() {
        cancellation.cancel();
    }
    state.cancellation_generation = state.cancellation_generation.wrapping_add(1);
    state.active_plan = None;
    state.requested_plan = None;
    if invalidate_completed {
        state.last_completed_plan = None;
    }
    Ok(())
}

fn shutdown_queue_prefetch(mobile: &MobileCore) {
    let worker_handle = mobile.prefetch_state.lock().ok().and_then(|mut state| {
        state.closed = true;
        state.cancellation_generation = state.cancellation_generation.wrapping_add(1);
        if let Some(cancellation) = state.cancellation.take() {
            cancellation.cancel();
        }
        state.active_plan = None;
        state.requested_plan = None;
        state.worker_handle.take()
    });
    let _ = mobile.core.cache_mutation_barrier();
    let Some(mut worker_handle) = worker_handle else {
        return;
    };

    mobile.runtime.block_on(async {
        tokio::select! {
            _ = &mut worker_handle => {}
            () = tokio::time::sleep(Duration::from_secs(2)) => {
                worker_handle.abort();
                let _ = worker_handle.await;
            }
        }
    });
}

fn start_queue_prefetch(mobile: &MobileCore, reserve_first: bool) -> Result<(), String> {
    spawn_queue_prefetch(
        &mobile.runtime,
        Arc::clone(&mobile.core),
        &mobile.prefetch_state,
        reserve_first,
    )
}

fn spawn_queue_prefetch(
    runtime: &tokio::runtime::Runtime,
    core: Arc<StereodromeCore>,
    state: &Arc<Mutex<BackgroundPrefetchState>>,
    _reserve_first: bool,
) -> Result<(), String> {
    let request_generation = {
        let state = state
            .lock()
            .map_err(|_| "background prefetch state lock is poisoned".to_string())?;
        if state.closed {
            return Ok(());
        }
        state.cancellation_generation
    };
    let settings = core
        .get_audio_processing_settings()
        .map_err(|error| error.to_string())?;
    let prefetch_count = settings.prefetch_count as usize;
    let requested_plan = core
        .queue_prefetch_plan(prefetch_count)
        .map_err(|error| error.to_string())?;
    let requested_plan_is_satisfied = core
        .queue_prefetch_plan_is_satisfied(&requested_plan)
        .map_err(|error| error.to_string())?;

    let mut state_guard = state
        .lock()
        .map_err(|_| "background prefetch state lock is poisoned".to_string())?;
    if state_guard.closed || state_guard.cancellation_generation != request_generation {
        return Ok(());
    }
    if state_guard.active_plan.as_ref() == Some(&requested_plan)
        || state_guard.requested_plan.as_ref() == Some(&requested_plan)
        || (!state_guard.running
            && requested_plan_is_satisfied
            && state_guard.last_completed_plan.as_ref() == Some(&requested_plan))
    {
        return Ok(());
    }
    if state_guard.last_completed_plan.as_ref() == Some(&requested_plan)
        && !requested_plan_is_satisfied
    {
        state_guard.last_completed_plan = None;
    }
    if let Some(cancellation) = state_guard.cancellation.take() {
        cancellation.cancel();
    }
    state_guard.requested_plan = Some(requested_plan);
    if state_guard.running {
        return Ok(());
    }
    state_guard.running = true;
    state_guard.worker_generation = state_guard.worker_generation.wrapping_add(1);
    let worker_generation = state_guard.worker_generation;

    let worker_state = Arc::clone(state);
    let worker_handle = runtime.spawn(async move {
        let state = worker_state;
        let _guard = BackgroundPrefetchGuard {
            state: Arc::clone(&state),
            worker_generation,
        };
        loop {
            let Some((plan, cancellation)) = ({
                let Ok(mut state) = state.lock() else {
                    return;
                };
                if let Some(plan) = state.requested_plan.take() {
                    let cancellation = PrefetchCancellationToken::new();
                    state.active_plan = Some(plan.clone());
                    state.cancellation = Some(cancellation.clone());
                    Some((plan, cancellation))
                } else {
                    if state.worker_generation == worker_generation {
                        state.running = false;
                    }
                    None
                }
            }) else {
                break;
            };

            let outcome = core.run_queue_prefetch_plan(&plan, &cancellation).await;
            let Ok(mut state) = state.lock() else {
                return;
            };
            if state.active_plan.as_ref() == Some(&plan) {
                if outcome
                    .as_ref()
                    .is_ok_and(|outcome| outcome.completed && !cancellation.is_cancelled())
                {
                    state.last_completed_plan = Some(plan.clone());
                }
                state.active_plan = None;
                state.cancellation = None;
            }
            drop(state);

            if let Err(error) = outcome {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Failed to prefetch upcoming queue tracks: {error}"
                );
            }
        }
    });
    state_guard.worker_handle = Some(worker_handle);
    drop(state_guard);
    Ok(())
}

fn start_saved_playlist_offline_job(
    mobile: &MobileCore,
    target: SavedPlaylistOfflineTarget,
) -> Result<(), String> {
    {
        let mut state = mobile
            .saved_playlist_offline_state
            .lock()
            .map_err(|_| "saved playlist offline state lock is poisoned".to_string())?;
        if state.running {
            return Ok(());
        }
        state.running = true;
        state.last_error = None;
    }
    emit_saved_playlist_offline_status(&mobile.event_emitter, &mobile.saved_playlist_offline_state);

    let data_dir = mobile.data_dir.clone();
    let saved_playlist_offline_state = Arc::clone(&mobile.saved_playlist_offline_state);
    let event_emitter = mobile.event_emitter.clone();
    let cache_event_sender = mobile.cache_event_sender.clone();
    thread::spawn(move || {
        let job_name = target.display_name();
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            run_saved_playlist_offline_job(data_dir, target, cache_event_sender)
        }));
        let last_error = match result {
            Ok(Ok(())) => {
                log::info!(
                    target: "stereodrome_ffi",
                    "Mobile {job_name} finished"
                );
                None
            }
            Ok(Err(error)) => {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Mobile {job_name} failed: {error}"
                );
                Some(error)
            }
            Err(payload) => {
                let error = format!(
                    "Mobile {job_name} panicked: {}",
                    panic_payload_message(payload.as_ref())
                );
                log::error!(target: "stereodrome_ffi", "{error}");
                Some(error)
            }
        };

        if let Ok(mut state) = saved_playlist_offline_state.lock() {
            state.running = false;
            state.last_error = last_error;
        }
        emit_saved_playlist_offline_status(&event_emitter, &saved_playlist_offline_state);
    });

    Ok(())
}

fn run_saved_playlist_offline_job(
    data_dir: PathBuf,
    target: SavedPlaylistOfflineTarget,
    cache_event_sender: Sender<CacheStateEvent>,
) -> Result<(), String> {
    log::info!(
        target: "stereodrome_ffi",
        "Starting mobile {} in background",
        target.display_name()
    );
    let core = StereodromeCore::new_with_cache_events(data_dir, cache_event_sender)
        .map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
    runtime
        .block_on(async { core.restore_session().await })
        .map_err(|error| error.to_string())?;

    match target {
        SavedPlaylistOfflineTarget::All => runtime
            .block_on(async { core.reconcile_saved_playlists_offline().await })
            .map(|_| ())
            .map_err(|error| error.to_string()),
        SavedPlaylistOfflineTarget::Playlist(playlist_id) => runtime
            .block_on(async { core.download_playlist(playlist_id).await })
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}

fn get_saved_playlist_offline_status(
    mobile: &MobileCore,
) -> Result<SavedPlaylistOfflineStatus, String> {
    let state = mobile
        .saved_playlist_offline_state
        .lock()
        .map_err(|_| "saved playlist offline state lock is poisoned".to_string())?;
    Ok(SavedPlaylistOfflineStatus {
        running: state.running,
        last_error: state.last_error.clone(),
    })
}

fn run_due_sync_job(mobile: &MobileCore) -> Result<Option<String>, String> {
    mobile
        .runtime
        .block_on(async { mobile.core.restore_session().await })
        .map_err(|error| error.to_string())?;

    let Some(due_job) = mobile
        .core
        .next_due_library_sync_job()
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let mobile_job = MobileSyncJob::from(due_job);

    {
        let mut state = mobile
            .sync_state
            .lock()
            .map_err(|_| "sync state lock is poisoned".to_string())?;
        if let Some(active_job) = state.active_job {
            return Err(format!("{} is already running", active_job.display_name()));
        }
        state.active_job = Some(mobile_job);
    }
    emit_mobile_sync_status(&mobile.event_emitter, &mobile.core, &mobile.sync_state);

    let result = mobile
        .runtime
        .block_on(async { mobile.core.run_due_library_sync().await })
        .map_err(|error| error.to_string());

    if let Ok(mut state) = mobile.sync_state.lock()
        && state.active_job == Some(mobile_job)
    {
        state.active_job = None;
    }
    emit_mobile_sync_status(&mobile.event_emitter, &mobile.core, &mobile.sync_state);

    result
}

fn get_mobile_library_sync_status(mobile: &MobileCore) -> Result<LibrarySyncStatus, String> {
    mobile_library_sync_status(&mobile.core, &mobile.sync_state)
}

fn mobile_library_sync_status(
    core: &StereodromeCore,
    sync_state: &Mutex<MobileSyncState>,
) -> Result<LibrarySyncStatus, String> {
    let mut status = core
        .get_library_sync_status()
        .map_err(|error| error.to_string())?;
    let active_job = sync_state
        .lock()
        .map_err(|_| "sync state lock is poisoned".to_string())?
        .active_job;

    if let Some(job) = active_job {
        status.active_job = Some(job.active_job().to_string());
        match job {
            MobileSyncJob::Full => status.full_reconcile.running = true,
            MobileSyncJob::Incremental => status.incremental.running = true,
        }
    }

    Ok(status)
}

fn emit_mobile_sync_status(
    event_emitter: &MobileEventEmitter,
    core: &StereodromeCore,
    sync_state: &Mutex<MobileSyncState>,
) {
    event_emitter.emit(|| {
        mobile_library_sync_status(core, sync_state)
            .map(Box::new)
            .map(MobileCoreEvent::SyncStatus)
    });
}

fn emit_saved_playlist_offline_status(
    event_emitter: &MobileEventEmitter,
    state: &Mutex<SavedPlaylistOfflineState>,
) {
    event_emitter.emit(|| {
        state
            .lock()
            .map_err(|_| "saved playlist offline state lock is poisoned".to_string())
            .map(|state| {
                MobileCoreEvent::SavedPlaylistOfflineStatus(SavedPlaylistOfflineStatus {
                    running: state.running,
                    last_error: state.last_error.clone(),
                })
            })
    });
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

#[allow(clippy::too_many_lines)]
fn start_mobile_monitor_adapter<T: Send + 'static>(
    receiver: Receiver<T>,
    sender: Sender<MobileMonitorEvent>,
    wrap: fn(T) -> MobileMonitorEvent,
) {
    thread::spawn(move || {
        while let Ok(event) = receiver.recv() {
            if sender.send(wrap(event)).is_err() {
                break;
            }
        }
    });
}

#[allow(clippy::too_many_lines)]
fn start_mobile_playback_monitor(
    core: Arc<StereodromeCore>,
    audio: Arc<AudioPlayer>,
    announcer: PlaybackAnnouncer,
    prefetch_state: Arc<Mutex<BackgroundPrefetchState>>,
    running: Arc<AtomicBool>,
    events: Receiver<MobileMonitorEvent>,
    mut cache_reconcile_pending: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Failed to start mobile playback monitor runtime: {error}"
                );
                return;
            }
        };

        let state_handle = audio.state_handle();
        let mut last_segment_idx = 0usize;
        let mut last_report: Option<MobileProgressReport> = None;
        let mut last_snapshot_marker: Option<MobilePlaybackMarker> = None;
        let mut cache_snapshot_pending = false;
        let mut last_cache_reconcile_attempt: Option<Instant> = None;
        let mut crossfade_attempted: Option<PlaybackIdentity> = None;

        while running.load(Ordering::SeqCst) {
            let current_identity = audio.current_playback_identity();
            if crossfade_attempted.is_some() && crossfade_attempted != current_identity {
                crossfade_attempted = None;
            }
            let wait = mobile_monitor_wait_duration(
                &core,
                &audio,
                &state_handle,
                last_report.as_ref(),
                cache_reconcile_pending,
                last_cache_reconcile_attempt,
                crossfade_attempted.as_ref(),
            );
            let event = match wait {
                Some(duration) if duration.is_zero() => None,
                Some(duration) => match events.recv_timeout(duration) {
                    Ok(event) => Some(event),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                },
                None => match events.recv() {
                    Ok(event) => Some(event),
                    Err(_) => break,
                },
            };

            let mut terminal_identity = None;
            match event {
                Some(MobileMonitorEvent::Shutdown) => break,
                Some(MobileMonitorEvent::Cache(cache_event)) => {
                    let reconciles = matches!(&cache_event, CacheStateEvent::Reconcile);
                    match announcer.apply_cache_state_event(&core, cache_event) {
                        Ok(changed) => {
                            cache_snapshot_pending |= changed;
                            if reconciles {
                                cache_reconcile_pending = false;
                            }
                        }
                        Err(error) => {
                            log::warn!(
                                target: "stereodrome_ffi",
                                "Failed to apply mobile cache event: {error}"
                            );
                            cache_reconcile_pending = true;
                        }
                    }
                }
                Some(MobileMonitorEvent::Audio(notification)) => {
                    if !audio_notification_is_current(&audio, &notification) {
                        continue;
                    }
                    if let AudioNotification::EndOfTrack { identity } = notification {
                        terminal_identity = Some(identity);
                    }
                }
                Some(MobileMonitorEvent::RecalculateDeadlines) | None => {}
            }

            let cache_retry_due = cache_reconcile_pending
                && last_cache_reconcile_attempt.is_none_or(|last_attempt| {
                    last_attempt.elapsed() >= MOBILE_CACHE_RECONCILE_RETRY_INTERVAL
                });
            if cache_retry_due {
                last_cache_reconcile_attempt = Some(Instant::now());
                match announcer.refresh_file_state(&core) {
                    Ok(changed) => {
                        cache_snapshot_pending |= changed;
                        cache_reconcile_pending = false;
                    }
                    Err(error) => {
                        log::warn!(
                            target: "stereodrome_ffi",
                            "Failed to reconcile mobile cache state: {error}"
                        );
                    }
                }
            }
            if !running.load(Ordering::SeqCst) {
                break;
            }
            if cache_snapshot_pending && announcer.emit_file_state() {
                cache_snapshot_pending = false;
            }

            let (state, segment_idx) = state_handle.get_gapless_state();
            let marker = MobilePlaybackMarker::from_state(&state, segment_idx);
            if last_snapshot_marker.as_ref() != Some(&marker) {
                if last_snapshot_marker
                    .as_ref()
                    .is_some_and(|previous| previous.song_id.is_some() && marker.song_id.is_none())
                {
                    let _ = cancel_queue_prefetch(&core, &prefetch_state, false);
                }
                announcer.emit(&core, &audio);
                last_snapshot_marker = Some(marker);
            }
            let Some(song) = state.song.clone() else {
                last_segment_idx = 0;
                last_report = None;
                continue;
            };

            if segment_idx < last_segment_idx {
                last_segment_idx = 0;
            }

            if segment_idx > last_segment_idx {
                last_segment_idx = segment_idx;
                let _ = cancel_queue_prefetch(&core, &prefetch_state, false);
                match core.play_next(Some(false)) {
                    Ok(Some(next)) => {
                        let progress = PlaybackProgress {
                            song_id: next.song_id.clone(),
                            position_seconds: 0.0,
                            duration_seconds: duration_seconds(next.duration),
                            is_playing: true,
                        };
                        if block_on_monitor_future(
                            &runtime,
                            &running,
                            core.report_playback_progress(progress),
                        )
                        .is_none()
                        {
                            break;
                        }
                        let Some(prepared) = block_on_monitor_future(
                            &runtime,
                            &running,
                            prepare_next_transition_from(&core, &audio),
                        ) else {
                            break;
                        };
                        let prepared = prepared.unwrap_or(false);
                        let _ = spawn_queue_prefetch(
                            &runtime,
                            Arc::clone(&core),
                            &prefetch_state,
                            prepared,
                        );
                        announcer.emit(&core, &audio);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        log::warn!(
                            target: "stereodrome_ffi",
                            "Failed to advance queue after gapless transition: {error}"
                        );
                    }
                }
            }

            if !report_mobile_progress(
                &runtime,
                &core,
                &running,
                &mut last_report,
                &song.id,
                &state,
            ) {
                break;
            }

            if state.is_playing
                && state_handle.is_last_gapless_segment(segment_idx)
                && !state_handle.is_crossfade_initiated()
            {
                match core.get_audio_processing_settings() {
                    Ok(settings) if settings.crossfade_enabled => {
                        let crossfade_window_seconds =
                            f64::from(settings.crossfade_duration_ms) / 1000.0;
                        let remaining = state.duration - state.position;
                        if remaining <= crossfade_window_seconds && remaining > 0.5 {
                            crossfade_attempted = audio.current_playback_identity();
                            state_handle.set_crossfade_initiated(true);
                            let _ = cancel_queue_prefetch(&core, &prefetch_state, false);
                            let Some(crossfade_result) = block_on_monitor_future(
                                &runtime,
                                &running,
                                crossfade_next_from(&core, &audio, crossfade_attempted.as_ref()),
                            ) else {
                                break;
                            };
                            match crossfade_result {
                                Ok(Some(_)) => {
                                    let Some(prepared) = block_on_monitor_future(
                                        &runtime,
                                        &running,
                                        prepare_next_transition_from(&core, &audio),
                                    ) else {
                                        break;
                                    };
                                    let prepared = prepared.unwrap_or(false);
                                    let _ = spawn_queue_prefetch(
                                        &runtime,
                                        Arc::clone(&core),
                                        &prefetch_state,
                                        prepared,
                                    );
                                    announcer.emit(&core, &audio);
                                }
                                Ok(None) => state_handle.set_crossfade_initiated(false),
                                Err(error) => {
                                    state_handle.set_crossfade_initiated(false);
                                    log::warn!(
                                        target: "stereodrome_ffi",
                                        "Failed to start mobile crossfade: {error}"
                                    );
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        log::warn!(
                            target: "stereodrome_ffi",
                            "Failed to read mobile crossfade settings: {error}"
                        );
                    }
                }
            }

            let playback_finished = terminal_identity.is_some()
                && state.duration > 0.0
                && state.position >= state.duration - 0.2
                && !state.is_playing
                && !state_handle.is_crossfade_initiated();

            if playback_finished {
                last_segment_idx = 0;
                last_report = None;
                let _ = cancel_queue_prefetch(&core, &prefetch_state, false);

                let Some(navigation_result) = block_on_monitor_future(
                    &runtime,
                    &running,
                    play_queue_navigation_from(
                        &core,
                        &audio,
                        QueueNavigation::Next(false),
                        terminal_identity.clone(),
                    ),
                ) else {
                    break;
                };
                match navigation_result {
                    Ok(status) if status.current_song_id.is_some() => {
                        let Some(prepared) = block_on_monitor_future(
                            &runtime,
                            &running,
                            prepare_next_transition_from(&core, &audio),
                        ) else {
                            break;
                        };
                        let prepared = prepared.unwrap_or(false);
                        let _ = spawn_queue_prefetch(
                            &runtime,
                            Arc::clone(&core),
                            &prefetch_state,
                            prepared,
                        );
                        announcer.emit(&core, &audio);
                    }
                    Ok(_) => {
                        let _ = core.save_playback_position(PlaybackProgress {
                            song_id: song.id,
                            position_seconds: 0.0,
                            duration_seconds: state.duration,
                            is_playing: false,
                        });
                        announcer.emit(&core, &audio);
                    }
                    Err(error) => {
                        log::warn!(
                            target: "stereodrome_ffi",
                            "Failed to prepare mobile playback after track ended: {error}"
                        );
                        if terminal_identity.as_ref() == audio.current_playback_identity().as_ref()
                            && let Err(stop_error) = audio.stop()
                        {
                            log::warn!(
                                target: "stereodrome_ffi",
                                "Failed to release mobile audio after terminal transition error: \
                                 {stop_error}"
                            );
                        }
                        let _ = core.save_playback_position(PlaybackProgress {
                            song_id: song.id,
                            position_seconds: 0.0,
                            duration_seconds: state.duration,
                            is_playing: false,
                        });
                        announcer.emit(&core, &audio);
                    }
                }
            }
        }
    })
}

struct MobileProgressReport {
    at: Instant,
    is_playing: bool,
    position: f64,
    song_id: String,
    pending: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct MobilePlaybackMarker {
    state: stereodrome_audio::PlaybackLifecycleState,
    output_state: AudioOutputState,
    is_playing: bool,
    song_id: Option<String>,
    segment_idx: usize,
}

impl MobilePlaybackMarker {
    fn from_state(state: &stereodrome_audio::PlaybackState, segment_idx: usize) -> Self {
        Self {
            state: state.state,
            output_state: state.output_state,
            is_playing: state.is_playing,
            song_id: state.song.as_ref().map(|song| song.id.clone()),
            segment_idx,
        }
    }
}

fn audio_notification_is_current(audio: &AudioPlayer, notification: &AudioNotification) -> bool {
    let current = audio.current_playback_identity();
    match notification {
        AudioNotification::PlaybackChanged { identity, .. } => identity == &current,
        AudioNotification::GaplessSegmentChanged { identity, .. }
        | AudioNotification::EndOfTrack { identity }
        | AudioNotification::PositionChanged { identity } => current.as_ref() == Some(identity),
        AudioNotification::OutputStateChanged { .. } => true,
    }
}

fn earlier_deadline(current: Option<Instant>, candidate: Instant) -> Instant {
    current.map_or(candidate, |deadline| deadline.min(candidate))
}

fn mobile_monitor_wait_duration(
    core: &StereodromeCore,
    audio: &AudioPlayer,
    state_handle: &AudioStateHandle,
    last_report: Option<&MobileProgressReport>,
    cache_reconcile_pending: bool,
    last_cache_reconcile_attempt: Option<Instant>,
    crossfade_attempted: Option<&PlaybackIdentity>,
) -> Option<Duration> {
    let now = Instant::now();
    let mut deadline = None;

    if cache_reconcile_pending {
        let retry_at = last_cache_reconcile_attempt.map_or(now, |attempt| {
            attempt + MOBILE_CACHE_RECONCILE_RETRY_INTERVAL
        });
        deadline = Some(earlier_deadline(deadline, retry_at));
    }

    let (state, segment_idx) = state_handle.get_gapless_state();
    if state.is_playing {
        let report_at = last_report.map_or(now, |report| report.at + Duration::from_secs(15));
        deadline = Some(earlier_deadline(deadline, report_at));

        if !state_handle.is_crossfade_initiated()
            && state_handle.is_last_gapless_segment(segment_idx)
            && let Ok(settings) = core.get_audio_processing_settings()
            && settings.crossfade_enabled
            && let Some(identity) = audio.current_playback_identity()
            && crossfade_attempted != Some(&identity)
        {
            let remaining = (state.duration - state.position).max(0.0);
            if remaining > 0.5 {
                let until_window =
                    (remaining - f64::from(settings.crossfade_duration_ms) / 1000.0).max(0.0);
                deadline = Some(earlier_deadline(
                    deadline,
                    now + Duration::from_secs_f64(until_window),
                ));
            }
        }
    }
    if let Some(report) = last_report
        && report.pending
    {
        deadline = Some(earlier_deadline(
            deadline,
            report.at + MOBILE_PROGRESS_RETRY_INTERVAL,
        ));
    }

    deadline.map(|deadline| deadline.saturating_duration_since(now))
}

fn block_on_monitor_future<F>(
    runtime: &tokio::runtime::Runtime,
    running: &AtomicBool,
    future: F,
) -> Option<F::Output>
where
    F: Future,
{
    runtime.block_on(async move {
        tokio::pin!(future);
        loop {
            tokio::select! {
                result = &mut future => return Some(result),
                () = tokio::time::sleep(Duration::from_millis(50)) => {
                    if !running.load(Ordering::SeqCst) {
                        return None;
                    }
                }
            }
        }
    })
}

fn report_mobile_progress(
    runtime: &tokio::runtime::Runtime,
    core: &StereodromeCore,
    running: &AtomicBool,
    last_report: &mut Option<MobileProgressReport>,
    song_id: &str,
    state: &stereodrome_audio::PlaybackState,
) -> bool {
    let now = Instant::now();
    let should_report = should_report_mobile_progress(last_report.as_ref(), song_id, state, now);

    if !should_report {
        return true;
    }

    let progress = PlaybackProgress {
        song_id: song_id.to_string(),
        position_seconds: state.position,
        duration_seconds: state.duration,
        is_playing: state.is_playing,
    };
    match block_on_monitor_future(runtime, running, core.report_playback_progress(progress)) {
        None => false,
        Some(Ok(_)) => {
            *last_report = Some(MobileProgressReport {
                at: now,
                is_playing: state.is_playing,
                position: state.position,
                song_id: song_id.to_string(),
                pending: false,
            });
            true
        }
        Some(Err(error)) => {
            log::warn!(
                target: "stereodrome_ffi",
                "Failed to report mobile playback progress: {error}"
            );
            *last_report = Some(MobileProgressReport {
                at: now,
                is_playing: state.is_playing,
                position: state.position,
                song_id: song_id.to_string(),
                pending: true,
            });
            true
        }
    }
}

fn should_report_mobile_progress(
    last_report: Option<&MobileProgressReport>,
    song_id: &str,
    state: &stereodrome_audio::PlaybackState,
    now: Instant,
) -> bool {
    last_report.is_none_or(|previous| {
        (previous.pending && now.duration_since(previous.at) >= MOBILE_PROGRESS_RETRY_INTERVAL)
            || previous.song_id != song_id
            || previous.is_playing != state.is_playing
            || (previous.position - state.position).abs() >= 15.0
            || (state.is_playing && now.duration_since(previous.at) >= Duration::from_secs(15))
    })
}

async fn play_current_queue_item(
    mobile: &MobileCore,
    seek_position: Option<f64>,
) -> Result<stereodrome_audio::PlaybackStatus, String> {
    play_current_queue_item_from(&mobile.core, &mobile.audio, seek_position).await
}

async fn play_current_queue_item_from(
    core: &StereodromeCore,
    audio: &AudioPlayer,
    seek_position: Option<f64>,
) -> Result<stereodrome_audio::PlaybackStatus, String> {
    let queue = core.get_queue().map_err(|e| e.to_string())?;
    let Some(index) = queue.current_index else {
        audio.stop().map_err(|e| e.to_string())?;
        return Ok(audio.get_status());
    };
    let item = queue
        .items
        .get(index)
        .cloned()
        .ok_or_else(|| "current queue index is out of range".to_string())?;

    let prepared = prepare_queue_item_audio_from(core, item).await?;

    let status = play_prepared_audio(core, audio, prepared, None)?;

    if let Some(position) = seek_position
        && let Err(error) = audio.seek(position)
    {
        log::warn!(
            target: "stereodrome_ffi",
            "Playback started but the restored position could not be applied: {error}"
        );
    }

    Ok(if seek_position.is_some() {
        audio.get_status()
    } else {
        status
    })
}

#[derive(Clone, Copy)]
enum QueueNavigation {
    Index(usize),
    Next(bool),
    Previous,
}

impl QueueNavigation {
    fn preview(self, core: &StereodromeCore) -> Result<Option<QueueItem>, String> {
        match self {
            Self::Index(index) => core
                .get_queue()
                .map_err(|error| error.to_string())?
                .items
                .get(index)
                .cloned()
                .map(Some)
                .ok_or_else(|| format!("queue index {index} is out of range")),
            Self::Next(force) => core
                .preview_next_queue_item(Some(force))
                .map_err(|error| error.to_string()),
            Self::Previous => core
                .preview_previous_queue_item()
                .map_err(|error| error.to_string()),
        }
    }

    fn commit_if_matches(
        self,
        core: &StereodromeCore,
        expected_current_song_id: Option<&str>,
        expected_target_song_id: &str,
    ) -> Result<Option<QueueItem>, String> {
        match self {
            Self::Index(index) => core.play_queue_item_if_matches(
                index,
                expected_current_song_id,
                expected_target_song_id,
            ),
            Self::Next(force) => core.play_next_if_matches(
                Some(force),
                expected_current_song_id,
                expected_target_song_id,
            ),
            Self::Previous => {
                core.play_previous_if_matches(expected_current_song_id, expected_target_song_id)
            }
        }
        .map_err(|error| error.to_string())
    }

    fn commit(self, core: &StereodromeCore) -> Result<Option<QueueItem>, String> {
        match self {
            Self::Index(index) => core.play_queue_item(index),
            Self::Next(force) => core.play_next(Some(force)),
            Self::Previous => core.play_previous(),
        }
        .map_err(|error| error.to_string())
    }
}

async fn play_queue_navigation(
    mobile: &MobileCore,
    navigation: QueueNavigation,
) -> Result<stereodrome_audio::PlaybackStatus, String> {
    play_queue_navigation_from(&mobile.core, &mobile.audio, navigation, None).await
}

async fn play_queue_navigation_from(
    core: &StereodromeCore,
    audio: &AudioPlayer,
    navigation: QueueNavigation,
    expected_playback: Option<PlaybackIdentity>,
) -> Result<stereodrome_audio::PlaybackStatus, String> {
    if expected_playback
        .as_ref()
        .is_some_and(|expected| audio.current_playback_identity().as_ref() != Some(expected))
    {
        return Err("playback changed before queue navigation began".to_string());
    }
    let queue_before = core.get_queue().map_err(|error| error.to_string())?;
    let expected_current_song_id = queue_before
        .current_index
        .and_then(|index| queue_before.items.get(index))
        .map(|item| item.song_id.clone());
    let preview = navigation.preview(core)?;

    let Some(item) = preview else {
        audio.stop().map_err(|error| error.to_string())?;
        let committed = navigation.commit(core)?;
        if committed.is_some() {
            return Err("queue navigation changed while playback was being prepared".to_string());
        }
        return Ok(audio.get_status());
    };

    let expected_song_id = item.song_id.clone();
    let prepared = prepare_queue_item_audio_from(core, item).await?;
    if expected_playback
        .as_ref()
        .is_some_and(|expected| audio.current_playback_identity().as_ref() != Some(expected))
    {
        return Err("playback changed while queue navigation was being prepared".to_string());
    }
    let status = play_prepared_audio(core, audio, prepared, expected_playback)?;
    let committed = match navigation.commit_if_matches(
        core,
        expected_current_song_id.as_deref(),
        &expected_song_id,
    ) {
        Ok(committed) => committed,
        Err(error) => {
            if let Err(stop_error) = audio.stop() {
                log::warn!(
                    target: "stereodrome_ffi",
                    "Queue commit failed after playback started and stop was not acknowledged: \
                     {stop_error}"
                );
                return Ok(status);
            }
            return Err(error);
        }
    };

    if committed.as_ref().map(|item| item.song_id.as_str()) != Some(expected_song_id.as_str()) {
        if let Err(stop_error) = audio.stop() {
            log::warn!(
                target: "stereodrome_ffi",
                "Queue changed after playback started and stop was not acknowledged: {stop_error}"
            );
            return Ok(status);
        }
        return Err("queue navigation changed while playback was being prepared".to_string());
    }

    Ok(status)
}

async fn resume_current_playback(
    mobile: &MobileCore,
) -> Result<stereodrome_audio::PlaybackStatus, String> {
    if mobile.audio.get_status().current_song_id.is_some() {
        mobile.audio.resume().map_err(|e| e.to_string())?;
        return Ok(mobile.audio.get_status());
    }

    let persisted = mobile
        .core
        .get_playback_state()
        .map_err(|e| e.to_string())?;
    let mut queue = mobile.core.get_queue().map_err(|e| e.to_string())?;
    if queue.current_index.is_none()
        && let Some(saved_song_id) = persisted.current_song_id.as_deref()
        && let Some(index) = queue
            .items
            .iter()
            .position(|item| item.song_id == saved_song_id)
    {
        mobile
            .core
            .play_queue_item(index)
            .map_err(|e| e.to_string())?;
        queue = mobile.core.get_queue().map_err(|e| e.to_string())?;
    }

    let current_song_id = queue
        .current_index
        .and_then(|index| queue.items.get(index))
        .map(|item| item.song_id.as_str());

    let seek_position = match (persisted.current_song_id.as_deref(), current_song_id) {
        (Some(saved_song_id), Some(queue_song_id))
            if saved_song_id == queue_song_id && persisted.position_seconds > 0.5 =>
        {
            let duration = if persisted.duration_seconds > 0.0 {
                persisted.duration_seconds
            } else {
                queue
                    .current_index
                    .and_then(|index| queue.items.get(index))
                    .map_or(0.0, |item| duration_seconds(item.duration))
            };
            Some(if duration > 1.0 {
                persisted.position_seconds.clamp(0.0, duration - 1.0)
            } else {
                0.0
            })
        }
        _ => None,
    };

    let status = play_current_queue_item(mobile, seek_position).await?;
    if let Err(error) = prepare_next_transition(mobile).await {
        log::warn!(
            target: "stereodrome_ffi",
            "Failed to prepare next transition after resuming playback: {error}"
        );
    }
    Ok(status)
}

async fn prepare_next_transition(mobile: &MobileCore) -> Result<bool, String> {
    prepare_next_transition_from(&mobile.core, &mobile.audio).await
}

async fn prepare_next_transition_from(
    core: &StereodromeCore,
    audio: &AudioPlayer,
) -> Result<bool, String> {
    let settings = core
        .get_audio_processing_settings()
        .map_err(|e| e.to_string())?;
    if !settings.gapless_enabled {
        return Ok(false);
    }
    if audio.get_status().current_song_id.is_none() {
        return Ok(false);
    }

    let queue = core.get_queue().map_err(|e| e.to_string())?;
    if queue.repeat_mode == RepeatMode::One {
        return Ok(false);
    }
    let Some(current_index) = queue.current_index else {
        return Ok(false);
    };
    let Some(current) = queue.items.get(current_index) else {
        return Ok(false);
    };
    let Some(next) = core.peek_next_queue_item().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    if current.song_id == next.song_id
        || !core
            .songs_are_gapless_eligible(&current.song_id, &next.song_id)
            .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    let Some(expected_playback) = audio.current_playback_identity() else {
        return Ok(false);
    };
    if expected_playback.song_id() != current.song_id {
        return Ok(false);
    }

    let next_song_id = next.song_id.clone();
    let prepared = prepare_queue_item_audio_from(core, next).await?;
    audio
        .append_gapless(
            expected_playback,
            prepared.audio_data,
            prepared.metadata,
            prepared.duration_secs,
            prepared.processing.normalization_gain,
            prepared.processing.dynamics_preset,
            prepared.processing.binaural_preset,
            prepared.processing.equalizer_settings,
        )
        .map(|()| true)
        .map_err(|error| {
            invalidate_cache_after_decode_error(core, &next_song_id, &error);
            error.to_string()
        })
}

async fn crossfade_next_from(
    core: &StereodromeCore,
    audio: &AudioPlayer,
    expected_playback: Option<&PlaybackIdentity>,
) -> Result<Option<QueueState>, String> {
    if expected_playback
        .is_some_and(|expected| audio.current_playback_identity().as_ref() != Some(expected))
    {
        return Ok(None);
    }
    let settings = core
        .get_audio_processing_settings()
        .map_err(|e| e.to_string())?;
    if !settings.crossfade_enabled || !audio.get_status().is_playing {
        return Ok(None);
    }

    let queue = core.get_queue().map_err(|e| e.to_string())?;
    if queue.repeat_mode == RepeatMode::One {
        return Ok(None);
    }
    let Some(current_index) = queue.current_index else {
        return Ok(None);
    };
    let Some(current) = queue.items.get(current_index) else {
        return Ok(None);
    };
    let current_song_id = current.song_id.clone();
    let Some(next) = core.peek_next_queue_item().map_err(|e| e.to_string())? else {
        return Ok(None);
    };

    if settings.gapless_enabled
        && core
            .songs_are_gapless_eligible(&current.song_id, &next.song_id)
            .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }

    let next_song_id = next.song_id.clone();
    let prepared = prepare_queue_item_audio_from(core, next).await?;
    if expected_playback
        .is_some_and(|expected| audio.current_playback_identity().as_ref() != Some(expected))
    {
        return Ok(None);
    }
    audio
        .crossfade_play(CrossfadePlayRequest {
            expected_playback: expected_playback.cloned(),
            audio_data: prepared.audio_data,
            metadata: prepared.metadata,
            duration_secs: prepared.duration_secs,
            normalization_gain: prepared.processing.normalization_gain,
            dynamics_preset: prepared.processing.dynamics_preset,
            binaural_preset: prepared.processing.binaural_preset,
            equalizer_settings: prepared.processing.equalizer_settings,
            crossfade_duration_ms: settings.crossfade_duration_ms,
        })
        .map_err(|error| {
            invalidate_cache_after_decode_error(core, &next_song_id, &error);
            error.to_string()
        })?;

    if let Err(error) =
        core.play_next_if_matches(Some(false), Some(&current_song_id), &next_song_id)
    {
        let _ = audio.stop();
        return Err(error.to_string());
    }
    Ok(Some(core.get_queue().map_err(|e| e.to_string())?))
}

async fn apply_audio_settings(
    mobile: &MobileCore,
) -> Result<stereodrome_audio::PlaybackStatus, String> {
    let status = mobile.audio.get_status();
    let Some(_) = status.current_song_id else {
        return Ok(status);
    };
    let was_playing = status.is_playing;
    let position = status.position;
    play_current_queue_item_from(&mobile.core, &mobile.audio, Some(position)).await?;
    if !was_playing {
        mobile.audio.pause().map_err(|e| e.to_string())?;
    }
    Ok(mobile.audio.get_status())
}

struct AudioProcessing {
    normalization_gain: Option<f32>,
    dynamics_preset: Option<DynamicsPreset>,
    binaural_preset: Option<BinauralPreset>,
    equalizer_settings: Option<EqualizerSettings>,
}

struct PreparedAudioItem {
    audio_data: Arc<[u8]>,
    metadata: SongMetadata,
    duration_secs: f64,
    processing: AudioProcessing,
}

fn play_prepared_audio(
    core: &StereodromeCore,
    audio: &AudioPlayer,
    prepared: PreparedAudioItem,
    expected_playback: Option<PlaybackIdentity>,
) -> Result<stereodrome_audio::PlaybackStatus, String> {
    let song_id = prepared.metadata.id.clone();
    if let Err(error) = audio.play_with_expected(
        expected_playback,
        prepared.audio_data,
        prepared.metadata,
        prepared.duration_secs,
        prepared.processing.normalization_gain,
        prepared.processing.dynamics_preset,
        prepared.processing.binaural_preset,
        prepared.processing.equalizer_settings,
    ) {
        invalidate_cache_after_decode_error(core, &song_id, &error);
        return Err(error.to_string());
    }
    Ok(audio.get_status())
}

fn invalidate_cache_after_decode_error(core: &StereodromeCore, song_id: &str, error: &AudioError) {
    if !matches!(error, AudioError::Decode(_)) {
        return;
    }
    if let Err(invalidation_error) = core.invalidate_cached_song(song_id) {
        log::warn!(
            target: "stereodrome_ffi",
            "Failed to invalidate undecodable cached song {song_id}: {invalidation_error}"
        );
    }
}

async fn prepare_queue_item_audio_from(
    core: &StereodromeCore,
    item: QueueItem,
) -> Result<PreparedAudioItem, String> {
    let status = core
        .download_song(item.song_id.clone())
        .await
        .map_err(|e| e.to_string())?;
    let path = status
        .path
        .as_deref()
        .ok_or_else(|| format!("song {} did not produce a cached audio path", item.song_id))?;
    let audio_path = file_uri_to_path(path)?;
    let audio_data = std::fs::read(&audio_path).map_err(|e| e.to_string())?;
    let settings = core
        .get_audio_processing_settings()
        .map_err(|e| e.to_string())?;
    let processing = audio_processing_from_settings(&settings)?;

    Ok(PreparedAudioItem {
        audio_data: Arc::<[u8]>::from(audio_data),
        metadata: SongMetadata {
            id: item.song_id,
            title: item.title,
            artist: item.artist,
            album: item.album,
            cover_art_id: None,
        },
        duration_secs: duration_seconds(item.duration),
        processing,
    })
}

fn audio_processing_from_settings(
    settings: &AudioProcessingSettings,
) -> Result<AudioProcessing, String> {
    let normalization_gain = if settings.normalization_enabled || settings.preamp_db.abs() > 0.01 {
        Some(10.0_f32.powf(narrow_f64_to_f32(settings.preamp_db, "preamp_db")? / 20.0))
    } else {
        None
    };
    let dynamics_preset = if settings.dynamics_enabled {
        Some(match settings.dynamics_preset.as_str() {
            "light" => DynamicsPreset::Light,
            "medium" => DynamicsPreset::Medium,
            "heavy" => DynamicsPreset::Heavy,
            other => return Err(format!("unknown dynamics preset: {other}")),
        })
    } else {
        None
    };
    let binaural_preset = if settings.binaural_enabled {
        Some(match settings.binaural_preset.as_str() {
            "light" => BinauralPreset::Default,
            "medium" => BinauralPreset::Jmeier,
            "strong" => BinauralPreset::Aggressive,
            other => return Err(format!("unknown binaural preset: {other}")),
        })
    } else {
        None
    };
    let equalizer_settings = if settings.equalizer_enabled {
        Some(EqualizerSettings::new(
            settings
                .equalizer_bands_db
                .iter()
                .map(|value| narrow_f64_to_f32(*value, "equalizer band"))
                .collect::<Result<Vec<_>, _>>()?,
        ))
    } else {
        None
    };

    Ok(AudioProcessing {
        normalization_gain,
        dynamics_preset,
        binaural_preset,
        equalizer_settings,
    })
}

fn duration_seconds(duration: i64) -> f64 {
    let duration = duration.clamp(0, i64::from(u32::MAX));
    f64::from(u32::try_from(duration).expect("clamped duration fits in u32"))
}

fn narrow_f64_to_f32(value: f64, name: &str) -> Result<f32, String> {
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(format!("{name} is outside the supported f32 range"));
    }

    // The finite range check above makes this the only intentionally narrowing step.
    #[allow(clippy::cast_possible_truncation)]
    {
        Ok(value as f32)
    }
}

fn file_uri_to_path(value: &str) -> Result<PathBuf, String> {
    if value.starts_with("file://") {
        Url::parse(value)
            .map_err(|e| e.to_string())?
            .to_file_path()
            .map_err(|()| format!("invalid file URI: {value}"))
    } else {
        Ok(PathBuf::from(value))
    }
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

    fn test_queue_item(id: &str) -> QueueItem {
        QueueItem {
            song_id: id.to_string(),
            title: format!("Song {id}"),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration: 180,
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
                "clearQueue" => CoreCommand::ClearQueue,
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
            assert_eq!(typed_response["accepted_revision"], index + 1);
        }

        let snapshot = typed.mobile().core_runtime.snapshot();
        assert_eq!(snapshot.status, CommandStatus::Succeeded);
        assert_eq!(snapshot.accepted_revision, 5);
        assert_eq!(snapshot.value.unwrap()["revision"], 5);
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
    fn concurrent_mobile_jobs_and_backup_exclusion_are_characterized() {
        let core = TestMobileCore::new("job-exclusion-characterization");
        let mobile = core.mobile();

        mobile
            .sync_state
            .lock()
            .expect("sync state lock")
            .active_job = Some(MobileSyncJob::Incremental);
        assert_eq!(
            start_sync_job(mobile, MobileSyncJob::Full),
            Err("incremental library sync is already running".to_string())
        );
        assert!(ensure_backup_jobs_idle(mobile).is_err());

        mobile
            .sync_state
            .lock()
            .expect("sync state lock")
            .active_job = None;
        mobile
            .saved_playlist_offline_state
            .lock()
            .expect("saved playlist state lock")
            .running = true;
        assert!(ensure_backup_jobs_idle(mobile).is_err());

        mobile
            .saved_playlist_offline_state
            .lock()
            .expect("saved playlist state lock")
            .running = false;
        mobile
            .prefetch_state
            .lock()
            .expect("prefetch state lock")
            .requested_plan = Some(QueuePrefetchPlan {
            queue_revision: 1,
            current_index: None,
            song_ids: vec!["song".to_string()],
        });
        assert!(ensure_backup_jobs_idle(mobile).is_err());

        mobile
            .prefetch_state
            .lock()
            .expect("prefetch state lock")
            .requested_plan = None;
        assert!(ensure_backup_jobs_idle(mobile).is_ok());
    }

    #[test]
    fn mobile_core_shutdown_cancels_an_active_prefetch_worker() {
        let core = TestMobileCore::new("shutdown-prefetch-characterization");
        core.mobile()
            .core
            .add_songs_to_queue(vec![test_queue_item("current"), test_queue_item("next")])
            .expect("queue is populated");
        core.mobile()
            .core
            .play_queue_item(0)
            .expect("current song is selected");

        start_queue_prefetch(core.mobile(), false).expect("prefetch starts");

        drop(core);
    }

    #[test]
    fn failed_navigation_preparation_does_not_advance_queue() {
        static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

        let test_id = NEXT_TEST_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let data_dir = std::env::temp_dir().join(format!(
            "stereodrome-ffi-navigation-{}-{test_id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&data_dir);
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        core.add_to_queue(test_queue_item("a"))
            .expect("first queue item is added");
        core.add_to_queue(test_queue_item("b"))
            .expect("second queue item is added");
        core.play_queue_item(0)
            .expect("initial queue item is selected");
        let audio = AudioPlayer::new_with_spectrum(false).expect("audio player initializes");
        let runtime = tokio::runtime::Runtime::new().expect("runtime initializes");

        let result = runtime.block_on(async {
            play_queue_navigation_from(&core, &audio, QueueNavigation::Next(true), None).await
        });

        assert!(
            result.is_err(),
            "uncached playback without a server must fail"
        );
        assert_eq!(
            core.get_queue()
                .expect("queue remains readable")
                .current_index,
            Some(0)
        );

        drop(audio);
        drop(core);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn mobile_monitor_future_stops_when_shutdown_is_requested() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime initializes");
        let running = AtomicBool::new(false);

        let result = block_on_monitor_future(&runtime, &running, std::future::pending::<()>());

        assert!(result.is_none());
    }

    #[test]
    fn stopped_mobile_monitor_has_no_playback_deadline() {
        let data_dir = std::env::temp_dir().join(format!(
            "stereodrome-ffi-idle-monitor-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&data_dir);
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let audio = AudioPlayer::new_with_spectrum(false).expect("audio initializes");
        let state_handle = audio.state_handle();

        let wait =
            mobile_monitor_wait_duration(&core, &audio, &state_handle, None, false, None, None);

        assert!(wait.is_none());
        drop(audio);
        drop(core);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn paused_progress_is_not_reported_again_on_elapsed_time() {
        let now = Instant::now();
        let previous = MobileProgressReport {
            at: now
                .checked_sub(Duration::from_secs(30))
                .expect("test instant supports a 30-second offset"),
            is_playing: false,
            position: 10.0,
            song_id: "song".to_string(),
            pending: false,
        };
        let state = stereodrome_audio::PlaybackState {
            state: PlaybackLifecycleState::Paused,
            is_playing: false,
            position: 10.0,
            duration: 180.0,
            volume: 0.8,
            song: Some(SongMetadata {
                id: "song".to_string(),
                title: "Song".to_string(),
                artist: "Artist".to_string(),
                album: "Album".to_string(),
                cover_art_id: None,
            }),
            output_state: AudioOutputState::Ready,
        };

        assert!(!should_report_mobile_progress(
            Some(&previous),
            "song",
            &state,
            now
        ));
    }

    #[test]
    fn playback_snapshot_policy_includes_dispatch_mutations_that_emit() {
        let emitting_methods = [
            "importPortableBackup",
            "setAudioProcessingSettings",
            "audioPlayCurrent",
            "audioPlayQueueItem",
            "audioPlayNext",
            "audioPlayPrevious",
            "audioApplySettings",
            "audioPause",
            "audioResume",
            "audioRebuildOutput",
            "audioStop",
            "audioSeek",
            "audioSetVolume",
            "playSongWithQueue",
            "addToQueue",
            "addSongsToQueue",
            "insertNext",
            "insertNextSongs",
            "removeFromQueue",
            "clearQueue",
            "moveQueueItem",
            "playQueueItem",
            "playNext",
            "playPrevious",
            "toggleShuffle",
            "setRepeatMode",
            "cycleRepeatMode",
            "rerollNext",
        ];

        for method in emitting_methods {
            assert!(
                should_emit_playback_snapshot(method),
                "{method} should emit a playback snapshot after successful dispatch"
            );
        }
    }

    #[test]
    fn playback_snapshot_policy_excludes_non_emitting_dispatch_methods() {
        let non_emitting_methods = [
            "getPlaybackSnapshot",
            "getPlaybackState",
            "audioPrepareNextTransition",
            "getArtists",
            "unknownMethod",
        ];

        for method in non_emitting_methods {
            assert!(
                !should_emit_playback_snapshot(method),
                "{method} should not emit a playback snapshot from dispatch"
            );
        }
    }

    #[test]
    fn stale_prefetch_is_cancelled_for_queue_stop_and_offline_mutations() {
        let cancelling_methods = [
            "disconnectServer",
            "setConnectivitySettings",
            "audioPlayNext",
            "audioPrepareNextTransition",
            "audioStop",
            "playSongWithQueue",
            "insertNext",
            "removeFromQueue",
            "clearQueue",
            "moveQueueItem",
            "toggleShuffle",
            "setRepeatMode",
        ];

        for method in cancelling_methods {
            assert!(
                should_cancel_queue_prefetch(method),
                "{method} should cancel stale queue prefetch"
            );
        }
        assert!(!should_cancel_queue_prefetch("prefetchNext"));
        assert!(!should_cancel_queue_prefetch("getPlaybackSnapshot"));
    }

    #[test]
    fn cancelling_prefetch_clears_bounded_pending_work() {
        let plan = QueuePrefetchPlan {
            queue_revision: 3,
            current_index: Some(1),
            song_ids: vec!["next".to_string()],
        };
        let cancellation = PrefetchCancellationToken::new();
        let state = Arc::new(Mutex::new(BackgroundPrefetchState {
            closed: false,
            running: true,
            cancellation_generation: 0,
            worker_generation: 1,
            worker_handle: None,
            active_plan: Some(plan.clone()),
            requested_plan: Some(plan.clone()),
            cancellation: Some(cancellation.clone()),
            last_completed_plan: Some(plan.clone()),
        }));

        signal_queue_prefetch_cancellation(&state, false).expect("prefetch cancellation");

        let state_guard = state.lock().expect("prefetch state lock");
        assert!(cancellation.is_cancelled());
        assert!(state_guard.active_plan.is_none());
        assert!(state_guard.requested_plan.is_none());
        assert_eq!(state_guard.last_completed_plan.as_ref(), Some(&plan));
        drop(state_guard);

        signal_queue_prefetch_cancellation(&state, true).expect("prefetch invalidation");
        assert!(
            state
                .lock()
                .expect("prefetch state lock")
                .last_completed_plan
                .is_none()
        );
    }

    #[test]
    fn old_prefetch_worker_cannot_clear_replacement_state() {
        let state = Arc::new(Mutex::new(BackgroundPrefetchState {
            running: true,
            worker_generation: 2,
            ..BackgroundPrefetchState::default()
        }));

        drop(BackgroundPrefetchGuard {
            state: Arc::clone(&state),
            worker_generation: 1,
        });

        assert!(state.lock().expect("prefetch state lock").running);
    }

    #[test]
    fn closed_prefetch_state_rejects_late_monitor_start() {
        static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

        let test_id = NEXT_TEST_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let data_dir = std::env::temp_dir().join(format!(
            "stereodrome-ffi-prefetch-close-{}-{test_id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&data_dir);
        let core = Arc::new(StereodromeCore::new(&data_dir).expect("core initializes"));
        let runtime = tokio::runtime::Runtime::new().expect("runtime initializes");
        let state = Arc::new(Mutex::new(BackgroundPrefetchState {
            closed: true,
            ..BackgroundPrefetchState::default()
        }));

        spawn_queue_prefetch(&runtime, core, &state, false)
            .expect("closed prefetch start is ignored");

        let state = state.lock().expect("prefetch state lock");
        assert!(!state.running);
        assert!(state.worker_handle.is_none());
        assert!(state.requested_plan.is_none());
        drop(state);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn decode_failure_invalidates_cached_audio_but_output_failure_does_not() {
        static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

        let test_id = NEXT_TEST_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let data_dir = std::env::temp_dir().join(format!(
            "stereodrome-ffi-decode-cache-{}-{test_id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&data_dir);
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let cache_path = data_dir.join("audio_cache").join("song.mp3");
        std::fs::create_dir_all(cache_path.parent().expect("cache parent"))
            .expect("create cache directory");
        std::fs::write(&cache_path, b"invalid audio").expect("write invalid cache");

        invalidate_cache_after_decode_error(
            &core,
            "song",
            &AudioError::Playback("output unavailable".to_string()),
        );
        assert!(cache_path.exists());

        invalidate_cache_after_decode_error(
            &core,
            "song",
            &AudioError::Decode("invalid media".to_string()),
        );
        assert!(!cache_path.exists());
        let _ = std::fs::remove_dir_all(data_dir);
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
    fn mobile_file_state_event_serialization_is_tagged_and_sequenced() {
        let snapshot = MobileFileStateSnapshot {
            seq: 7,
            downloaded_song_ids: vec!["cached".to_string()],
            downloading_song_ids: vec!["active".to_string()],
        };

        let event = serde_json::to_value(MobileCoreEventEnvelope {
            stream_id: 3,
            seq: 11,
            event: MobileCoreEvent::FileState(snapshot),
        })
        .expect("file state event serializes");

        assert_eq!(event["stream_id"], 3);
        assert_eq!(event["seq"], 11);
        assert_eq!(event["type"], "file-state");
        assert_eq!(event["payload"]["seq"], 7);
        assert_eq!(event["payload"]["downloaded_song_ids"][0], "cached");
        assert_eq!(event["payload"]["downloading_song_ids"][0], "active");
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
