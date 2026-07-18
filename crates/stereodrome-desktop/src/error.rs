use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;
use thiserror::Error;

pub trait MutexExt<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Debug, Error)]
pub enum DesktopError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Subsonic API error: {0}")]
    Subsonic(String),
    #[error("Not connected to server")]
    NotConnected,
    #[error("Offline mode is enabled")]
    OfflineMode,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Audio playback error: {0}")]
    Audio(String),
    #[error("Search error: {0}")]
    Search(String),
    #[error("Credentials error: {0}")]
    Credentials(String),
    #[error("Client channel error: {0}")]
    ClientChannel(String),
    #[error("Request timed out")]
    Timeout,
    #[error("Window error: {0}")]
    Window(String),
    #[error("Last.fm error: {0}")]
    Lastfm(String),
    #[error("no desktop data directory is available")]
    NoDataDirectory,
    #[error("desktop profile mismatch: candidate {candidate:?}, installed profile {installed:?}")]
    ProfileMismatch {
        candidate: PathBuf,
        installed: PathBuf,
    },
    #[error("invalid JSON at {path:?}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("JSON store root at {path:?} must be an object")]
    JsonRootNotObject { path: PathBuf },
    #[error("worker registry is shutting down")]
    ShuttingDown,
    #[error("desktop shutdown failed: {0}")]
    Shutdown(String),
}

impl DesktopError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io(std::io::Error::new(
            source.kind(),
            format!("{:?}: {source}", path.into()),
        ))
    }

    pub(crate) fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.into(),
            source,
        }
    }
}

impl From<crate::client::ClientError> for DesktopError {
    fn from(error: crate::client::ClientError) -> Self {
        match error {
            crate::client::ClientError::NotConnected => Self::NotConnected,
            crate::client::ClientError::ConnectionFailed(message)
            | crate::client::ClientError::ApiError(message) => Self::Subsonic(message),
            crate::client::ClientError::ChannelClosed => {
                Self::ClientChannel("Client channel closed".to_string())
            }
            crate::client::ClientError::Timeout => Self::Timeout,
        }
    }
}

impl Serialize for DesktopError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppError = DesktopError;
pub type AppResult<T> = Result<T, AppError>;
