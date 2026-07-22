use log::warn;
use serde::{Deserialize, Serialize};
use stereodrome_core::CoreCommand;
use tauri::State;

use crate::credentials;
use crate::error::AppResult;
use crate::runtime::{dispatch, dispatch_async, dispatch_unit_async};
use crate::state::{AppState, ServerConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    pub server_url: Option<String>,
    pub username: Option<String>,
    pub server_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectParams {
    pub url: String,
    pub username: String,
    pub password: String,
}

impl From<stereodrome_core::ConnectionStatus> for ConnectionStatus {
    fn from(status: stereodrome_core::ConnectionStatus) -> Self {
        Self {
            connected: status.connected,
            server_url: status.server_url,
            username: status.username,
            server_version: status.server_version,
        }
    }
}

#[tauri::command]
pub async fn connect_server(
    state: State<'_, AppState>,
    params: ConnectParams,
) -> AppResult<ConnectionStatus> {
    let status: stereodrome_core::ConnectionStatus = dispatch_async(
        &state,
        CoreCommand::Connect {
            params: stereodrome_core::ConnectParams {
                url: params.url.clone(),
                username: params.username.clone(),
                password: params.password.clone(),
            },
        },
    )
    .await?;

    if let Err(error) = credentials::save_credentials(&ServerConfig {
        url: params.url,
        username: params.username,
        password: params.password,
    }) {
        warn!("Failed to mirror runtime credentials to the desktop keyring: {error}");
    }
    Ok(status.into())
}

#[tauri::command]
pub async fn disconnect_server(state: State<'_, AppState>) -> AppResult<()> {
    dispatch_unit_async(&state, CoreCommand::Disconnect).await?;
    if let Err(error) = credentials::delete_credentials() {
        warn!("Failed to clear mirrored desktop credentials: {error}");
    }
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_connection_status(state: State<'_, AppState>) -> AppResult<ConnectionStatus> {
    dispatch::<stereodrome_core::ConnectionStatus>(&state, CoreCommand::GetConnectionStatus)
        .map(Into::into)
}

#[tauri::command]
pub async fn restore_session(state: State<'_, AppState>) -> AppResult<ConnectionStatus> {
    let mut status: stereodrome_core::ConnectionStatus =
        dispatch_async(&state, CoreCommand::RestoreSession).await?;

    if !status.connected
        && let Some(config) = credentials::load_credentials()?
    {
        let params = ConnectParams {
            url: config.url,
            username: config.username,
            password: config.password,
        };
        status = dispatch_async(
            &state,
            CoreCommand::Connect {
                params: stereodrome_core::ConnectParams {
                    url: params.url.clone(),
                    username: params.username.clone(),
                    password: params.password.clone(),
                },
            },
        )
        .await?;
    }

    Ok(status.into())
}
