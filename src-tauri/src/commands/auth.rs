use serde::{Deserialize, Serialize};
use submarine::{auth::AuthBuilder, Client};
use tauri::State;

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
    let auth = AuthBuilder::new(&params.username, "1.16.1")
        .client_name("Stereodrome")
        .hashed(&params.password);

    let client = Client::new(&params.url, auth);

    // Test connection with ping
    let ping_result = client
        .ping()
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    // Store client and config
    {
        let mut client_lock = state.client.lock().unwrap();
        *client_lock = Some(client);
    }

    {
        let mut config_lock = state.server_config.lock().unwrap();
        *config_lock = Some(ServerConfig {
            url: params.url.clone(),
            username: params.username.clone(),
            password: params.password,
        });
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
        let mut client_lock = state.client.lock().unwrap();
        *client_lock = None;
    }

    {
        let mut config_lock = state.server_config.lock().unwrap();
        *config_lock = None;
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
