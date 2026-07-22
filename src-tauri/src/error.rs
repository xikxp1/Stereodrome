use serde::Serialize;
use std::sync::{Mutex, MutexGuard};
use thiserror::Error;

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
        self.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Not connected to server")]
    NotConnected,

    #[error("Offline mode is enabled")]
    OfflineMode,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Audio playback error: {0}")]
    Audio(String),

    #[error("Credentials error: {0}")]
    Credentials(String),

    #[error("Window error: {0}")]
    Window(String),

    #[error("Backup error: {0}")]
    Backup(String),

    #[error("Runtime error: {0}")]
    Runtime(String),
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
