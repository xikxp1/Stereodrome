use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::cache::{
    AudioCache, CacheStats, KEY_MAX_CACHE_SIZE, MAX_CACHE_SIZE, MIN_CACHE_SIZE, STORE_FILE,
};
use crate::error::AppResult;

/// Get statistics about the audio cache
#[tauri::command]
pub async fn get_audio_cache_stats(app_handle: AppHandle) -> AppResult<CacheStats> {
    let cache = AudioCache::new(&app_handle)?;
    cache.get_stats()
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

    // Save to store
    if let Ok(store) = app_handle.store(STORE_FILE) {
        store.set(KEY_MAX_CACHE_SIZE, serde_json::json!(size));
        let _ = store.save();
    }

    // Enforce new limit (AudioCache::new reads the size we just saved to the store)
    let cache = AudioCache::new(&app_handle)?;
    cache.enforce_size_limit()?;

    // Return updated stats
    cache.get_stats()
}
