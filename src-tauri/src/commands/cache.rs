use stereodrome_desktop::DesktopBackend;
use tauri::{AppHandle, Manager, State};

use crate::cache::{
    AudioCache, CacheLocationInfo, CacheRootUpdateResult, CacheStats, KEY_MAX_CACHE_SIZE,
    MAX_CACHE_SIZE, MIN_CACHE_SIZE, cache_location_info, emit_audio_cache_changed,
    set_cache_root as persist_cache_root,
};
use crate::error::{AppResult, MutexExt};
use crate::state::AppState;

/// Get configured cache locations.
#[tauri::command]
pub fn get_cache_locations(app_handle: AppHandle) -> AppResult<CacheLocationInfo> {
    cache_location_info(&app_handle)
}

/// Set the cache root and move existing cache files into the new root.
#[tauri::command]
pub fn set_cache_root(
    app_handle: AppHandle,
    cache_root: Option<String>,
) -> AppResult<CacheRootUpdateResult> {
    let result = persist_cache_root(&app_handle, cache_root)?;
    emit_audio_cache_changed(&app_handle, "location_changed");
    Ok(result)
}

/// Get statistics about the audio cache
#[tauri::command]
pub async fn get_audio_cache_stats(app_handle: AppHandle) -> AppResult<CacheStats> {
    let cache = AudioCache::new(&app_handle)?;
    cache.get_stats()
}

/// Get locally cached song IDs that can be played without a server connection.
#[tauri::command]
pub fn get_offline_song_ids(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
    let cache = AudioCache::new(&app_handle)?;
    let db = state.db.lock_recover();
    let mut stmt = db.prepare("SELECT id, suffix FROM songs")?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        ))
    })?;

    let mut song_ids = Vec::new();
    for row in rows {
        let (song_id, suffix) = row?;
        if cache.is_cached(&song_id, &suffix) {
            song_ids.push(song_id);
        }
    }

    Ok(song_ids)
}

/// Clear all cached audio files
#[tauri::command]
pub async fn clear_audio_cache(app_handle: AppHandle) -> AppResult<()> {
    let cache = AudioCache::new(&app_handle)?;
    cache.clear()
}

/// Set the maximum cache size (in bytes)
#[tauri::command]
pub async fn set_max_cache_size(app_handle: AppHandle, size: u64) -> AppResult<CacheStats> {
    let size = size.clamp(MIN_CACHE_SIZE, MAX_CACHE_SIZE);

    app_handle
        .state::<DesktopBackend>()
        .settings()
        .set(KEY_MAX_CACHE_SIZE, size)?;

    // Enforce new limit (AudioCache::new reads the size we just saved to the store)
    let cache = AudioCache::new(&app_handle)?;
    let evicted = cache.enforce_size_limit()?;
    if evicted {
        cache.emit_changed("evicted");
    }

    // Return updated stats
    cache.get_stats()
}
