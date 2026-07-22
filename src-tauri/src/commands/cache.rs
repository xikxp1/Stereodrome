use stereodrome_core::{CacheStats, CoreCommand};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_store::StoreExt;

use crate::cache::{
    CacheLocationInfo, CacheRootUpdateResult, cache_location_info,
    set_cache_root as persist_cache_root,
};
use crate::error::AppResult;
use crate::runtime::{dispatch, dispatch_async, dispatch_unit_async, snapshot};
use crate::state::AppState;

const STORE_FILE: &str = "settings.json";
const KEY_MAX_CACHE_SIZE: &str = "max_cache_size";

pub(crate) fn migrate_desktop_cache_settings(
    app_handle: &AppHandle,
    state: &AppState,
) -> AppResult<()> {
    let Ok(store) = app_handle.store(STORE_FILE) else {
        return Ok(());
    };
    let Some(size) = store
        .get(KEY_MAX_CACHE_SIZE)
        .and_then(|value| value.as_u64())
    else {
        return Ok(());
    };
    dispatch::<CacheStats>(state, CoreCommand::SetMaxCacheSize { max_size: size })?;
    store.delete(KEY_MAX_CACHE_SIZE);
    let _ = store.save();
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_cache_locations(app_handle: AppHandle) -> AppResult<CacheLocationInfo> {
    cache_location_info(&app_handle)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_cache_root(
    app_handle: AppHandle,
    cache_root: Option<String>,
) -> AppResult<CacheRootUpdateResult> {
    if cache_root.is_some() {
        return Err(crate::error::AppError::Runtime(
            "custom cache roots are not supported by the shared runtime".to_string(),
        ));
    }
    let result = persist_cache_root(&app_handle, cache_root)?;
    let _ = app_handle.emit(
        "audio-cache-changed",
        serde_json::json!({ "reason": "location_changed" }),
    );
    Ok(result)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_audio_cache_stats(state: State<'_, AppState>) -> AppResult<CacheStats> {
    dispatch(&state, CoreCommand::GetAudioCacheStats)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_offline_song_ids(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    dispatch(&state, CoreCommand::GetOfflineSongIds)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_downloading_song_ids(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    Ok(snapshot(&state)?.downloads.downloading_song_ids)
}

#[tauri::command]
pub async fn clear_audio_cache(state: State<'_, AppState>) -> AppResult<()> {
    dispatch_unit_async(&state, CoreCommand::ClearAudioCache).await
}

#[tauri::command]
pub async fn set_max_cache_size(state: State<'_, AppState>, size: u64) -> AppResult<CacheStats> {
    dispatch_async(&state, CoreCommand::SetMaxCacheSize { max_size: size }).await
}
