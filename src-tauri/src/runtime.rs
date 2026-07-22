use std::path::PathBuf;
use std::thread;

use serde::de::DeserializeOwned;
use stereodrome_core::{
    CommandStatus, CoreCommand, CoreCommandResult, CoreEventKind, CoreSnapshot, PlaybackProjection,
    ProtocolError, ProtocolErrorCode, StereodromeRuntimeHandle,
};
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::SongMetadata;
use crate::error::{AppError, AppResult};
use crate::media::MediaControlsManager;
use crate::state::AppState;
use crate::tray::TrayManager;

pub fn dispatch<T: DeserializeOwned>(state: &AppState, command: CoreCommand) -> AppResult<T> {
    deserialize_result(state.runtime.dispatch_command(command))
}

pub fn dispatch_unit(state: &AppState, command: CoreCommand) -> AppResult<()> {
    successful_value(state.runtime.dispatch_command(command)).map(drop)
}

pub async fn dispatch_async<T>(state: &AppState, command: CoreCommand) -> AppResult<T>
where
    T: DeserializeOwned + Send + 'static,
{
    let runtime = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || {
        deserialize_result(runtime.dispatch_command(command))
    })
    .await
    .map_err(|error| AppError::Runtime(error.to_string()))?
}

pub async fn dispatch_unit_async(state: &AppState, command: CoreCommand) -> AppResult<()> {
    let runtime = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || {
        successful_value(runtime.dispatch_command(command)).map(drop)
    })
    .await
    .map_err(|error| AppError::Runtime(error.to_string()))?
}

