use stereodrome_desktop::DesktopBackend;
use tauri::State;

use crate::error::AppResult;

pub use stereodrome_desktop::cache::{CacheLocationInfo, CacheRootUpdateResult, CacheStats};

#[tauri::command]
pub fn get_cache_locations(backend: State<'_, DesktopBackend>) -> AppResult<CacheLocationInfo> {
    stereodrome_desktop::operations::cache::get_cache_locations(&backend.state())
}

#[tauri::command]
pub fn set_cache_root(
    backend: State<'_, DesktopBackend>,
    cache_root: Option<String>,
) -> AppResult<CacheRootUpdateResult> {
    stereodrome_desktop::operations::cache::set_cache_root(&backend.state(), cache_root)
}

#[tauri::command]
pub async fn get_audio_cache_stats(backend: State<'_, DesktopBackend>) -> AppResult<CacheStats> {
    stereodrome_desktop::operations::cache::get_audio_cache_stats(backend.state())
}

#[tauri::command]
pub fn get_offline_song_ids(backend: State<'_, DesktopBackend>) -> AppResult<Vec<String>> {
    stereodrome_desktop::operations::cache::get_offline_song_ids(backend.state())
}

#[tauri::command]
pub async fn clear_audio_cache(backend: State<'_, DesktopBackend>) -> AppResult<()> {
    stereodrome_desktop::operations::cache::clear_audio_cache(backend.state())
}

#[tauri::command]
pub async fn set_max_cache_size(
    backend: State<'_, DesktopBackend>,
    size: u64,
) -> AppResult<CacheStats> {
    stereodrome_desktop::operations::cache::set_max_cache_size(backend.state(), size)
}
