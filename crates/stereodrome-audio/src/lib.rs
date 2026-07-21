pub mod analyzer;
pub mod binaural;
pub mod compressor;
pub mod dynamics;
pub mod equalizer;
pub mod error;
pub mod loudness;
pub mod normalizer;
pub mod player;
pub mod spectrum;

use std::sync::{Mutex, MutexGuard};

pub use binaural::BinauralPreset;
pub use compressor::DynamicsPreset;
pub use equalizer::{EQ_BAND_COUNT, EqualizerSettings, default_bands_db, sanitize_bands_db};
pub use error::{AudioError, AudioResult};
pub use player::{
    AudioNotification, AudioOutputState, AudioPlayer, AudioStateHandle, CrossfadePlayRequest,
    PlaybackIdentity, PlaybackLifecycleState, PlaybackState, PlaybackStatus, SongMetadata,
};

pub trait MutexExt<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        self.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
