use std::sync::{Arc, Mutex};

use ringbuf::HeapCons;

use crate::audio::binaural::BinauralPreset;
use crate::audio::compressor::DynamicsPreset;
use crate::audio::equalizer::EqualizerSettings;
use crate::error::{AppError, AppResult};

pub use stereodrome_audio::{CrossfadePlayRequest, PlaybackStatus, SongMetadata};

pub struct AudioPlayer {
    inner: stereodrome_audio::AudioPlayer,
}

#[allow(dead_code)]
impl AudioPlayer {
    pub fn new() -> AppResult<Self> {
        let inner = stereodrome_audio::AudioPlayer::new()
            .map_err(|error| AppError::Audio(error.to_string()))?;
        Ok(Self { inner })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &self,
        audio_data: Arc<[u8]>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    ) -> AppResult<()> {
        self.inner
            .play(
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
            )
            .map_err(|error| AppError::Audio(error.to_string()))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_gapless(
        &self,
        audio_data: Arc<[u8]>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    ) -> AppResult<()> {
        self.inner
            .append_gapless(
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
            )
            .map_err(|error| AppError::Audio(error.to_string()))
    }

    pub fn crossfade_play(&self, request: CrossfadePlayRequest) -> AppResult<()> {
        self.inner
            .crossfade_play(request)
            .map_err(|error| AppError::Audio(error.to_string()))
    }

    pub fn is_crossfade_initiated(&self) -> bool {
        self.inner.is_crossfade_initiated()
    }

    pub fn set_crossfade_initiated(&self, value: bool) {
        self.inner.set_crossfade_initiated(value);
    }

    pub fn pause(&self) -> AppResult<()> {
        self.inner
            .pause()
            .map_err(|error| AppError::Audio(error.to_string()))
    }

    pub fn resume(&self) -> AppResult<()> {
        self.inner
            .resume()
            .map_err(|error| AppError::Audio(error.to_string()))
    }

    pub fn stop(&self) -> AppResult<()> {
        self.inner
            .stop()
            .map_err(|error| AppError::Audio(error.to_string()))
    }

    pub fn set_volume(&self, volume: f32) -> AppResult<()> {
        self.inner
            .set_volume(volume)
            .map_err(|error| AppError::Audio(error.to_string()))
    }

    pub fn seek(&self, position_secs: f64) -> AppResult<()> {
        self.inner
            .seek(position_secs)
            .map_err(|error| AppError::Audio(error.to_string()))
    }

    pub fn get_volume(&self) -> f32 {
        self.inner.get_volume()
    }

    pub fn get_position(&self) -> f64 {
        self.inner.get_position()
    }

    pub fn get_duration(&self) -> f64 {
        self.inner.get_duration()
    }

    pub fn get_status(&self) -> PlaybackStatus {
        self.inner.get_status()
    }

    pub fn current_song_id(&self) -> Option<String> {
        self.inner.current_song_id()
    }

    pub fn is_playing(&self) -> bool {
        self.inner.is_playing()
    }

    pub fn get_spectrum_consumer(&self) -> Arc<Mutex<HeapCons<f32>>> {
        self.inner.get_spectrum_consumer()
    }

    pub fn state_handle(&self) -> stereodrome_audio::AudioStateHandle {
        self.inner.state_handle()
    }
}
