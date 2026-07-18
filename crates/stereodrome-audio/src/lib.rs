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
    AudioPlayer, AudioStateHandle, CrossfadePlayRequest, PlaybackLifecycleState, PlaybackState,
    PlaybackStatus, SongMetadata,
};

pub trait MutexExt<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use rodio::buffer::SamplesBuffer;

    use crate::{
        BinauralPreset, DynamicsPreset,
        binaural::BinauralSource,
        dynamics::DynamicsSource,
        equalizer::{EQ_BAND_COUNT, EqualizerSettings, EqualizerSource},
        normalizer::NormalizingSource,
    };

    #[test]
    fn composed_dsp_pipeline_produces_finite_stereo_samples() {
        let input: Vec<f32> = (0..4_096).flat_map(|_| [0.8, 0.2]).collect();
        let source = SamplesBuffer::new(
            NonZero::new(2).unwrap(),
            NonZero::new(44_100).unwrap(),
            input.clone(),
        );
        let normalized = NormalizingSource::new(source, 0.5);
        let dynamics = DynamicsSource::new(normalized, &DynamicsPreset::Medium);
        let equalized =
            EqualizerSource::new(dynamics, &EqualizerSettings::new(vec![3.0; EQ_BAND_COUNT]));
        let output: Vec<f32> = BinauralSource::new(equalized, &BinauralPreset::Default).collect();

        assert_eq!(output.len(), input.len());
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert_ne!(output, input);
    }
}
