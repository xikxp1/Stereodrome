use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::lastfm::{LastfmAuthStart, LastfmQueueItem, LastfmStatus};
use crate::state::AppState;

#[tauri::command]
pub fn get_lastfm_status(app_handle: AppHandle, state: State<'_, AppState>) -> LastfmStatus {
    crate::lastfm::lastfm_status(&app_handle, state.inner())
}

#[tauri::command]
pub async fn begin_lastfm_auth(app_handle: AppHandle) -> AppResult<LastfmAuthStart> {
    crate::lastfm::begin_auth(&app_handle).await
}

#[tauri::command]
pub async fn complete_lastfm_auth(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<LastfmStatus> {
    crate::lastfm::complete_auth(&app_handle, state.inner()).await
}

#[tauri::command]
pub fn disconnect_lastfm(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<LastfmStatus> {
    crate::lastfm::disconnect(&app_handle, state.inner())
}

#[tauri::command]
pub fn get_lastfm_queue(state: State<'_, AppState>) -> AppResult<Vec<LastfmQueueItem>> {
    crate::lastfm::queue(state.inner())
}

#[tauri::command]
pub async fn retry_lastfm_queue(app_handle: AppHandle) -> AppResult<usize> {
    crate::lastfm::retry_queue(&app_handle).await
}
