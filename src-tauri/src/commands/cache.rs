use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::cache::{AudioCache, CacheStats, DEFAULT_MAX_CACHE_SIZE};
use crate::error::AppResult;

const STORE_FILE: &str = "settings.json";
const KEY_MAX_CACHE_SIZE: &str = "max_cache_size";

/// Minimum cache size: 500 MB
pub const MIN_CACHE_SIZE: u64 = 500 * 1024 * 1024;
/// Maximum cache size: 50 GB
pub const MAX_CACHE_SIZE: u64 = 50 * 1024 * 1024 * 1024;

/// Get the configured max cache size from settings
fn get_max_cache_size(app_handle: &AppHandle) -> u64 {
    if let Ok(store) = app_handle.store(STORE_FILE) {
        if let Some(value) = store.get(KEY_MAX_CACHE_SIZE) {
            if let Some(size) = value.as_u64() {
                return size.clamp(MIN_CACHE_SIZE, MAX_CACHE_SIZE);
            }
        }
    }
    DEFAULT_MAX_CACHE_SIZE
}

/// Get statistics about the audio cache
#[tauri::command]
pub async fn get_audio_cache_stats(app_handle: AppHandle) -> AppResult<CacheStats> {
    let max_size = get_max_cache_size(&app_handle);
    let cache = AudioCache::new(&app_handle, max_size)?;
    cache.get_stats()
}

/// Clear all cached audio files
#[tauri::command]
pub async fn clear_audio_cache(app_handle: AppHandle) -> AppResult<()> {
    let max_size = get_max_cache_size(&app_handle);
    let cache = AudioCache::new(&app_handle, max_size)?;
    cache.clear()
}

/// Set the maximum cache size (in bytes)
#[tauri::command]
pub async fn set_max_cache_size(app_handle: AppHandle, size: u64) -> AppResult<CacheStats> {
    let size = size.clamp(MIN_CACHE_SIZE, MAX_CACHE_SIZE);

    // Save to store
    if let Ok(store) = app_handle.store(STORE_FILE) {
        store.set(KEY_MAX_CACHE_SIZE, serde_json::json!(size));
        let _ = store.save();
    }

    // Enforce new limit (may trigger eviction)
    let cache = AudioCache::new(&app_handle, size)?;
    cache.enforce_size_limit()?;

    // Return updated stats
    cache.get_stats()
}
