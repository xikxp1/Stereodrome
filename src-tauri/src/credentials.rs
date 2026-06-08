use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::error::{AppError, AppResult};
use crate::state::ServerConfig;
use keyring_core::{Entry, Error as KeyringError};

const SERVICE_NAME: &str = "stereodrome";
const ACCOUNT_NAME: &str = "server_credentials";
const LASTFM_ACCOUNT_NAME: &str = "lastfm_session";
static KEYRING_STORE_INITIALIZED: OnceLock<()> = OnceLock::new();

fn keyring_entry(account: &str) -> AppResult<Entry> {
    if KEYRING_STORE_INITIALIZED.get().is_none() {
        keyring::use_native_store(false).map_err(|e| {
            AppError::Credentials(format!("Failed to initialize keyring store: {}", e))
        })?;
        let _ = KEYRING_STORE_INITIALIZED.set(());
    }

    Entry::new(SERVICE_NAME, account)
        .map_err(|e| AppError::Credentials(format!("Failed to create keyring entry: {}", e)))
}

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
    let entry = keyring_entry(ACCOUNT_NAME)?;

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
    let entry = keyring_entry(ACCOUNT_NAME)?;

    match entry.get_password() {
        Ok(json) => {
            let creds: StoredCredentials = serde_json::from_str(&json).map_err(|e| {
                AppError::Credentials(format!("Failed to parse credentials: {}", e))
            })?;
            Ok(Some(creds.into()))
        }
        Err(KeyringError::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Credentials(format!(
            "Failed to load credentials: {}",
            e
        ))),
    }
}

/// Delete server credentials from the OS keyring
pub fn delete_credentials() -> AppResult<()> {
    let entry = keyring_entry(ACCOUNT_NAME)?;

    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(KeyringError::NoEntry) => Ok(()), // Already deleted, that's fine
        Err(e) => Err(AppError::Credentials(format!(
            "Failed to delete credentials: {}",
            e
        ))),
    }
}

/// Last.fm session stored in the OS keyring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastfmSession {
    pub username: String,
    pub session_key: String,
}

pub fn save_lastfm_session(session: &LastfmSession) -> AppResult<()> {
    let entry = keyring_entry(LASTFM_ACCOUNT_NAME)?;

    let json = serde_json::to_string(session).map_err(|e| {
        AppError::Credentials(format!("Failed to serialize Last.fm session: {}", e))
    })?;

    entry
        .set_password(&json)
        .map_err(|e| AppError::Credentials(format!("Failed to save Last.fm session: {}", e)))?;

    Ok(())
}

pub fn load_lastfm_session() -> AppResult<Option<LastfmSession>> {
    let entry = keyring_entry(LASTFM_ACCOUNT_NAME)?;

    match entry.get_password() {
        Ok(json) => {
            let session: LastfmSession = serde_json::from_str(&json).map_err(|e| {
                AppError::Credentials(format!("Failed to parse Last.fm session: {}", e))
            })?;
            Ok(Some(session))
        }
        Err(KeyringError::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Credentials(format!(
            "Failed to load Last.fm session: {}",
            e
        ))),
    }
}

pub fn delete_lastfm_session() -> AppResult<()> {
    let entry = keyring_entry(LASTFM_ACCOUNT_NAME)?;

    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(KeyringError::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Credentials(format!(
            "Failed to delete Last.fm session: {}",
            e
        ))),
    }
}
