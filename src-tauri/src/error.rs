use serde::Serialize;
use std::sync::{Mutex, MutexGuard};
use thiserror::Error;

use crate::client::ClientError;

/// Extension trait for Mutex that provides poison-recovery locking.
///
/// When a thread panics while holding a mutex lock, the mutex becomes "poisoned".
/// Rather than panicking on subsequent lock attempts, this trait recovers the
/// inner data, allowing the application to continue functioning.
pub trait MutexExt<T> {
    /// Locks the mutex, recovering from poison if necessary.
    ///
    /// If the mutex is poisoned (a thread panicked while holding it),
    /// this method recovers the inner data instead of panicking.
    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Subsonic API error: {0}")]
    Subsonic(String),

    #[error("Not connected to server")]
    NotConnected,

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
}

impl From<ClientError> for AppError {
    fn from(err: ClientError) -> Self {
        match err {
            ClientError::NotConnected => AppError::NotConnected,
            ClientError::ConnectionFailed(s) => AppError::Subsonic(s),
            ClientError::ApiError(s) => AppError::Subsonic(s),
            ClientError::ChannelClosed => {
                AppError::ClientChannel("Client channel closed".to_string())
            }
            ClientError::Timeout => AppError::Timeout,
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
