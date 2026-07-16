use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DesktopError {
    #[error("no desktop data directory is available")]
    NoDataDirectory,
    #[error(
        "desktop profile mismatch: candidate {candidate:?}, installed Tauri profile {installed:?}"
    )]
    ProfileMismatch {
        candidate: PathBuf,
        installed: PathBuf,
    },
    #[error("I/O error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
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
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.into(),
            source,
        }
    }
}
