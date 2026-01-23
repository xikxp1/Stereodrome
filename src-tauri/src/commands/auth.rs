use log::warn;
use serde::{Deserialize, Serialize};
use submarine::{auth::AuthBuilder, Client};
use tauri::State;

use crate::credentials;
use crate::error::{AppError, AppResult, MutexExt};
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
    let auth = AuthBuilder::new(&params.username, "1.16.1")
        .client_name("Stereodrome")
        .hashed(&params.password);

    let client = Client::new(&params.url, auth);

    // Test connection with ping
    let ping_result = client
        .ping()
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    let config = ServerConfig {
        url: params.url.clone(),
        username: params.username.clone(),
        password: params.password.clone(),
    };

    // Store client and config in memory
    {
        let mut client_lock = state.client.lock_recover();
        *client_lock = Some(client);
    }

    {
        let mut config_lock = state.server_config.lock_recover();
        *config_lock = Some(config.clone());
    }

    // Persist credentials to OS keyring
    if let Err(e) = credentials::save_credentials(&config) {
        warn!("Failed to save credentials to keyring: {}", e);
    }

    Ok(ConnectionStatus {
        connected: true,
        server_url: Some(params.url),
        username: Some(params.username),
        server_version: Some(ping_result.version),
    })
}

#[tauri::command]
pub async fn disconnect_server(state: State<'_, AppState>) -> AppResult<()> {
    {
        let mut client_lock = state.client.lock_recover();
        *client_lock = None;
    }

    {
        let mut config_lock = state.server_config.lock_recover();
        *config_lock = None;
    }

    // Clear credentials from OS keyring
    if let Err(e) = credentials::delete_credentials() {
        warn!("Failed to delete credentials from keyring: {}", e);
    }

    Ok(())
}

#[tauri::command]
pub async fn get_connection_status(state: State<'_, AppState>) -> AppResult<ConnectionStatus> {
    let config = state.server_config.lock_recover();

    match config.as_ref() {
        Some(cfg) => Ok(ConnectionStatus {
            connected: state.is_connected(),
            server_url: Some(cfg.url.clone()),
            username: Some(cfg.username.clone()),
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
        let config = state.server_config.lock_recover();
        if let Some(cfg) = config.as_ref() {
            return Ok(ConnectionStatus {
                connected: true,
                server_url: Some(cfg.url.clone()),
                username: Some(cfg.username.clone()),
                server_version: None,
            });
        }
    }

    // Try to load credentials from keyring
    match credentials::load_credentials()? {
        Some(config) => {
            let url = config.url.clone();
            let username = config.username.clone();

            // Attempt to reconnect
            let auth = AuthBuilder::new(&config.username, "1.16.1")
                .client_name("Stereodrome")
                .hashed(&config.password);

            let client = Client::new(&config.url, auth);

            // Test connection
            match client.ping().await {
                Ok(ping_result) => {
                    // Store client and config
                    {
                        let mut client_lock = state.client.lock_recover();
                        *client_lock = Some(client);
                    }
                    {
                        let mut config_lock = state.server_config.lock_recover();
                        *config_lock = Some(config);
                    }

                    Ok(ConnectionStatus {
                        connected: true,
                        server_url: Some(url),
                        username: Some(username),
                        server_version: Some(ping_result.version),
                    })
                }
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
