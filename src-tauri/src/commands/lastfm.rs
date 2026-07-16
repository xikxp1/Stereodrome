use stereodrome_desktop::DesktopBackend;
use stereodrome_desktop::lastfm::{LastfmAuthStart, LastfmQueueItem, LastfmStatus};
use stereodrome_desktop::operations::settings::manual_offline_enabled;
use tauri::State;

use crate::error::{AppError, AppResult};

#[tauri::command]
pub fn get_lastfm_status(backend: State<'_, DesktopBackend>) -> LastfmStatus {
    stereodrome_desktop::lastfm::lastfm_status(backend.settings(), &backend.state())
}

#[tauri::command]
pub async fn begin_lastfm_auth(backend: State<'_, DesktopBackend>) -> AppResult<LastfmAuthStart> {
    if manual_offline_enabled(backend.settings()) {
        return Err(AppError::OfflineMode);
    }
    stereodrome_desktop::lastfm::begin_auth(backend.settings()).await
}

#[tauri::command]
pub async fn complete_lastfm_auth(backend: State<'_, DesktopBackend>) -> AppResult<LastfmStatus> {
    if manual_offline_enabled(backend.settings()) {
        return Err(AppError::OfflineMode);
    }
    stereodrome_desktop::lastfm::complete_auth(backend.settings(), &backend.state()).await
}

#[tauri::command]
pub fn disconnect_lastfm(backend: State<'_, DesktopBackend>) -> AppResult<LastfmStatus> {
    stereodrome_desktop::lastfm::disconnect(backend.settings(), &backend.state())
}

#[tauri::command]
pub fn get_lastfm_queue(backend: State<'_, DesktopBackend>) -> AppResult<Vec<LastfmQueueItem>> {
    stereodrome_desktop::lastfm::queue(&backend.state())
}

#[tauri::command]
pub async fn retry_lastfm_queue(backend: State<'_, DesktopBackend>) -> AppResult<usize> {
    if manual_offline_enabled(backend.settings()) {
        return Err(AppError::OfflineMode);
    }
    stereodrome_desktop::lastfm::retry_queue(backend.settings(), &backend.state()).await
}
