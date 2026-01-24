use log::warn;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::credentials;
use crate::error::{AppError, AppResult};
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

#[tauri::command]
pub async fn connect_server(
    state: State<'_, AppState>,
    params: ConnectParams,
) -> AppResult<ConnectionStatus> {
    // Connect via client handle (this delegates to the client thread)
    let result = state
        .client
        .connect(&params.url, &params.username, &params.password)
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    let config = ServerConfig {
        url: params.url.clone(),
        username: params.username.clone(),
        password: params.password.clone(),
    };

    // Persist credentials to OS keyring
    if let Err(e) = credentials::save_credentials(&config) {
        warn!("Failed to save credentials to keyring: {}", e);
    }

    Ok(ConnectionStatus {
        connected: true,
        server_url: Some(params.url),
        username: Some(params.username),
        server_version: Some(result.server_version),
    })
}

#[tauri::command]
pub async fn disconnect_server(state: State<'_, AppState>) -> AppResult<()> {
    // Disconnect via client handle
    state
        .client
        .disconnect()
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    // Clear credentials from OS keyring
    if let Err(e) = credentials::delete_credentials() {
        warn!("Failed to delete credentials from keyring: {}", e);
    }

    Ok(())
}

#[tauri::command]
pub async fn get_connection_status(state: State<'_, AppState>) -> AppResult<ConnectionStatus> {
    // Try to load credentials from keyring to get url/username
    let creds = credentials::load_credentials()?;

    match creds {
        Some(config) => Ok(ConnectionStatus {
            connected: state.is_connected(),
            server_url: Some(config.url),
            username: Some(config.username),
            server_version: None,
        }),
        None => Ok(ConnectionStatus {
            connected: false,
            server_url: None,
            username: None,
            server_version: None,
        }),
    }
}

/// Attempt to restore session from stored credentials
#[tauri::command]
pub async fn restore_session(state: State<'_, AppState>) -> AppResult<ConnectionStatus> {
    // Check if already connected
    if state.is_connected() {
        // Load credentials to get url/username
        if let Ok(Some(config)) = credentials::load_credentials() {
            return Ok(ConnectionStatus {
                connected: true,
                server_url: Some(config.url),
                username: Some(config.username),
                server_version: None,
            });
        }
    }

    // Try to load credentials from keyring
    match credentials::load_credentials()? {
        Some(config) => {
            let url = config.url.clone();
            let username = config.username.clone();

            // Attempt to reconnect via client handle
            match state
                .client
                .connect(&config.url, &config.username, &config.password)
                .await
            {
                Ok(result) => Ok(ConnectionStatus {
                    connected: true,
                    server_url: Some(url),
                    username: Some(username),
                    server_version: Some(result.server_version),
                }),
                Err(_) => {
                    // Connection failed, return disconnected status
                    Ok(ConnectionStatus {
                        connected: false,
                        server_url: Some(url),
                        username: Some(username),
                        server_version: None,
                    })
                }
            }
        }
        None => {
            // No stored credentials
            Ok(ConnectionStatus {
                connected: false,
                server_url: None,
                username: None,
                server_version: None,
            })
        }
    }
}
