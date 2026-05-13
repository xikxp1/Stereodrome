pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("not connected to a Subsonic server")]
    NotConnected,
    #[error("unsupported album list type: {0}")]
    InvalidAlbumListType(String),
    #[error("shared state lock was poisoned")]
    LockPoisoned,
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Subsonic error: {0}")]
    Subsonic(String),
}
