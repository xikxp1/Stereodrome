use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Audio decode error: {0}")]
    Decode(String),
    #[error("Audio playback error: {0}")]
    Playback(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type AudioResult<T> = Result<T, AudioError>;
