use serde::{Deserialize, Serialize};
use submarine::{auth::AuthBuilder, Client};
use tauri::{AppHandle, State};
use tauri_plugin_store::StoreExt;

use crate::error::{AppError, AppResult};
use crate::state::{AppState, ServerConfig};

const STORE_FILE: &str = "settings.json";
const KEY_SERVER_URL: &str = "server_url";
const KEY_USERNAME: &str = "username";
const KEY_PASSWORD: &str = "password";

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
    app: AppHandle,
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

    // Store client and config in memory
    {
        let mut client_lock = state.client.lock().unwrap();
        *client_lock = Some(client);
    }

    {
        let mut config_lock = state.server_config.lock().unwrap();
        *config_lock = Some(ServerConfig {
            url: params.url.clone(),
            username: params.username.clone(),
            password: params.password.clone(),
        });
    }

    // Persist credentials to store
    if let Ok(store) = app.store(STORE_FILE) {
        store.set(KEY_SERVER_URL, serde_json::json!(params.url));
        store.set(KEY_USERNAME, serde_json::json!(params.username));
        store.set(KEY_PASSWORD, serde_json::json!(params.password));
        let _ = store.save();
    }

    Ok(ConnectionStatus {
        connected: true,
        server_url: Some(params.url),
        username: Some(params.username),
        server_version: Some(ping_result.version),
    })
}

#[tauri::command]
pub async fn disconnect_server(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    {
        let mut client_lock = state.client.lock().unwrap();
        *client_lock = None;
    }

    {
        let mut config_lock = state.server_config.lock().unwrap();
        *config_lock = None;
    }

    // Clear stored credentials
    if let Ok(store) = app.store(STORE_FILE) {
        let _ = store.delete(KEY_SERVER_URL);
        let _ = store.delete(KEY_USERNAME);
        let _ = store.delete(KEY_PASSWORD);
        let _ = store.save();
    }

    Ok(())
}

#[tauri::command]
pub async fn get_connection_status(state: State<'_, AppState>) -> AppResult<ConnectionStatus> {
    let config = state.server_config.lock().unwrap();

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
pub async fn restore_session(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ConnectionStatus> {
    // Check if already connected
    if state.is_connected() {
        let config = state.server_config.lock().unwrap();
        if let Some(cfg) = config.as_ref() {
            return Ok(ConnectionStatus {
                connected: true,
                server_url: Some(cfg.url.clone()),
                username: Some(cfg.username.clone()),
                server_version: None,
            });
        }
    }

    // Try to load credentials from store
    let store = app
        .store(STORE_FILE)
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    let url = store
        .get(KEY_SERVER_URL)
        .and_then(|v| v.as_str().map(String::from));
    let username = store
        .get(KEY_USERNAME)
        .and_then(|v| v.as_str().map(String::from));
    let password = store
        .get(KEY_PASSWORD)
        .and_then(|v| v.as_str().map(String::from));

    match (url, username, password) {
        (Some(url), Some(username), Some(password)) => {
            // Attempt to reconnect
            let auth = AuthBuilder::new(&username, "1.16.1")
                .client_name("Stereodrome")
                .hashed(&password);

            let client = Client::new(&url, auth);

            // Test connection
            match client.ping().await {
                Ok(ping_result) => {
                    // Store client and config
                    {
                        let mut client_lock = state.client.lock().unwrap();
                        *client_lock = Some(client);
                    }
                    {
                        let mut config_lock = state.server_config.lock().unwrap();
                        *config_lock = Some(ServerConfig {
                            url: url.clone(),
                            username: username.clone(),
                            password,
                        });
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
        _ => {
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
