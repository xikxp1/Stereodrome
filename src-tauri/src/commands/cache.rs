use tauri::AppHandle;

use crate::cache::{AudioCache, CacheStats, DEFAULT_MAX_CACHE_SIZE};
use crate::error::AppResult;

/// Get statistics about the audio cache
#[tauri::command]
pub async fn get_audio_cache_stats(app_handle: AppHandle) -> AppResult<CacheStats> {
    let cache = AudioCache::new(&app_handle, DEFAULT_MAX_CACHE_SIZE)?;
    cache.get_stats()
}

/// Clear all cached audio files
#[tauri::command]
pub async fn clear_audio_cache(app_handle: AppHandle) -> AppResult<()> {
    let cache = AudioCache::new(&app_handle, DEFAULT_MAX_CACHE_SIZE)?;
    cache.clear()
}
