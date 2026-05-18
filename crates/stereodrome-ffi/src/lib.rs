//! Mobile FFI boundary for Stereodrome.
//!
//! The first mobile implementation uses JSON-over-FFI so the Swift/Kotlin Expo
//! module can remain thin while the Rust API stabilizes. The crate is isolated
//! so a UniFFI surface can be generated here without touching the desktop
//! Tauri adapter.

use std::ffi::{CStr, CString, c_char};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

use log::{Level, LevelFilter, Metadata, Record};
use serde::Deserialize;
use serde_json::Value;
use stereodrome_audio::{
    AudioPlayer, BinauralPreset, CrossfadePlayRequest, DynamicsPreset, EqualizerSettings,
    SongMetadata,
};
use stereodrome_core::queue::{QueueItem, QueueState, RepeatMode};
use stereodrome_core::{
    AudioProcessingSettings, ConnectParams, LibrarySyncStatus, PlaybackProgress,
    ServerSettingsUpdate, StereodromeCore,
};
use url::Url;

static MOBILE_LOGGER: MobileLogger = MobileLogger;
static INIT_LOGGER: Once = Once::new();
static INIT_PANIC_HOOK: Once = Once::new();
static LOG_CALLBACK: Mutex<Option<MobileLogCallback>> = Mutex::new(None);

type MobileLogCallback = extern "C" fn(*const c_char);

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
            eprintln!("{message}");
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
            let location = panic_info
                .location()
                .map(|location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                })
                .unwrap_or_else(|| "unknown location".to_string());
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

