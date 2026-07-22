use stereodrome_core::{CoreCommand, LastfmAuthStart, LastfmQueueItem, LastfmStatus};
use tauri::State;

use crate::error::AppResult;
use crate::runtime::{dispatch, dispatch_async};
use crate::state::AppState;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_lastfm_status(state: State<'_, AppState>) -> AppResult<LastfmStatus> {
    dispatch(&state, CoreCommand::GetLastfmStatus)
}

#[tauri::command]
pub async fn begin_lastfm_auth(state: State<'_, AppState>) -> AppResult<LastfmAuthStart> {
    dispatch_async(&state, CoreCommand::BeginLastfmAuth).await
}

#[tauri::command]
pub async fn complete_lastfm_auth(state: State<'_, AppState>) -> AppResult<LastfmStatus> {
    dispatch_async(&state, CoreCommand::CompleteLastfmAuth).await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn disconnect_lastfm(state: State<'_, AppState>) -> AppResult<LastfmStatus> {
    dispatch(&state, CoreCommand::DisconnectLastfm)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_lastfm_queue(state: State<'_, AppState>) -> AppResult<Vec<LastfmQueueItem>> {
    dispatch(&state, CoreCommand::GetLastfmQueue)
}

#[tauri::command]
pub async fn retry_lastfm_queue(state: State<'_, AppState>) -> AppResult<usize> {
    dispatch_async(&state, CoreCommand::RetryLastfmQueue).await
}
