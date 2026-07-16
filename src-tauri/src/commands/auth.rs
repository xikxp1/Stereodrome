use stereodrome_desktop::DesktopBackend;
use tauri::State;

use crate::error::AppResult;

pub use stereodrome_desktop::operations::auth::{ConnectParams, ConnectionStatus};

#[tauri::command]
pub async fn connect_server(
    backend: State<'_, DesktopBackend>,
    params: ConnectParams,
) -> AppResult<ConnectionStatus> {
    let state = backend.state();
    stereodrome_desktop::operations::auth::connect_server(backend.settings(), &state, params).await
}

#[tauri::command]
pub async fn disconnect_server(backend: State<'_, DesktopBackend>) -> AppResult<()> {
    let state = backend.state();
    stereodrome_desktop::operations::auth::disconnect_server(&state).await
}

#[tauri::command]
pub fn get_connection_status(backend: State<'_, DesktopBackend>) -> AppResult<ConnectionStatus> {
    let state = backend.state();
    stereodrome_desktop::operations::auth::get_connection_status(backend.settings(), &state)
}

#[tauri::command]
pub async fn restore_session(backend: State<'_, DesktopBackend>) -> AppResult<ConnectionStatus> {
    let state = backend.state();
    stereodrome_desktop::operations::auth::restore_session(backend.settings(), &state).await
}
