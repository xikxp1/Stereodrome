use log::warn;
use serde::{Deserialize, Serialize};

use crate::JsonStore;
use crate::credentials;
use crate::error::{AppError, AppResult};
use crate::operations::settings::manual_offline_enabled;
use crate::state::{DesktopState, ServerConfig};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

pub async fn connect_server(
    settings: &JsonStore,
    state: &DesktopState,
    params: ConnectParams,
) -> AppResult<ConnectionStatus> {
    if manual_offline_enabled(settings) {
        return Err(AppError::OfflineMode);
    }

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

pub async fn disconnect_server(state: &DesktopState) -> AppResult<()> {
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

pub fn get_connection_status(
    settings: &JsonStore,
    state: &DesktopState,
) -> AppResult<ConnectionStatus> {
    // Try to load credentials from keyring to get url/username
    let creds = credentials::load_credentials()?;

    match creds {
        Some(config) => Ok(ConnectionStatus {
            connected: state.is_connected() && !manual_offline_enabled(settings),
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
pub async fn restore_session(
    settings: &JsonStore,
    state: &DesktopState,
) -> AppResult<ConnectionStatus> {
    if manual_offline_enabled(settings) {
        return match credentials::load_credentials()? {
            Some(config) => Ok(ConnectionStatus {
                connected: false,
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
        };
    }

    // Check if already connected - verify with ping
    if state.is_connected()
        && let Ok(Some(config)) = credentials::load_credentials()
    {
        // Ping to verify connection is still valid and get server version
        match state.client.ping().await {
            Ok(server_version) => {
                return Ok(ConnectionStatus {
                    connected: true,
                    server_url: Some(config.url),
                    username: Some(config.username),
                    server_version: Some(server_version),
                });
            }
            Err(_) => {
                // Connection is stale, will try to reconnect below
            }
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
