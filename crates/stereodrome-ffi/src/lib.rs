//! Mobile FFI boundary for Stereodrome.
//!
//! The first mobile implementation uses JSON-over-FFI so the Swift/Kotlin Expo
//! module can remain thin while the Rust API stabilizes. The crate is isolated
//! so a UniFFI surface can be generated here without touching the desktop
//! Tauri adapter.

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use serde::Deserialize;
use serde_json::Value;
use stereodrome_core::queue::{QueueItem, RepeatMode};
use stereodrome_core::{ConnectParams, PlaybackProgress, StereodromeCore};

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }

    unsafe {
        let _ = CString::from_raw(value);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_new(data_dir: *const c_char) -> *mut StereodromeCore {
    let Some(data_dir) = read_c_string(data_dir) else {
        return ptr::null_mut();
    };

    match StereodromeCore::new(data_dir) {
        Ok(core) => Box::into_raw(Box::new(core)),
        Err(_) => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_destroy(core: *mut StereodromeCore) {
    if core.is_null() {
        return;
    }

    unsafe {
        let _ = Box::from_raw(core);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_get_connection_status(
    core: *mut StereodromeCore,
) -> *mut c_char {
    let Some(core) = core_ref(core) else {
        return json_error("core is not initialized");
    };

    match core.get_connection_status() {
        Ok(status) => json_ok(status),
        Err(error) => json_error(&error.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_get_stream_uri(
    core: *mut StereodromeCore,
    song_id: *const c_char,
) -> *mut c_char {
    let Some(core) = core_ref(core) else {
        return json_error("core is not initialized");
    };
    let Some(song_id) = read_c_string(song_id) else {
        return json_error("song_id is required");
    };

    match core.get_stream_uri(song_id) {
        Ok(uri) => json_ok(uri),
        Err(error) => json_error(&error.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_core_call(
    core: *mut StereodromeCore,
    method: *const c_char,
    payload: *const c_char,
) -> *mut c_char {
    let Some(core) = core_ref(core) else {
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

    match dispatch(core, &method, payload) {
        Ok(value) => into_c_string(value),
        Err(error) => json_error(&error),
    }
}

fn dispatch(core: &StereodromeCore, method: &str, payload: Value) -> Result<String, String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;

    match method {
        "connectServer" => {
            let params = parse_payload::<ConnectParams>(payload)?;
            json_result(runtime.block_on(async { core.connect_server(params).await }))
        }
        "restoreSession" => json_result(runtime.block_on(async { core.restore_session().await })),
        "disconnectServer" => {
            json_result(runtime.block_on(async { core.disconnect_server().await }))
        }
        "getConnectionStatus" => json_result(core.get_connection_status()),
        "syncLibrary" => json_result(runtime.block_on(async { core.sync_library().await })),
        "syncLibraryIncremental" => {
            json_result(runtime.block_on(async { core.sync_library_incremental().await }))
        }
        "reconcileLibrary" => {
            json_result(runtime.block_on(async { core.reconcile_library().await }))
        }
        "getLibrarySyncStatus" => json_result(core.get_library_sync_status()),
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
            json_result(core.get_cover_art_uri(args.id, args.size))
        }
        "getSongCoverArtUri" => {
            let args = parse_payload::<IdSizePayload>(payload)?;
            json_result(core.get_song_cover_art_uri(args.id, args.size))
        }
        "getStreamUri" => {
            let song_id = parse_payload::<String>(payload)?;
            json_result(core.get_stream_uri(song_id))
        }
        "getAudioCacheStats" => json_result(core.get_audio_cache_stats()),
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

fn core_ref<'a>(core: *mut StereodromeCore) -> Option<&'a StereodromeCore> {
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
