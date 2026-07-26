use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use log::warn;
use stereodrome_core::{
    Album, AlbumListEntry, Artist, CoreCommand, LibrarySyncStatus, ScanStatus, Song, SyncKind,
    SyncResult,
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppResult;
use crate::runtime::{dispatch, dispatch_async, dispatch_detached_and_wait};
use crate::state::AppState;

const SYNC_SCHEDULER_POLL_INTERVAL: Duration = Duration::from_mins(1);

pub fn start_library_sync_scheduler(app_handle: &AppHandle) {
    let Some(state) = app_handle.try_state::<AppState>() else {
        return;
    };
    let runtime = state.runtime.clone();
    let running = std::sync::Arc::clone(&state.emitter_running);
    let app_handle = app_handle.clone();
    thread::spawn(move || {
        while running.load(Ordering::SeqCst) {
            thread::sleep(SYNC_SCHEDULER_POLL_INTERVAL);
            if !running.load(Ordering::SeqCst) {
                break;
            }
            let result = runtime.dispatch_command(CoreCommand::RunDueLibrarySync);
            match crate::runtime::deserialize_result::<Option<String>>(result) {
                Ok(Some(job)) => emit_library_content_updated(&app_handle, &job, None),
                Ok(None) => {}
                Err(error) => warn!("Runtime library scheduler tick failed: {error}"),
            }
        }
    });
}

fn emit_library_content_updated(app_handle: &AppHandle, job: &str, result: Option<&SyncResult>) {
    let artists = result.map_or(0, |result| result.artists);
    let albums = result.map_or(0, |result| result.albums);
    let songs = result.map_or(0, |result| result.songs);
    let _ = app_handle.emit(
        "library-content-updated",
        serde_json::json!({
            "job": job,
            "new_artists": artists,
            "new_albums": albums,
            "new_songs": songs,
            // Scheduled sync currently returns only the completed job kind, so
            // conservatively invalidate library views after it runs.
            "has_new_items": result.is_none() || artists > 0 || albums > 0 || songs > 0,
        }),
    );
}

#[tauri::command]
pub async fn sync_library(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<SyncResult> {
    dispatch_detached_and_wait(
        &state,
        CoreCommand::StartSync {
            kind: SyncKind::Incremental,
        },
    )
    .await?;
    let result = SyncResult::default();
    emit_library_content_updated(&app_handle, "incremental", Some(&result));
    Ok(result)
}

#[tauri::command]
pub async fn reconcile_library_state(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<SyncResult> {
    dispatch_detached_and_wait(
        &state,
        CoreCommand::StartSync {
            kind: SyncKind::FullReconcile,
        },
    )
    .await?;
    let result = SyncResult::default();
    emit_library_content_updated(&app_handle, "full_reconcile", Some(&result));
    Ok(result)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_library_sync_status(state: State<'_, AppState>) -> AppResult<LibrarySyncStatus> {
    dispatch(&state, CoreCommand::GetLibrarySyncStatus)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_artists(state: State<'_, AppState>) -> AppResult<Vec<Artist>> {
    dispatch(&state, CoreCommand::GetArtists)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_album_count(state: State<'_, AppState>) -> AppResult<i64> {
    let albums: Vec<Album> = dispatch(&state, CoreCommand::GetAlbums { artist_id: None })?;
    i64::try_from(albums.len()).map_err(|error| crate::error::AppError::Runtime(error.to_string()))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_albums(state: State<'_, AppState>, artist_id: Option<String>) -> AppResult<Vec<Album>> {
    dispatch(&state, CoreCommand::GetAlbums { artist_id })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_songs(
    state: State<'_, AppState>,
    album_id: Option<String>,
    artist_id: Option<String>,
) -> AppResult<Vec<Song>> {
    dispatch(
        &state,
        CoreCommand::GetSongs {
            album_id,
            artist_id,
        },
    )
}

#[tauri::command]
pub async fn get_album_list(
    state: State<'_, AppState>,
    list_type: String,
    size: Option<u32>,
    offset: Option<u32>,
) -> AppResult<Vec<AlbumListEntry>> {
    dispatch_async(
        &state,
        CoreCommand::GetAlbumList {
            list_type,
            size: size.map(|value| value as usize),
            offset: offset.map(|value| value as usize),
        },
    )
    .await
}

#[tauri::command]
pub async fn get_scan_status(state: State<'_, AppState>) -> AppResult<ScanStatus> {
    dispatch_async(&state, CoreCommand::GetScanStatus).await
}

#[tauri::command]
pub async fn start_scan(state: State<'_, AppState>) -> AppResult<ScanStatus> {
    dispatch_async(&state, CoreCommand::StartScan).await
}
