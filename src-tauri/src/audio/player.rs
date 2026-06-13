use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use log::warn;
use ringbuf::HeapCons;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::binaural::BinauralPreset;
use crate::audio::compressor::DynamicsPreset;
use crate::audio::equalizer::EqualizerSettings;
use crate::audio::spectrum;
use crate::error::{AppError, AppResult, MutexExt};
use crate::media::MediaControlsManager;
use crate::tray::TrayManager;

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

    pub fn start_spectrum_emitter(&self, app_handle: AppHandle) {
        let consumer = self.inner.get_spectrum_consumer();
        let state = self.inner.state_handle();

        const DEFAULT_SAMPLE_RATE: u32 = 44100;

        thread::spawn(move || {
            let mut analyzer = spectrum::SpectrumAnalyzer::new(DEFAULT_SAMPLE_RATE);
            let mut emitted_idle_state = false;

            loop {
                thread::sleep(Duration::from_millis(33));

                if !state.is_playing() {
                    if !emitted_idle_state {
                        let _ = app_handle.emit("spectrum-data", spectrum::SpectrumData::default());
                        analyzer.clear();
                        emitted_idle_state = true;
                    }
                    continue;
                }

                emitted_idle_state = false;

                if let Ok(mut cons) = consumer.try_lock()
                    && let Some(spectrum_data) = analyzer.process(&mut cons)
                {
                    let _ = app_handle.emit("spectrum-data", spectrum_data);
                }
            }
        });
    }

    pub fn start_position_emitter(&self, app_handle: AppHandle) {
        let state_handle = self.inner.state_handle();

        thread::spawn(move || {
            let mut last_song_id: Option<String> = None;
            let mut last_is_playing = false;
            let mut position_update_counter: u8 = 0;
            let mut last_segment_idx: usize = 0;

            loop {
                thread::sleep(Duration::from_millis(100));

                let (state, segment_idx) = state_handle.get_gapless_state();

                if !state.is_playing && state.song.is_none() {
                    if last_song_id.is_some() {
                        if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>()
                        {
                            media_controls.clear();
                        }
                        if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
                            tray_manager.update_song_info("", "");
                            tray_manager.update_playback_state(false);
                        }
                        last_song_id = None;
                        last_is_playing = false;
                        last_segment_idx = 0;
                    }
                    continue;
                }

                if segment_idx < last_segment_idx {
                    last_segment_idx = 0;
                }

                if segment_idx > last_segment_idx && state.song.is_some() {
                    last_segment_idx = segment_idx;

                    let next_song_id = {
                        let app_state: tauri::State<'_, crate::state::AppState> =
                            app_handle.state();
                        let song_id = {
                            let mut q = app_state.queue.lock_recover();
                            q.next(false).map(|item| item.song_id.clone())
                        };
                        crate::commands::queue::persist_and_emit(&app_state, &app_handle);
                        song_id
                    };

                    let app = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::commands::playback::after_gapless_transition(&app, next_song_id)
                            .await;
                    });
                }

                if state.is_playing
                    && state.song.is_some()
                    && state_handle.is_last_gapless_segment(segment_idx)
                    && !state_handle.is_crossfade_initiated()
                {
                    let settings = crate::commands::settings::read_playback_settings(&app_handle);
                    if settings.crossfade_enabled {
                        let cf_secs = settings.crossfade_duration_ms as f64 / 1000.0;
                        let remaining = state.duration - state.position;

                        if remaining <= cf_secs && remaining > 0.5 {
                            state_handle.set_crossfade_initiated(true);

                            let app = app_handle.clone();
                            let cf_duration_ms = settings.crossfade_duration_ms;
                            tauri::async_runtime::spawn(async move {
                                crate::commands::playback::initiate_crossfade(&app, cf_duration_ms)
                                    .await;
                            });
                        }
                    }
                }

                let playback_finished = state.duration > 0.0
                    && state.position >= state.duration - 0.2
                    && state.song.is_some()
                    && !state.is_playing
                    && !state_handle.is_crossfade_initiated();

                if playback_finished {
                    state_handle.clear_finished_state();

                    last_song_id = None;
                    last_is_playing = false;
                    last_segment_idx = 0;

                    let app = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        match crate::commands::playback::handle_playback_finished(&app).await {
                            Ok(true) => {}
                            Ok(false) => {
                                if let Some(media_controls) =
                                    app.try_state::<MediaControlsManager>()
                                {
                                    media_controls.clear();
                                }

                                if let Some(tray_manager) = app.try_state::<TrayManager>() {
                                    tray_manager.update_song_info("", "");
                                    tray_manager.update_playback_state(false);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to advance playback after track ended: {e}");
                                if let Some(media_controls) =
                                    app.try_state::<MediaControlsManager>()
                                {
                                    media_controls.clear();
                                }

                                if let Some(tray_manager) = app.try_state::<TrayManager>() {
                                    tray_manager.update_song_info("", "");
                                    tray_manager.update_playback_state(false);
                                }
                                let _ = app.emit("playback-ended", ());
                            }
                        }
                    });
                    continue;
                }

                let current_song_id = state.song.as_ref().map(|s| s.id.clone());
                if current_song_id != last_song_id {
                    if let Some(song) = &state.song {
                        if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>()
                        {
                            let cover_art_path = song.cover_art_id.as_ref().and_then(|id| {
                                let cache_dir = crate::cache::cover_cache_dir(&app_handle).ok()?;
                                let sanitized_id = id.replace(['/', '\\'], "_");

                                for size in [800, 128, 64] {
                                    let path =
                                        cache_dir.join(format!("{}_{}.jpg", sanitized_id, size));
                                    if path.exists() {
                                        return Some(path.to_string_lossy().to_string());
                                    }
                                }

                                let path = cache_dir.join(format!("{}.jpg", sanitized_id));
                                if path.exists() {
                                    return Some(path.to_string_lossy().to_string());
                                }
                                None
                            });

                            media_controls.update_metadata(song, state.duration, cover_art_path);
                        }

                        if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
                            tray_manager.update_song_info(&song.title, &song.artist);
                        }
                    }
                    last_song_id = current_song_id;
                }

                if state.is_playing != last_is_playing {
                    if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>() {
                        media_controls.set_playback_status(state.is_playing, state.position);
                    }

                    if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
                        tray_manager.update_playback_state(state.is_playing);
                    }

                    last_is_playing = state.is_playing;
                }

                position_update_counter = (position_update_counter + 1) % 10;
                if position_update_counter == 0
                    && state.is_playing
                    && let Some(media_controls) = app_handle.try_state::<MediaControlsManager>()
                {
                    media_controls.set_playback_status(state.is_playing, state.position);
                }

                if position_update_counter == 0
                    && state.is_playing
                    && let Some(song) = &state.song
                {
                    let app_state: tauri::State<'_, crate::state::AppState> = app_handle.state();
                    crate::lastfm::handle_playback_progress(
                        &app_handle,
                        &app_state,
                        song,
                        state.position,
                        state.duration,
                    );
                }

                let _ = app_handle.emit("playback-state", &state);
            }
        });
    }
}