pub struct MobileCore {
    core: Arc<StereodromeCore>,
    audio: Arc<AudioPlayer>,
    runtime: tokio::runtime::Runtime,
    data_dir: PathBuf,
    sync_state: Arc<Mutex<MobileSyncState>>,
    monitor_running: Arc<AtomicBool>,
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

#[derive(Debug, Default)]
struct MobileSyncState {
    active_job: Option<MobileSyncJob>,
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
        match (
            StereodromeCore::new(&data_dir),
            AudioPlayer::new_with_spectrum(false),
            tokio::runtime::Runtime::new(),
        ) {
            (Ok(core), Ok(audio), Ok(runtime)) => {
                let core = Arc::new(core);
                let audio = Arc::new(audio);
                let monitor_running = Arc::new(AtomicBool::new(true));
                start_mobile_playback_monitor(
                    Arc::clone(&core),
                    Arc::clone(&audio),
                    Arc::clone(&monitor_running),
                );

                Box::into_raw(Box::new(MobileCore {
                    core,
                    audio,
                    runtime,
                    data_dir,
                    sync_state: Arc::new(Mutex::new(MobileSyncState::default())),
                    monitor_running,
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
            let mobile = Box::from_raw(core);
            mobile.monitor_running.store(false, Ordering::SeqCst);
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

fn dispatch(mobile: &MobileCore, method: &str, payload: Value) -> Result<String, String> {
    let runtime = &mobile.runtime;
    let core = &mobile.core;

    match method {
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
        "syncLibrary" => json_result(start_sync_job(mobile, MobileSyncJob::Full)),
        "syncLibraryIncremental" => json_result(start_sync_job(mobile, MobileSyncJob::Incremental)),
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
        "prefetchNext" => json_result(runtime.block_on(async { core.prefetch_next().await })),
        "getPlaybackState" => json_result(core.get_playback_state()),
        "savePlaybackPosition" => {
            let progress = parse_payload::<PlaybackProgress>(payload)?;
            json_result(core.save_playback_position(progress))
        }
        "reportPlaybackProgress" => {
            let progress = parse_payload::<PlaybackProgress>(payload)?;
            json_result(runtime.block_on(async { core.report_playback_progress(progress).await }))
        }
        "getAudioProcessingSettings" => json_result(core.get_audio_processing_settings()),
        "setAudioProcessingSettings" => {
            let settings = parse_payload::<AudioProcessingSettings>(payload)?;
            json_result(core.set_audio_processing_settings(settings))
        }
        "audioPlayCurrent" => {
            json_result(runtime.block_on(async { play_current_queue_item(mobile, None).await }))
        }
        "audioApplySettings" => {
            json_result(runtime.block_on(async { apply_audio_settings(mobile).await }))
        }
        "audioPrepareNextTransition" => {
            json_result(runtime.block_on(async { prepare_next_transition(mobile).await }))
        }
        "audioCrossfadeNext" => {
            json_result(runtime.block_on(async { crossfade_next(mobile).await }))
        }
        "audioPause" => json_result(mobile.audio.pause().map(|_| ())),
        "audioResume" => json_result(mobile.audio.resume().map(|_| ())),
        "audioStop" => json_result(mobile.audio.stop().map(|_| ())),
        "audioSeek" => {
            let position = parse_payload::<f64>(payload)?;
            json_result(mobile.audio.seek(position).map(|_| ()))
        }
        "audioSetVolume" => {
            let volume = parse_payload::<f32>(payload)?;
            json_result(mobile.audio.set_volume(volume).map(|_| ()))
        }
        "audioGetStatus" => json_result(Ok::<_, String>(mobile.audio.get_status())),
        "getQueue" => json_result(core.get_queue()),
        "playSongWithQueue" => {
            let args = parse_payload::<PlaySongWithQueuePayload>(payload)?;
            json_result(core.play_song_with_queue(args.song_id, args.song_ids))
        }
        "addToQueue" => {
            let item = parse_payload::<QueueItem>(payload)?;
            json_result(core.add_to_queue(item))
        }
        "addSongsToQueue" => {
            let items = parse_payload::<Vec<QueueItem>>(payload)?;
            json_result(core.add_songs_to_queue(items))
        }
        "insertNext" => {
            let item = parse_payload::<QueueItem>(payload)?;
            json_result(core.insert_next(item))
        }
        "insertNextSongs" => {
            let items = parse_payload::<Vec<QueueItem>>(payload)?;
            json_result(core.insert_next_songs(items))
        }
        "removeFromQueue" => {
            let index = parse_payload::<usize>(payload)?;
            json_result(core.remove_from_queue(index))
        }
        "clearQueue" => json_result(core.clear_queue()),
        "moveQueueItem" => {
            let args = parse_payload::<MoveQueueItemPayload>(payload)?;
            json_result(core.move_queue_item(args.from, args.to))
        }
        "playQueueItem" => {
            let index = parse_payload::<usize>(payload)?;
            json_result(core.play_queue_item(index))
        }
        "playNext" => {
            let force = parse_payload::<Option<bool>>(payload)?;
            json_result(core.play_next(force))
        }
        "playPrevious" => json_result(core.play_previous()),
        "toggleShuffle" => json_result(core.toggle_shuffle()),
        "setRepeatMode" => {
            let mode = parse_payload::<RepeatMode>(payload)?;
            json_result(core.set_repeat_mode(mode))
        }
        "cycleRepeatMode" => json_result(core.cycle_repeat_mode()),
        "rerollNext" => json_result(core.reroll_next()),
        other => Err(format!("unknown method: {other}")),
    }
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

    let data_dir = mobile.data_dir.clone();
    let sync_state = Arc::clone(&mobile.sync_state);
    thread::spawn(move || {
        let result = panic::catch_unwind(AssertUnwindSafe(|| run_sync_job(data_dir, job)));
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
    });

    Ok(())
}

fn run_sync_job(data_dir: PathBuf, job: MobileSyncJob) -> Result<(), String> {
    log::info!(
        target: "stereodrome_ffi",
        "Starting mobile {} in background",
        job.display_name()
    );
    let core = StereodromeCore::new(data_dir).map_err(|error| error.to_string())?;
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

fn get_mobile_library_sync_status(mobile: &MobileCore) -> Result<LibrarySyncStatus, String> {
    let mut status = mobile
        .core
        .get_library_sync_status()
        .map_err(|error| error.to_string())?;
    let active_job = mobile
        .sync_state
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

fn start_mobile_playback_monitor(
    core: Arc<StereodromeCore>,
    audio: Arc<AudioPlayer>,
    running: Arc<AtomicBool>,
) {
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

        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));

            let (state, segment_idx) = state_handle.get_gapless_state();
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
                match core.play_next(Some(false)) {
                    Ok(Some(next)) => {
                        let progress = PlaybackProgress {
                            song_id: next.song_id.clone(),
                            position_seconds: 0.0,
                            duration_seconds: next.duration as f64,
                            is_playing: true,
                        };
                        let _ = runtime
                            .block_on(async { core.report_playback_progress(progress).await });
                        let _ = runtime
                            .block_on(async { prepare_next_transition_from(&core, &audio).await });
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

            report_mobile_progress(&runtime, &core, &mut last_report, &song.id, &state);

            if state.is_playing
                && state_handle.is_last_gapless_segment(segment_idx)
                && !state_handle.is_crossfade_initiated()
            {
                match core.get_audio_processing_settings() {
                    Ok(settings) if settings.crossfade_enabled => {
                        let crossfade_window_seconds =
                            settings.crossfade_duration_ms as f64 / 1000.0;
                        let remaining = state.duration - state.position;
                        if remaining <= crossfade_window_seconds && remaining > 0.5 {
                            state_handle.set_crossfade_initiated(true);
                            match runtime
                                .block_on(async { crossfade_next_from(&core, &audio).await })
                            {
                                Ok(Some(_)) => {
                                    let _ = runtime.block_on(async {
                                        prepare_next_transition_from(&core, &audio).await
                                    });
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

            let playback_finished = state.duration > 0.0
                && state.position >= state.duration - 0.2
                && !state.is_playing
                && !state_handle.is_crossfade_initiated();

            if playback_finished {
                state_handle.clear_finished_state();
                last_segment_idx = 0;
                last_report = None;

                match core.play_next(Some(false)) {
                    Ok(Some(_)) => {
                        if let Err(error) = runtime.block_on(async {
                            play_current_queue_item_from(&core, &audio, None).await
                        }) {
                            log::warn!(
                                target: "stereodrome_ffi",
                                "Failed to advance mobile playback after track ended: {error}"
                            );
                        }
                        let _ = runtime
                            .block_on(async { prepare_next_transition_from(&core, &audio).await });
                    }
                    Ok(None) => {
                        let _ = core.save_playback_position(PlaybackProgress {
                            song_id: song.id,
                            position_seconds: 0.0,
                            duration_seconds: state.duration,
                            is_playing: false,
                        });
                    }
                    Err(error) => {
                        log::warn!(
                            target: "stereodrome_ffi",
                            "Failed to advance mobile queue after track ended: {error}"
                        );
                    }
                }
            }
        }
    });
}

struct MobileProgressReport {
    at: Instant,
    is_playing: bool,
    position: f64,
    song_id: String,
}

fn report_mobile_progress(
    runtime: &tokio::runtime::Runtime,
    core: &StereodromeCore,
    last_report: &mut Option<MobileProgressReport>,
    song_id: &str,
    state: &stereodrome_audio::PlaybackState,
) {
    let now = Instant::now();
    let should_report = last_report.as_ref().is_none_or(|previous| {
        previous.song_id != song_id
            || previous.is_playing != state.is_playing
            || (previous.position - state.position).abs() >= 15.0
            || now.duration_since(previous.at) >= Duration::from_secs(15)
    });

    if !should_report {
        return;
    }

    *last_report = Some(MobileProgressReport {
        at: now,
        is_playing: state.is_playing,
        position: state.position,
        song_id: song_id.to_string(),
    });

    let progress = PlaybackProgress {
        song_id: song_id.to_string(),
        position_seconds: state.position,
        duration_seconds: state.duration,
        is_playing: state.is_playing,
    };
    let _ = runtime.block_on(async { core.report_playback_progress(progress).await });
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

    audio
        .play(
            prepared.audio_data,
            prepared.metadata,
            prepared.duration_secs,
            prepared.processing.normalization_gain,
            prepared.processing.dynamics_preset,
            prepared.processing.binaural_preset,
            prepared.processing.equalizer_settings,
        )
        .map_err(|e| e.to_string())?;

    if let Some(position) = seek_position {
        audio.seek(position).map_err(|e| e.to_string())?;
    }

    Ok(audio.get_status())
}

async fn prepare_next_transition(mobile: &MobileCore) -> Result<(), String> {
    prepare_next_transition_from(&mobile.core, &mobile.audio).await
}

async fn prepare_next_transition_from(
    core: &StereodromeCore,
    audio: &AudioPlayer,
) -> Result<(), String> {
    let settings = core
        .get_audio_processing_settings()
        .map_err(|e| e.to_string())?;
    if !settings.gapless_enabled {
        return Ok(());
    }
    if audio.get_status().current_song_id.is_none() {
        return Ok(());
    }

    let queue = core.get_queue().map_err(|e| e.to_string())?;
    if queue.repeat_mode == RepeatMode::One {
        return Ok(());
    }
    let Some(current_index) = queue.current_index else {
        return Ok(());
    };
    let Some(current) = queue.items.get(current_index) else {
        return Ok(());
    };
    let Some(next) = core.peek_next_queue_item().map_err(|e| e.to_string())? else {
        return Ok(());
    };
    if current.song_id == next.song_id
        || !core
            .songs_are_gapless_eligible(&current.song_id, &next.song_id)
            .map_err(|e| e.to_string())?
    {
        return Ok(());
    }

    let prepared = prepare_queue_item_audio_from(core, next).await?;
    audio
        .append_gapless(
            prepared.audio_data,
            prepared.metadata,
            prepared.duration_secs,
            prepared.processing.normalization_gain,
            prepared.processing.dynamics_preset,
            prepared.processing.binaural_preset,
            prepared.processing.equalizer_settings,
        )
        .map_err(|e| e.to_string())
}

async fn crossfade_next(mobile: &MobileCore) -> Result<Option<QueueState>, String> {
    if mobile.audio.is_crossfade_initiated() {
        return Ok(None);
    }
    crossfade_next_from(&mobile.core, &mobile.audio).await
}

async fn crossfade_next_from(
    core: &StereodromeCore,
    audio: &AudioPlayer,
) -> Result<Option<QueueState>, String> {
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

    let prepared = prepare_queue_item_audio_from(core, next).await?;
    audio
        .crossfade_play(CrossfadePlayRequest {
            audio_data: prepared.audio_data,
            metadata: prepared.metadata,
            duration_secs: prepared.duration_secs,
            normalization_gain: prepared.processing.normalization_gain,
            dynamics_preset: prepared.processing.dynamics_preset,
            binaural_preset: prepared.processing.binaural_preset,
            equalizer_settings: prepared.processing.equalizer_settings,
            crossfade_duration_ms: settings.crossfade_duration_ms,
        })
        .map_err(|e| e.to_string())?;

    core.play_next(Some(false)).map_err(|e| e.to_string())?;
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
        duration_secs: item.duration as f64,
        processing,
    })
}

fn audio_processing_from_settings(
    settings: &AudioProcessingSettings,
) -> Result<AudioProcessing, String> {
    let normalization_gain = if settings.normalization_enabled || settings.preamp_db.abs() > 0.01 {
        Some(10.0_f32.powf(settings.preamp_db as f32 / 20.0))
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
                .map(|value| *value as f32)
                .collect(),
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

fn file_uri_to_path(value: &str) -> Result<PathBuf, String> {
    if value.starts_with("file://") {
        Url::parse(value)
            .map_err(|e| e.to_string())?
            .to_file_path()
            .map_err(|_| format!("invalid file URI: {value}"))
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
