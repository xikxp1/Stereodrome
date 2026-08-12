use stereodrome_core::{CacheStats, CoreCommand};
use tauri::{AppHandle, State};
use tauri_plugin_store::StoreExt;

use crate::cache::{CacheLocationInfo, cache_location_info};
use crate::error::AppResult;
use crate::runtime::{dispatch, snapshot};
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
pub fn get_downloading_song_ids(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    Ok(snapshot(&state)?.downloads.downloading_song_ids)
}
