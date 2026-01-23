use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::state::ServerConfig;

const SERVICE_NAME: &str = "stereodrome";
const ACCOUNT_NAME: &str = "server_credentials";

/// Credentials stored in the OS keyring as a single JSON entry
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCredentials {
    url: String,
    username: String,
    password: String,
}

impl From<&ServerConfig> for StoredCredentials {
    fn from(config: &ServerConfig) -> Self {
        Self {
            url: config.url.clone(),
            username: config.username.clone(),
            password: config.password.clone(),
        }
    }
}

impl From<StoredCredentials> for ServerConfig {
    fn from(creds: StoredCredentials) -> Self {
        Self {
            url: creds.url,
            username: creds.username,
            password: creds.password,
        }
    }
}

/// Save server credentials to the OS keyring
pub fn save_credentials(config: &ServerConfig) -> AppResult<()> {
    let entry = keyring::Entry::new(SERVICE_NAME, ACCOUNT_NAME)
        .map_err(|e| AppError::Credentials(format!("Failed to create keyring entry: {}", e)))?;

    let creds = StoredCredentials::from(config);
    let json = serde_json::to_string(&creds)
        .map_err(|e| AppError::Credentials(format!("Failed to serialize credentials: {}", e)))?;

    entry
        .set_password(&json)
        .map_err(|e| AppError::Credentials(format!("Failed to save credentials: {}", e)))?;

    Ok(())
}

/// Load server credentials from the OS keyring
pub fn load_credentials() -> AppResult<Option<ServerConfig>> {
    let entry = keyring::Entry::new(SERVICE_NAME, ACCOUNT_NAME)
        .map_err(|e| AppError::Credentials(format!("Failed to create keyring entry: {}", e)))?;

    match entry.get_password() {
        Ok(json) => {
            let creds: StoredCredentials = serde_json::from_str(&json).map_err(|e| {
                AppError::Credentials(format!("Failed to parse credentials: {}", e))
            })?;
            Ok(Some(creds.into()))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Credentials(format!(
            "Failed to load credentials: {}",
            e
        ))),
    }
}

/// Delete server credentials from the OS keyring
pub fn delete_credentials() -> AppResult<()> {
    let entry = keyring::Entry::new(SERVICE_NAME, ACCOUNT_NAME)
        .map_err(|e| AppError::Credentials(format!("Failed to create keyring entry: {}", e)))?;

    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // Already deleted, that's fine
        Err(e) => Err(AppError::Credentials(format!(
            "Failed to delete credentials: {}",
            e
        ))),
    }
}