/// Dispatches a runtime-owned background operation and waits for its authoritative
/// completion event. Detached commands acknowledge with a null value, so desktop
/// adapters that historically waited for completion must not deserialize that
/// acknowledgement as the operation's eventual result.
pub async fn dispatch_detached_and_wait(state: &AppState, command: CoreCommand) -> AppResult<()> {
    let runtime = state.runtime.clone();
    let mut events = runtime.subscribe();
    let dispatch_runtime = runtime.clone();
    let accepted =
        tauri::async_runtime::spawn_blocking(move || dispatch_runtime.dispatch_command(command))
            .await
            .map_err(|error| AppError::Runtime(error.to_string()))?;
    let operation_id = accepted.operation_id;
    successful_value(accepted)?;
    let Some(operation_id) = operation_id else {
        return Ok(());
    };

    loop {
        match events.recv().await {
            Ok(event) => match event.kind {
                CoreEventKind::OperationFailed { failure }
                    if failure.operation_id == Some(operation_id) =>
                {
                    return Err(protocol_error(failure.error));
                }
                CoreEventKind::SnapshotChanged { snapshot }
                    if snapshot
                        .operations
                        .iter()
                        .all(|operation| operation.operation_id != operation_id) =>
                {
                    if let Some(failure) = snapshot
                        .last_failure
                        .filter(|failure| failure.operation_id == Some(operation_id))
                    {
                        return Err(protocol_error(failure.error));
                    }
                    return Ok(());
                }
                CoreEventKind::RuntimeShuttingDown => {
                    return Err(AppError::Runtime(
                        "runtime shut down while an operation was running".to_string(),
                    ));
                }
                _ => {}
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                let snapshot: CoreSnapshot = deserialize_result(runtime.snapshot())?;
                if snapshot
                    .operations
                    .iter()
                    .all(|operation| operation.operation_id != operation_id)
                {
                    if let Some(failure) = snapshot
                        .last_failure
                        .filter(|failure| failure.operation_id == Some(operation_id))
                    {
                        return Err(protocol_error(failure.error));
                    }
                    return Ok(());
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                return Err(AppError::Runtime(
                    "runtime event stream closed while an operation was running".to_string(),
                ));
            }
        }
    }
}

pub fn snapshot(state: &AppState) -> AppResult<CoreSnapshot> {
    deserialize_result(state.runtime.snapshot())
}

pub(crate) fn deserialize_result<T: DeserializeOwned>(result: CoreCommandResult) -> AppResult<T> {
    serde_json::from_value(successful_value(result)?)
        .map_err(|error| AppError::Runtime(format!("invalid runtime response: {error}")))
}

fn successful_value(result: CoreCommandResult) -> AppResult<serde_json::Value> {
    if result.status == CommandStatus::Failed {
        return Err(result.error.map_or_else(
            || AppError::Runtime("runtime command failed without an error".to_string()),
            protocol_error,
        ));
    }

    Ok(result.value.unwrap_or(serde_json::Value::Null))
}

fn protocol_error(error: ProtocolError) -> AppError {
    match error.code {
        ProtocolErrorCode::NotConnected => AppError::NotConnected,
        ProtocolErrorCode::OfflineMode => AppError::OfflineMode,
        ProtocolErrorCode::Audio => AppError::Audio(error.message),
        _ => AppError::Runtime(error.message),
    }
}

pub fn start_event_bridge(app_handle: AppHandle, runtime: StereodromeRuntimeHandle) {
    let initial_app = app_handle.clone();
    let initial_runtime = runtime.clone();
    thread::spawn(move || {
        let mut events = runtime.subscribe();
        let mut offline_song_ids = Vec::new();
        let mut analyzed_song_id = None;
        let mut projection_cache = ProjectionCache::default();
        if let Ok(snapshot) = deserialize_result::<CoreSnapshot>(initial_runtime.snapshot()) {
            offline_song_ids.clone_from(&snapshot.downloads.offline_song_ids);
            project_normalization_analysis(&initial_app, &mut analyzed_song_id, &snapshot);
            project_snapshot(&initial_app, &snapshot, &mut projection_cache);
        }
        let event_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create desktop runtime event bridge");

        event_runtime.block_on(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        let snapshot = match &event.kind {
                            CoreEventKind::SnapshotChanged { snapshot } => Some(snapshot.as_ref()),
                            _ => None,
                        };
                        let _ = app_handle.emit("core-event", &event);
                        if let Some(snapshot) = snapshot {
                            project_normalization_analysis(
                                &app_handle,
                                &mut analyzed_song_id,
                                snapshot,
                            );
                            project_cache_change(
                                &app_handle,
                                &mut offline_song_ids,
                                &snapshot.downloads.offline_song_ids,
                            );
                            project_snapshot(&app_handle, snapshot, &mut projection_cache);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if let Some(state) = app_handle.try_state::<AppState>()
                            && let Ok(snapshot) = snapshot(&state)
                        {
                            project_normalization_analysis(
                                &app_handle,
                                &mut analyzed_song_id,
                                &snapshot,
                            );
                            project_cache_change(
                                &app_handle,
                                &mut offline_song_ids,
                                &snapshot.downloads.offline_song_ids,
                            );
                            project_snapshot(&app_handle, &snapshot, &mut projection_cache);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
}

fn project_normalization_analysis(
    app_handle: &AppHandle,
    previous_song_id: &mut Option<String>,
    snapshot: &CoreSnapshot,
) {
    let song_id = snapshot.playback.song.as_ref().map(|song| song.id.clone());
    if *previous_song_id == song_id {
        return;
    }
    previous_song_id.clone_from(&song_id);
    let Some(song_id) = song_id else {
        return;
    };
    if let Some(state) = app_handle.try_state::<AppState>() {
        crate::commands::normalization::analyze_song_if_needed(
            state.runtime.clone(),
            state.db_path.clone(),
            song_id,
        );
    }
}

fn project_cache_change(app_handle: &AppHandle, previous: &mut Vec<String>, current: &[String]) {
    if previous.as_slice() == current {
        return;
    }
    previous.clear();
    previous.extend_from_slice(current);
    let _ = app_handle.emit(
        "audio-cache-changed",
        serde_json::json!({ "reason": "runtime_cache_changed" }),
    );
}

#[derive(Default)]
struct ProjectionCache {
    queue: Option<serde_json::Value>,
    sync: Option<serde_json::Value>,
}

fn project_snapshot(app_handle: &AppHandle, snapshot: &CoreSnapshot, cache: &mut ProjectionCache) {
    let _ = app_handle.emit("runtime-snapshot", snapshot);
    if let Ok(queue) = serde_json::to_value(&snapshot.queue)
        && cache.queue.as_ref() != Some(&queue)
    {
        cache.queue = Some(queue);
        let _ = app_handle.emit("queue-changed", &snapshot.queue);
    }
    if let Ok(sync) = serde_json::to_value(&snapshot.sync)
        && cache.sync.as_ref() != Some(&sync)
    {
        cache.sync = Some(sync);
        let _ = app_handle.emit("library-sync-status-changed", &snapshot.sync);
    }
    project_platform_playback(app_handle, &snapshot.playback);
}

fn project_platform_playback(app_handle: &AppHandle, playback: &PlaybackProjection) {
    let Some(song) = playback.song.as_ref() else {
        if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>() {
            media_controls.clear();
        }
        if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
            tray_manager.update_song_info("", "");
            tray_manager.update_playback_state(false);
        }
        return;
    };

    let metadata = SongMetadata {
        id: song.id.clone(),
        title: song.title.clone(),
        artist: song.artist.clone(),
        album: song.album.clone(),
        cover_art_id: None,
    };
    if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>() {
        media_controls.update_metadata(
            &metadata,
            playback.duration_seconds,
            song.artwork_uri
                .as_deref()
                .and_then(file_uri_path)
                .map(|path| path.to_string_lossy().into_owned()),
        );
        media_controls.set_playback_status(playback.is_playing, playback.position_seconds);
    }
    if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
        tray_manager.update_song_info(&song.title, &song.artist);
        tray_manager.update_playback_state(playback.is_playing);
    }
}

pub(crate) fn file_uri_path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}
