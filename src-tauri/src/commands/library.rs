use stereodrome_desktop::DesktopBackend;
use stereodrome_desktop::operations::library::{
    Album, Artist, LibrarySyncStatus, ScanStatus, Song, SyncResult,
};
use tauri::State;

use crate::error::AppResult;

#[tauri::command]
pub async fn sync_library(backend: State<'_, DesktopBackend>) -> AppResult<SyncResult> {
    stereodrome_desktop::operations::library::sync_library(&backend.state()).await
}

#[tauri::command]
pub async fn reconcile_library_state(backend: State<'_, DesktopBackend>) -> AppResult<SyncResult> {
    stereodrome_desktop::operations::library::reconcile_library_state(&backend.state()).await
}

#[tauri::command]
pub fn get_library_sync_status(backend: State<'_, DesktopBackend>) -> AppResult<LibrarySyncStatus> {
    stereodrome_desktop::operations::library::get_library_sync_status(&backend.state())
}

#[tauri::command]
pub async fn get_artists(backend: State<'_, DesktopBackend>) -> AppResult<Vec<Artist>> {
    stereodrome_desktop::operations::library::get_artists(&backend.state())
}

#[tauri::command]
pub fn get_album_count(backend: State<'_, DesktopBackend>) -> AppResult<i64> {
    stereodrome_desktop::operations::library::get_album_count(&backend.state())
}

#[tauri::command]
pub async fn get_albums(
    backend: State<'_, DesktopBackend>,
    artist_id: Option<String>,
) -> AppResult<Vec<Album>> {
    stereodrome_desktop::operations::library::get_albums(&backend.state(), artist_id)
}

#[tauri::command]
pub async fn get_songs(
    backend: State<'_, DesktopBackend>,
    album_id: Option<String>,
    artist_id: Option<String>,
) -> AppResult<Vec<Song>> {
    stereodrome_desktop::operations::library::get_songs(&backend.state(), album_id, artist_id)
}

#[tauri::command]
pub async fn get_album_list(
    backend: State<'_, DesktopBackend>,
    list_type: String,
    size: Option<u32>,
    offset: Option<u32>,
) -> AppResult<Vec<stereodrome_desktop::client::AlbumListEntry>> {
    stereodrome_desktop::operations::library::get_album_list(
        &backend.state(),
        list_type,
        size,
        offset,
    )
    .await
}

#[tauri::command]
pub async fn get_scan_status(backend: State<'_, DesktopBackend>) -> AppResult<ScanStatus> {
    stereodrome_desktop::operations::library::get_scan_status(&backend.state()).await
}

#[tauri::command]
pub async fn start_scan(backend: State<'_, DesktopBackend>) -> AppResult<ScanStatus> {
    stereodrome_desktop::operations::library::start_scan(&backend.state()).await
}
