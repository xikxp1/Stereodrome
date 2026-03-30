use log::{error, warn};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::Split};
use rodio::{Decoder, Player, Source};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::analyzer::AnalyzingSource;
use crate::audio::binaural::{BinauralPreset, BinauralSource};
use crate::audio::compressor::DynamicsPreset;
use crate::audio::dynamics::DynamicsSource;
use crate::audio::equalizer::{EqualizerSettings, EqualizerSource};
use crate::audio::normalizer::NormalizingSource;
use crate::audio::output::{self, AudioOutputRouteState};
use crate::error::{AppError, AppResult, MutexExt};
use crate::media::MediaControlsManager;
use crate::tray::TrayManager;

/// Ring buffer size for spectrum analysis (~370ms at 44.1kHz stereo)
/// Larger buffer prevents sample loss from lock contention
const SPECTRUM_BUFFER_SIZE: usize = 32768;

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct SongMetadata {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover_art_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackStatus {
    pub is_playing: bool,
    pub current_song_id: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackState {
    pub is_playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub song: Option<SongMetadata>,
}

/// Commands sent to the audio thread
enum AudioCommand {
    Play {
        audio_data: Vec<u8>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    },
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    Seek(f64),
    /// Append a song to the existing player for gapless playback.
    /// Unlike Play, this does NOT create a new player.
    AppendGapless {
        audio_data: Vec<u8>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    },
    /// Crossfade to a new song: keep the current sink fading out
    /// while a new sink fades in over the specified duration.
    CrossfadePlay {
        audio_data: Vec<u8>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
        crossfade_duration_ms: u32,
    },
    SetOutputDevice {
        preferred_device_id: Option<String>,
        result_tx: Sender<Result<(), String>>,
    },
    RecoverOutputStream,
    Shutdown,
}

#[derive(Debug)]
pub struct CrossfadePlayRequest {
    pub audio_data: Vec<u8>,
    pub metadata: SongMetadata,
    pub duration_secs: f64,
    pub normalization_gain: Option<f32>,
    pub dynamics_preset: Option<DynamicsPreset>,
    pub binaural_preset: Option<BinauralPreset>,
    pub equalizer_settings: Option<EqualizerSettings>,
    pub crossfade_duration_ms: u32,
}

/// A segment within a gapless playback chain.
/// Each segment represents one song appended to the same Rodio player.
#[derive(Debug, Clone)]
struct BufferedTrack {
    audio_data: Vec<u8>,
    metadata: SongMetadata,
    duration_secs: f64,
    normalization_gain: Option<f32>,
    dynamics_preset: Option<DynamicsPreset>,
    binaural_preset: Option<BinauralPreset>,
    equalizer_settings: Option<EqualizerSettings>,
}

#[derive(Debug, Clone)]
struct GaplessSegment {
    track: BufferedTrack,
    cumulative_start: f64, // sum of all previous segments' durations
}

#[derive(Debug, Clone)]
struct PlaybackSnapshot {
    track: BufferedTrack,
    position_secs: f64,
    was_playing: bool,
}

/// Inner playback state consolidated into a single struct for efficient locking
struct PlaybackInner {
    current_song: Option<SongMetadata>,
    current_track: Option<BufferedTrack>,
    volume: f32,
    playback_start: Option<Instant>,
    paused_position: f64,
    duration: f64,
    /// Gapless playback segments. When multiple consecutive album tracks are
    /// appended to the same player, each gets a segment for position tracking.
    gapless_segments: Vec<GaplessSegment>,
    preferred_output_device_id: Option<String>,
    output_route: AudioOutputRouteState,
}

impl Default for PlaybackInner {
    fn default() -> Self {
        Self {
            current_song: None,
            current_track: None,
            volume: 0.8,
            playback_start: None,
            paused_position: 0.0,
            duration: 0.0,
            gapless_segments: Vec::new(),
            preferred_output_device_id: None,
            output_route: AudioOutputRouteState::default(),
        }
    }
}

/// State shared between the main thread and audio thread.
/// Uses a single RwLock for efficient concurrent reads (position emitter at 10Hz).
struct SharedState {
    is_playing: AtomicBool,
    crossfade_initiated: AtomicBool,
    inner: RwLock<PlaybackInner>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            is_playing: AtomicBool::new(false),
            crossfade_initiated: AtomicBool::new(false),
            inner: RwLock::new(PlaybackInner::default()),
        }
    }

    fn read_inner(&self) -> std::sync::RwLockReadGuard<'_, PlaybackInner> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_inner(&self) -> std::sync::RwLockWriteGuard<'_, PlaybackInner> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn get_position(&self) -> f64 {
        let inner = self.read_inner();
        self.calculate_position(&inner)
    }

    /// Calculate position from an already-acquired read guard (avoids double locking)
    fn calculate_position(&self, inner: &PlaybackInner) -> f64 {
        if !self.is_playing.load(Ordering::SeqCst) {
            return inner.paused_position;
        }

        if let Some(instant) = inner.playback_start {
            (instant.elapsed().as_secs_f64() + inner.paused_position).min(inner.duration)
        } else {
            inner.paused_position
        }
    }

    /// Get playback state with segment-aware position/metadata and the current segment index.
    /// When gapless segments are active, returns per-song position and metadata
    /// instead of cumulative position across the chain.
    fn get_gapless_state(&self) -> (PlaybackState, usize) {
        let inner = self.read_inner();
        let is_playing = self.is_playing.load(Ordering::SeqCst);
        let cumulative_pos = self.calculate_position(&inner);

        if inner.gapless_segments.len() > 1 {
            // Find which segment we're in
            let mut seg_idx = 0;
            for (i, seg) in inner.gapless_segments.iter().enumerate() {
                if cumulative_pos < seg.cumulative_start + seg.track.duration_secs {
                    seg_idx = i;
                    break;
                }
                seg_idx = i; // default to last segment
            }
            let seg = &inner.gapless_segments[seg_idx];
            let song_pos = (cumulative_pos - seg.cumulative_start)
                .max(0.0)
                .min(seg.track.duration_secs);

            (
                PlaybackState {
                    is_playing,
                    position: song_pos,
                    duration: seg.track.duration_secs,
                    volume: inner.volume,
                    song: Some(seg.track.metadata.clone()),
                },
                seg_idx,
            )
        } else {
            (
                PlaybackState {
                    is_playing,
                    position: cumulative_pos,
                    duration: inner.duration,
                    volume: inner.volume,
                    song: inner.current_song.clone(),
                },
                0,
            )
        }
    }

    fn get_status(&self) -> PlaybackStatus {
        let (state, _) = self.get_gapless_state();
        PlaybackStatus {
            is_playing: state.is_playing,
            current_song_id: state.song.map(|s| s.id),
            position: state.position,
            duration: state.duration,
            volume: state.volume,
        }
    }

    fn get_output_route(&self) -> AudioOutputRouteState {
        self.read_inner().output_route.clone()
    }
}

pub struct AudioPlayer {
    command_tx: Sender<AudioCommand>,
    shared_state: Arc<SharedState>,
    spectrum_consumer: Arc<Mutex<HeapCons<f32>>>,
    _audio_thread: JoinHandle<()>,
}

// Implement Send + Sync manually since we use channels for thread communication
unsafe impl Send for AudioPlayer {}
unsafe impl Sync for AudioPlayer {}

impl AudioPlayer {
    pub fn new() -> AppResult<Self> {
        let (command_tx, command_rx) = mpsc::channel::<AudioCommand>();
        let shared_state = Arc::new(SharedState::new());
        let state_clone = Arc::clone(&shared_state);

        // Create ring buffer for spectrum analysis
        let ring_buffer = HeapRb::<f32>::new(SPECTRUM_BUFFER_SIZE);
        let (producer, consumer) = ring_buffer.split();
        let spectrum_producer = Arc::new(Mutex::new(producer));
        let spectrum_consumer = Arc::new(Mutex::new(consumer));

        let producer_clone = Arc::clone(&spectrum_producer);
        let command_tx_clone = command_tx.clone();
        let audio_thread = thread::spawn(move || {
            run_audio_thread(command_rx, command_tx_clone, state_clone, producer_clone);
        });

        Ok(Self {
            command_tx,
            shared_state,
            spectrum_consumer,
            _audio_thread: audio_thread,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &self,
        audio_data: Vec<u8>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    ) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Play {
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
            })
            .map_err(|e| AppError::Audio(format!("Failed to send play command: {}", e)))
    }

    /// Append a song to the existing player for gapless playback.
    /// The song's audio pipeline is decoded and appended without stopping the current player.
    #[allow(clippy::too_many_arguments)]
    pub fn append_gapless(
        &self,
        audio_data: Vec<u8>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    ) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::AppendGapless {
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
            })
            .map_err(|e| AppError::Audio(format!("Failed to send gapless command: {}", e)))
    }

    /// Start a crossfade transition: fade out current song while fading in a new one.
    pub fn crossfade_play(&self, request: CrossfadePlayRequest) -> AppResult<()> {
        let CrossfadePlayRequest {
            audio_data,
            metadata,
            duration_secs,
            normalization_gain,
            dynamics_preset,
            binaural_preset,
            equalizer_settings,
            crossfade_duration_ms,
        } = request;

        self.command_tx
            .send(AudioCommand::CrossfadePlay {
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
                crossfade_duration_ms,
            })
            .map_err(|e| AppError::Audio(format!("Failed to send crossfade command: {}", e)))
    }

    #[allow(dead_code)]
    pub fn is_crossfade_initiated(&self) -> bool {
        self.shared_state.crossfade_initiated.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    pub fn set_crossfade_initiated(&self, value: bool) {
        self.shared_state
            .crossfade_initiated
            .store(value, Ordering::SeqCst);
    }

    pub fn pause(&self) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Pause)
            .map_err(|e| AppError::Audio(format!("Failed to send pause command: {}", e)))
    }

    pub fn resume(&self) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Resume)
            .map_err(|e| AppError::Audio(format!("Failed to send resume command: {}", e)))
    }

    pub fn stop(&self) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Stop)
            .map_err(|e| AppError::Audio(format!("Failed to send stop command: {}", e)))
    }

    pub fn set_output_device(&self, preferred_device_id: Option<String>) -> AppResult<()> {
        {
            let mut inner = self.shared_state.write_inner();
            inner.preferred_output_device_id = preferred_device_id.clone();
        }

        let (result_tx, result_rx) = mpsc::channel();

        self.command_tx
            .send(AudioCommand::SetOutputDevice {
                preferred_device_id,
                result_tx,
            })
            .map_err(|e| AppError::Audio(format!("Failed to send output device command: {e}")))?;

        result_rx
            .recv()
            .map_err(|e| AppError::Audio(format!("Failed to receive output device result: {e}")))?
            .map_err(AppError::Audio)
    }

    pub fn set_volume(&self, volume: f32) -> AppResult<()> {
        let clamped = volume.clamp(0.0, 1.0);
        self.shared_state.write_inner().volume = clamped;
        self.command_tx
            .send(AudioCommand::SetVolume(clamped))
            .map_err(|e| AppError::Audio(format!("Failed to send volume command: {}", e)))
    }

    pub fn seek(&self, position_secs: f64) -> AppResult<()> {
        let duration = self.shared_state.read_inner().duration;
        let clamped = position_secs.clamp(0.0, duration);
        self.command_tx
            .send(AudioCommand::Seek(clamped))
            .map_err(|e| AppError::Audio(format!("Failed to send seek command: {}", e)))
    }

    #[allow(dead_code)]
    pub fn get_volume(&self) -> f32 {
        self.shared_state.read_inner().volume
    }

    #[allow(dead_code)]
    pub fn get_position(&self) -> f64 {
        self.shared_state.get_position()
    }

    #[allow(dead_code)]
    pub fn get_duration(&self) -> f64 {
        self.shared_state.read_inner().duration
    }

    pub fn get_status(&self) -> PlaybackStatus {
        self.shared_state.get_status()
    }

    pub fn get_output_route(&self) -> AudioOutputRouteState {
        self.shared_state.get_output_route()
    }

    #[allow(dead_code)]
    pub fn current_song_id(&self) -> Option<String> {
        self.shared_state.get_status().current_song_id
    }

    #[allow(dead_code)]
    pub fn is_playing(&self) -> bool {
        self.shared_state.is_playing.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    /// Get the spectrum buffer consumer for the spectrum analyzer
    pub fn get_spectrum_consumer(&self) -> Arc<Mutex<HeapCons<f32>>> {
        Arc::clone(&self.spectrum_consumer)
    }

    #[allow(dead_code)]
    /// Get the is_playing flag for the spectrum analyzer
    pub fn get_is_playing_flag(&self) -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(
            self.shared_state.is_playing.load(Ordering::SeqCst),
        ))
    }

    #[allow(dead_code)]
    /// Get a clone of the shared is_playing atomic for spectrum emitter
    pub fn get_shared_is_playing(&self) -> &AtomicBool {
        &self.shared_state.is_playing
    }

    /// Start a background thread that emits spectrum data for visualization
    pub fn start_spectrum_emitter(&self, app_handle: AppHandle) {
        use crate::audio::spectrum;

        let consumer = Arc::clone(&self.spectrum_consumer);
        let shared_state = Arc::clone(&self.shared_state);

        // Default sample rate (will work for most audio)
        const DEFAULT_SAMPLE_RATE: u32 = 44100;

        thread::spawn(move || {
            let mut analyzer = spectrum::SpectrumAnalyzer::new(DEFAULT_SAMPLE_RATE);

            loop {
                thread::sleep(Duration::from_millis(33)); // 30Hz updates

                let is_playing = shared_state.is_playing.load(Ordering::SeqCst);

                // Only process when playing
                if !is_playing {
                    // Emit empty spectrum when not playing
                    let _ = app_handle.emit("spectrum-data", spectrum::SpectrumData::default());
                    analyzer.clear();
                    continue;
                }

                // Process samples and emit if we have data
                if let Ok(mut cons) = consumer.try_lock()
                    && let Some(spectrum_data) = analyzer.process(&mut cons)
                {
                    let _ = app_handle.emit("spectrum-data", spectrum_data);
                }
            }
        });
    }

    /// Start a background thread that emits playback state updates
    pub fn start_position_emitter(&self, app_handle: AppHandle) {
        let shared_state = Arc::clone(&self.shared_state);

        thread::spawn(move || {
            // State tracking for media controls
            let mut last_song_id: Option<String> = None;
            let mut last_is_playing = false;
            let mut position_update_counter: u8 = 0;
            let mut last_segment_idx: usize = 0;

            loop {
                thread::sleep(Duration::from_millis(100)); // 10Hz updates

                let (state, segment_idx) = shared_state.get_gapless_state();

                // Only emit when playing or when we have a song (to update paused state)
                if !state.is_playing && state.song.is_none() {
                    // Clear media controls and tray if we had a song before
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

                // Reset segment tracking when a new gapless chain starts.
                // A fresh Play command resets segments to [new_song] with seg_idx=0,
                // but last_segment_idx may still hold the value from the previous chain.
                if segment_idx < last_segment_idx {
                    last_segment_idx = 0;
                }

                // Detect gapless segment transition (song changed within the same player)
                if segment_idx > last_segment_idx && state.song.is_some() {
                    last_segment_idx = segment_idx;

                    // Advance queue synchronously to prevent race with playback_finished.
                    // If we only spawned an async task, the queue might not advance before
                    // the next tick detects playback_finished and emits playback-ended.
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

                    // Spawn async task for scrobble, prefetch, and next gapless check
                    let app = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::commands::playback::after_gapless_transition(&app, next_song_id)
                            .await;
                    });
                }

                // Crossfade trigger: detect when approaching end of song/chain
                if state.is_playing && state.song.is_some() {
                    let is_last_segment = {
                        let inner = shared_state.read_inner();
                        inner.gapless_segments.len() <= 1
                            || segment_idx == inner.gapless_segments.len() - 1
                    };

                    if is_last_segment && !shared_state.crossfade_initiated.load(Ordering::SeqCst) {
                        let settings =
                            crate::commands::settings::read_playback_settings(&app_handle);
                        if settings.crossfade_enabled {
                            let cf_secs = settings.crossfade_duration_ms as f64 / 1000.0;
                            let remaining = state.duration - state.position;

                            if remaining <= cf_secs && remaining > 0.5 {
                                shared_state
                                    .crossfade_initiated
                                    .store(true, Ordering::SeqCst);

                                let app = app_handle.clone();
                                let cf_duration_ms = settings.crossfade_duration_ms;
                                tauri::async_runtime::spawn(async move {
                                    crate::commands::playback::initiate_crossfade(
                                        &app,
                                        cf_duration_ms,
                                    )
                                    .await;
                                });
                            }
                        }
                    }
                }

                // Check if playback ended naturally (entire gapless chain finished)
                // The audio thread sets is_playing=false and paused_position=duration
                // when sink.empty(). With gapless, this means ALL appended sources finished.
                let crossfade_active = shared_state.crossfade_initiated.load(Ordering::SeqCst);
                let playback_finished = state.duration > 0.0
                    && state.position >= state.duration - 0.2 // Small tolerance for timing
                    && state.song.is_some()
                    && !state.is_playing
                    && !crossfade_active;

                if playback_finished {
                    // Clear the current song so frontend updates
                    {
                        let mut inner = shared_state.write_inner();
                        inner.current_song = None;
                        inner.paused_position = 0.0;
                        inner.duration = 0.0;
                        inner.gapless_segments.clear();
                    }

                    // Clear media controls
                    if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>() {
                        media_controls.clear();
                    }

                    // Clear tray
                    if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
                        tray_manager.update_song_info("", "");
                        tray_manager.update_playback_state(false);
                    }

                    last_song_id = None;
                    last_is_playing = false;
                    last_segment_idx = 0;

                    let _ = app_handle.emit("playback-ended", ());
                    continue;
                }

                // Update media controls and tray when song changes
                let current_song_id = state.song.as_ref().map(|s| s.id.clone());
                if current_song_id != last_song_id {
                    if let Some(song) = &state.song {
                        if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>()
                        {
                            // Get cover art path from cache - try various sizes that might be cached
                            let cover_art_path = song.cover_art_id.as_ref().and_then(|id| {
                                let data_dir = app_handle.path().app_data_dir().ok()?;
                                let cache_dir = data_dir.join("cover_cache");
                                let sanitized_id = id.replace(['/', '\\'], "_");

                                // Try sizes in order of preference: 800 (full viewer), 128 (notifications), 64 (transport bar)
                                for size in [800, 128, 64] {
                                    let path =
                                        cache_dir.join(format!("{}_{}.jpg", sanitized_id, size));
                                    if path.exists() {
                                        return Some(path.to_string_lossy().to_string());
                                    }
                                }
                                // Fallback to unsized version
                                let path = cache_dir.join(format!("{}.jpg", sanitized_id));
                                if path.exists() {
                                    return Some(path.to_string_lossy().to_string());
                                }
                                None
                            });

                            media_controls.update_metadata(song, state.duration, cover_art_path);
                        }

                        // Update tray with song info
                        if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
                            tray_manager.update_song_info(&song.title, &song.artist);
                        }
                    }
                    last_song_id = current_song_id;
                }

                // Update playback status when it changes
                if state.is_playing != last_is_playing {
                    if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>() {
                        media_controls.set_playback_status(state.is_playing, state.position);
                    }

                    // Update tray with playback state
                    if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
                        tray_manager.update_playback_state(state.is_playing);
                    }

                    last_is_playing = state.is_playing;
                }

                // Throttled position updates to OS media controls (~1Hz instead of 10Hz)
                position_update_counter = (position_update_counter + 1) % 10;
                if position_update_counter == 0
                    && state.is_playing
                    && let Some(media_controls) = app_handle.try_state::<MediaControlsManager>()
                {
                    media_controls.set_playback_status(state.is_playing, state.position);
                }

                let _ = app_handle.emit("playback-state", &state);
            }
        });
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.command_tx.send(AudioCommand::Shutdown);
    }
}

/// Tracks crossfade timing state for volume ramping in the audio thread.
struct CrossfadeState {
    start: Instant,
    duration_secs: f64,
    /// Accumulated elapsed time before pauses
    elapsed_before_pause: f64,
    paused: bool,
}

impl CrossfadeState {
    fn new(duration_ms: u32) -> Self {
        Self {
            start: Instant::now(),
            duration_secs: duration_ms as f64 / 1000.0,
            elapsed_before_pause: 0.0,
            paused: false,
        }
    }

    /// Get crossfade progress from 0.0 to 1.0
    fn progress(&self) -> f64 {
        if self.paused {
            (self.elapsed_before_pause / self.duration_secs).min(1.0)
        } else {
            let elapsed = self.start.elapsed().as_secs_f64() + self.elapsed_before_pause;
            (elapsed / self.duration_secs).min(1.0)
        }
    }

    fn pause(&mut self) {
        if !self.paused {
            self.elapsed_before_pause += self.start.elapsed().as_secs_f64();
            self.paused = true;
        }
    }

    fn resume(&mut self) {
        if self.paused {
            self.start = Instant::now();
            self.paused = false;
        }
    }

    fn is_complete(&self) -> bool {
        self.progress() >= 1.0
    }
}

fn append_processed_source<S>(
    sink: &Player,
    source: S,
    normalization_gain: Option<f32>,
    dynamics_preset: Option<&DynamicsPreset>,
    binaural_preset: Option<&BinauralPreset>,
    equalizer_settings: Option<&EqualizerSettings>,
) where
    S: Source<Item = f32> + Send + 'static,
{
    let gain = normalization_gain.unwrap_or(1.0);
    let use_eq = equalizer_settings.is_some_and(|eq| !eq.is_flat());

    match (dynamics_preset, binaural_preset, use_eq) {
        (Some(dynamics), Some(binaural), true) => {
            let normalizing_source = NormalizingSource::with_clamp(source, gain, false);
            let dynamics_source = DynamicsSource::new(normalizing_source, dynamics);
            let equalizer_source =
                EqualizerSource::new(dynamics_source, equalizer_settings.expect("eq checked"));
            let binaural_source = BinauralSource::new(equalizer_source, binaural);
            sink.append(binaural_source);
        }
        (Some(dynamics), Some(binaural), false) => {
            let normalizing_source = NormalizingSource::with_clamp(source, gain, false);
            let dynamics_source = DynamicsSource::new(normalizing_source, dynamics);
            let binaural_source = BinauralSource::new(dynamics_source, binaural);
            sink.append(binaural_source);
        }
        (Some(dynamics), None, true) => {
            let normalizing_source = NormalizingSource::with_clamp(source, gain, false);
            let dynamics_source = DynamicsSource::new(normalizing_source, dynamics);
            let equalizer_source =
                EqualizerSource::new(dynamics_source, equalizer_settings.expect("eq checked"));
            sink.append(equalizer_source);
        }
        (Some(dynamics), None, false) => {
            let normalizing_source = NormalizingSource::with_clamp(source, gain, false);
            let dynamics_source = DynamicsSource::new(normalizing_source, dynamics);
            sink.append(dynamics_source);
        }
        (None, Some(binaural), true) => {
            let normalizing_source = NormalizingSource::new(source, gain);
            let equalizer_source =
                EqualizerSource::new(normalizing_source, equalizer_settings.expect("eq checked"));
            let binaural_source = BinauralSource::new(equalizer_source, binaural);
            sink.append(binaural_source);
        }
        (None, Some(binaural), false) => {
            let normalizing_source = NormalizingSource::new(source, gain);
            let binaural_source = BinauralSource::new(normalizing_source, binaural);
            sink.append(binaural_source);
        }
        (None, None, true) => {
            let normalizing_source = NormalizingSource::new(source, gain);
            let equalizer_source =
                EqualizerSource::new(normalizing_source, equalizer_settings.expect("eq checked"));
            sink.append(equalizer_source);
        }
        (None, None, false) => {
            let normalizing_source = NormalizingSource::new(source, gain);
            sink.append(normalizing_source);
        }
    }
}

fn build_track(
    audio_data: Vec<u8>,
    metadata: SongMetadata,
    duration_secs: f64,
    normalization_gain: Option<f32>,
    dynamics_preset: Option<DynamicsPreset>,
    binaural_preset: Option<BinauralPreset>,
    equalizer_settings: Option<EqualizerSettings>,
) -> BufferedTrack {
    BufferedTrack {
        audio_data,
        metadata,
        duration_secs,
        normalization_gain,
        dynamics_preset,
        binaural_preset,
        equalizer_settings,
    }
}

fn create_player_for_track(
    stream: &rodio::stream::MixerDeviceSink,
    track: &BufferedTrack,
    volume: f32,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
) -> AppResult<Player> {
    let byte_len = track.audio_data.len() as u64;
    let cursor = Cursor::new(track.audio_data.clone());
    let source = Decoder::builder()
        .with_data(cursor)
        .with_byte_len(byte_len)
        .with_coarse_seek(true)
        .build()
        .map_err(|e| AppError::Audio(format!("Failed to decode audio: {e}")))?;

    let analyzing_source = AnalyzingSource::new(source, Arc::clone(spectrum_producer));
    let sink = Player::connect_new(stream.mixer());
    sink.set_volume(volume);

    append_processed_source(
        &sink,
        analyzing_source,
        track.normalization_gain,
        track.dynamics_preset.as_ref(),
        track.binaural_preset.as_ref(),
        track.equalizer_settings.as_ref(),
    );

    Ok(sink)
}

fn update_track_state_after_start(
    shared_state: &Arc<SharedState>,
    track: &BufferedTrack,
    position_secs: f64,
    was_playing: bool,
) {
    {
        let mut inner = shared_state.write_inner();
        inner.current_song = Some(track.metadata.clone());
        inner.current_track = Some(track.clone());
        inner.playback_start = was_playing.then(Instant::now);
        inner.paused_position = position_secs;
        inner.duration = track.duration_secs;
        inner.gapless_segments = vec![GaplessSegment {
            track: track.clone(),
            cumulative_start: 0.0,
        }];
    }
    shared_state.is_playing.store(was_playing, Ordering::SeqCst);
}

fn restore_snapshot_on_stream(
    stream: &rodio::stream::MixerDeviceSink,
    snapshot: &PlaybackSnapshot,
    shared_state: &Arc<SharedState>,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
) -> AppResult<Player> {
    let sink = create_player_for_track(
        stream,
        &snapshot.track,
        shared_state.read_inner().volume,
        spectrum_producer,
    )?;

    if !snapshot.was_playing {
        sink.pause();
    }

    if snapshot.position_secs > 0.0
        && let Err(e) = sink.try_seek(Duration::from_secs_f64(snapshot.position_secs))
    {
        warn!("Seek failed while restoring playback on a new output device: {e:?}");
    }

    update_track_state_after_start(
        shared_state,
        &snapshot.track,
        snapshot.position_secs,
        snapshot.was_playing,
    );

    Ok(sink)
}

fn current_playback_snapshot(shared_state: &Arc<SharedState>) -> Option<PlaybackSnapshot> {
    let (state, segment_idx) = shared_state.get_gapless_state();
    let track = {
        let inner = shared_state.read_inner();
        if inner.gapless_segments.len() > 1 {
            inner
                .gapless_segments
                .get(segment_idx)
                .map(|segment| segment.track.clone())
        } else {
            inner.current_track.clone()
        }
    }?;

    Some(PlaybackSnapshot {
        track,
        position_secs: state.position,
        was_playing: state.is_playing,
    })
}

fn stream_error_callback(
    command_tx: Sender<AudioCommand>,
) -> impl FnMut(rodio::cpal::StreamError) + Send + Clone + 'static {
    move |err| {
        warn!("Audio output stream error: {err}");
        let _ = command_tx.send(AudioCommand::RecoverOutputStream);
    }
}

fn open_preferred_stream(
    preferred_device_id: Option<&str>,
    command_tx: &Sender<AudioCommand>,
) -> AppResult<(rodio::stream::MixerDeviceSink, AudioOutputRouteState)> {
    let mut stream = output::open_output_stream(
        preferred_device_id,
        stream_error_callback(command_tx.clone()),
    )?;
    stream.0.log_on_drop(false);
    Ok(stream)
}

#[allow(clippy::too_many_arguments)]
fn switch_output_stream(
    preferred_device_id: Option<String>,
    command_tx: &Sender<AudioCommand>,
    shared_state: &Arc<SharedState>,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
    stream: &mut Option<rodio::stream::MixerDeviceSink>,
    current_sink: &mut Option<Player>,
    crossfade_sink: &mut Option<Player>,
    crossfade_state: &mut Option<CrossfadeState>,
) -> AppResult<()> {
    {
        let mut inner = shared_state.write_inner();
        inner.preferred_output_device_id = preferred_device_id.clone();
    }

    let snapshot = current_playback_snapshot(shared_state);
    let (new_stream, output_route) =
        open_preferred_stream(preferred_device_id.as_deref(), command_tx)?;

    let new_sink = if let Some(snapshot) = snapshot.as_ref() {
        Some(restore_snapshot_on_stream(
            &new_stream,
            snapshot,
            shared_state,
            spectrum_producer,
        )?)
    } else {
        None
    };

    if let Some(sink) = current_sink.take() {
        sink.stop();
    }
    if let Some(sink) = crossfade_sink.take() {
        sink.stop();
    }

    *crossfade_state = None;
    *stream = Some(new_stream);
    *current_sink = new_sink;
    shared_state
        .crossfade_initiated
        .store(false, Ordering::SeqCst);
    shared_state.write_inner().output_route = output_route;

    Ok(())
}

/// Main audio thread function
fn run_audio_thread(
    command_rx: Receiver<AudioCommand>,
    command_tx: Sender<AudioCommand>,
    shared_state: Arc<SharedState>,
    spectrum_producer: Arc<Mutex<HeapProd<f32>>>,
) {
    let mut stream = match open_preferred_stream(None, &command_tx) {
        Ok((stream, output_route)) => {
            shared_state.write_inner().output_route = output_route;
            Some(stream)
        }
        Err(e) => {
            warn!("Failed to initialize audio output stream: {e}");
            None
        }
    };

    let mut current_sink: Option<Player> = None;
    let mut crossfade_sink: Option<Player> = None;
    let mut crossfade_state: Option<CrossfadeState> = None;
    #[cfg(target_os = "macos")]
    let mut default_output_poll_ticks: u8 = 0;
    #[cfg(target_os = "macos")]
    let mut last_default_output_device_id = output::current_default_output_device_id();

    loop {
        // Use recv_timeout to allow periodic checks
        match command_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(command) => match command {
                AudioCommand::Play {
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    equalizer_settings,
                } => {
                    if stream.is_none() {
                        let preferred_id =
                            shared_state.read_inner().preferred_output_device_id.clone();
                        if let Err(e) = switch_output_stream(
                            preferred_id,
                            &command_tx,
                            &shared_state,
                            &spectrum_producer,
                            &mut stream,
                            &mut current_sink,
                            &mut crossfade_sink,
                            &mut crossfade_state,
                        ) {
                            error!("Failed to open audio output stream: {e}");
                            continue;
                        }
                    }

                    // Stop any existing playback including crossfade
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }
                    if let Some(cf_sink) = crossfade_sink.take() {
                        cf_sink.stop();
                    }
                    crossfade_state = None;
                    shared_state
                        .crossfade_initiated
                        .store(false, Ordering::SeqCst);

                    let track = build_track(
                        audio_data,
                        metadata,
                        duration_secs,
                        normalization_gain,
                        dynamics_preset,
                        binaural_preset,
                        equalizer_settings,
                    );

                    match stream.as_ref().and_then(|stream| {
                        create_player_for_track(
                            stream,
                            &track,
                            shared_state.read_inner().volume,
                            &spectrum_producer,
                        )
                        .ok()
                    }) {
                        Some(sink) => {
                            update_track_state_after_start(&shared_state, &track, 0.0, true);
                            current_sink = Some(sink);
                        }
                        None => {
                            error!("Failed to decode audio for playback");
                        }
                    }
                }
                AudioCommand::Pause => {
                    if let Some(ref sink) = current_sink
                        && !sink.is_paused()
                    {
                        // Save current position (single lock acquisition)
                        {
                            let mut inner = shared_state.write_inner();
                            if let Some(instant) = inner.playback_start {
                                let elapsed =
                                    instant.elapsed().as_secs_f64() + inner.paused_position;
                                inner.paused_position = elapsed;
                            }
                        }

                        sink.pause();

                        // Also pause crossfade sink and freeze timer
                        if let Some(ref cf_sink) = crossfade_sink {
                            cf_sink.pause();
                        }
                        if let Some(ref mut cf_state) = crossfade_state {
                            cf_state.pause();
                        }

                        shared_state.is_playing.store(false, Ordering::SeqCst);
                    }
                }
                AudioCommand::Resume => {
                    if let Some(ref sink) = current_sink
                        && sink.is_paused()
                    {
                        shared_state.write_inner().playback_start = Some(Instant::now());
                        sink.play();

                        // Also resume crossfade sink and timer
                        if let Some(ref cf_sink) = crossfade_sink {
                            cf_sink.play();
                        }
                        if let Some(ref mut cf_state) = crossfade_state {
                            cf_state.resume();
                        }

                        shared_state.is_playing.store(true, Ordering::SeqCst);
                    }
                }
                AudioCommand::Stop => {
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }
                    if let Some(cf_sink) = crossfade_sink.take() {
                        cf_sink.stop();
                    }
                    crossfade_state = None;
                    shared_state
                        .crossfade_initiated
                        .store(false, Ordering::SeqCst);
                    shared_state.is_playing.store(false, Ordering::SeqCst);
                    // Reset all state in single lock acquisition
                    {
                        let mut inner = shared_state.write_inner();
                        inner.current_song = None;
                        inner.current_track = None;
                        inner.playback_start = None;
                        inner.paused_position = 0.0;
                        inner.duration = 0.0;
                        inner.gapless_segments.clear();
                    }
                }
                AudioCommand::SetVolume(volume) => {
                    // During crossfade, let the idle loop handle proportional volumes
                    if crossfade_state.is_none()
                        && let Some(ref sink) = current_sink
                    {
                        sink.set_volume(volume);
                    }
                }
                AudioCommand::Seek(position_secs) => {
                    // If crossfade is active, abort it and restore full volume
                    if crossfade_state.is_some() {
                        if let Some(cf_sink) = crossfade_sink.take() {
                            cf_sink.stop();
                        }
                        crossfade_state = None;
                        let vol = shared_state.read_inner().volume;
                        if let Some(ref sink) = current_sink {
                            sink.set_volume(vol);
                        }
                    }

                    if let Some(ref sink) = current_sink {
                        // Frontend sends segment-relative position (from get_gapless_state).
                        // sink.try_seek() operates on the currently-playing Rodio source,
                        // so we pass the segment-relative position directly.
                        // But paused_position must be cumulative for calculate_position().
                        let (seek_pos, cumulative_pos) = {
                            let inner = shared_state.read_inner();
                            if inner.gapless_segments.len() > 1 {
                                // Find current segment from cumulative position
                                let cumulative = shared_state.calculate_position(&inner);
                                let mut seg_idx = 0;
                                for (i, seg) in inner.gapless_segments.iter().enumerate() {
                                    if cumulative < seg.cumulative_start + seg.track.duration_secs {
                                        seg_idx = i;
                                        break;
                                    }
                                    seg_idx = i;
                                }
                                let seg = &inner.gapless_segments[seg_idx];
                                let clamped = position_secs.clamp(0.0, seg.track.duration_secs);
                                (clamped, seg.cumulative_start + clamped)
                            } else {
                                let clamped = position_secs.clamp(0.0, inner.duration);
                                (clamped, clamped)
                            }
                        };
                        let seek_duration = Duration::from_secs_f64(seek_pos);
                        if let Err(e) = sink.try_seek(seek_duration) {
                            warn!("Seek failed: {:?}", e);
                        } else {
                            let mut inner = shared_state.write_inner();
                            inner.paused_position = cumulative_pos;
                            inner.playback_start = Some(Instant::now());
                        }
                    }
                }
                AudioCommand::AppendGapless {
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    equalizer_settings,
                } => {
                    if let Some(ref sink) = current_sink {
                        let track = build_track(
                            audio_data,
                            metadata,
                            duration_secs,
                            normalization_gain,
                            dynamics_preset,
                            binaural_preset,
                            equalizer_settings,
                        );

                        match create_player_for_track(
                            stream
                                .as_ref()
                                .expect("stream checked before gapless append"),
                            &track,
                            0.0,
                            &spectrum_producer,
                        ) {
                            Ok(decoded_sink) => {
                                decoded_sink.stop();

                                let byte_len = track.audio_data.len() as u64;
                                let cursor = Cursor::new(track.audio_data.clone());
                                match Decoder::builder()
                                    .with_data(cursor)
                                    .with_byte_len(byte_len)
                                    .with_coarse_seek(true)
                                    .build()
                                {
                                    Ok(source) => {
                                        let analyzing_source = AnalyzingSource::new(
                                            source,
                                            Arc::clone(&spectrum_producer),
                                        );
                                        append_processed_source(
                                            sink,
                                            analyzing_source,
                                            track.normalization_gain,
                                            track.dynamics_preset.as_ref(),
                                            track.binaural_preset.as_ref(),
                                            track.equalizer_settings.as_ref(),
                                        );

                                        {
                                            let mut inner = shared_state.write_inner();
                                            let cumulative_start = inner
                                                .gapless_segments
                                                .last()
                                                .map(|s| s.cumulative_start + s.track.duration_secs)
                                                .unwrap_or(inner.duration);
                                            inner.gapless_segments.push(GaplessSegment {
                                                track,
                                                cumulative_start,
                                            });
                                            inner.duration = cumulative_start + duration_secs;
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to decode gapless audio: {:?}", e);
                                    }
                                }
                            }
                            Err(e) => error!("Failed to prepare gapless audio: {e}"),
                        }
                    }
                }
                AudioCommand::CrossfadePlay {
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    equalizer_settings,
                    crossfade_duration_ms,
                } => {
                    // Stop any previous crossfade that's still running
                    if let Some(old_cf_sink) = crossfade_sink.take() {
                        old_cf_sink.stop();
                    }
                    // Move current sink to crossfade_sink (it keeps playing, fading out)
                    crossfade_sink = current_sink.take();

                    let track = build_track(
                        audio_data,
                        metadata,
                        duration_secs,
                        normalization_gain,
                        dynamics_preset,
                        binaural_preset,
                        equalizer_settings,
                    );

                    match stream.as_ref().and_then(|stream| {
                        create_player_for_track(stream, &track, 0.0, &spectrum_producer).ok()
                    }) {
                        Some(new_sink) => {
                            update_track_state_after_start(&shared_state, &track, 0.0, true);
                            shared_state
                                .crossfade_initiated
                                .store(false, Ordering::SeqCst);

                            current_sink = Some(new_sink);
                            crossfade_state = Some(CrossfadeState::new(crossfade_duration_ms));
                        }
                        None => {
                            error!("Failed to decode crossfade audio");
                            current_sink = crossfade_sink.take();
                        }
                    }
                }
                AudioCommand::SetOutputDevice {
                    preferred_device_id,
                    result_tx,
                } => {
                    let result = switch_output_stream(
                        preferred_device_id,
                        &command_tx,
                        &shared_state,
                        &spectrum_producer,
                        &mut stream,
                        &mut current_sink,
                        &mut crossfade_sink,
                        &mut crossfade_state,
                    )
                    .map_err(|e| {
                        warn!("Failed to switch audio output device: {e}");
                        e.to_string()
                    });
                    let _ = result_tx.send(result);
                }
                AudioCommand::RecoverOutputStream => {
                    let preferred_id = shared_state.read_inner().preferred_output_device_id.clone();
                    if let Err(e) = switch_output_stream(
                        preferred_id,
                        &command_tx,
                        &shared_state,
                        &spectrum_producer,
                        &mut stream,
                        &mut current_sink,
                        &mut crossfade_sink,
                        &mut crossfade_state,
                    ) {
                        warn!("Failed to recover audio output stream: {e}");
                    }
                }
                AudioCommand::Shutdown => {
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }
                    if let Some(cf_sink) = crossfade_sink.take() {
                        cf_sink.stop();
                    }
                    break;
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                #[cfg(target_os = "macos")]
                {
                    default_output_poll_ticks = default_output_poll_ticks.wrapping_add(1);

                    if default_output_poll_ticks >= 20 {
                        default_output_poll_ticks = 0;

                        let current_default_output_device_id =
                            output::current_default_output_device_id();
                        if current_default_output_device_id != last_default_output_device_id {
                            last_default_output_device_id = current_default_output_device_id;

                            let (preferred_id, should_rebind_selected_output) = {
                                let inner = shared_state.read_inner();
                                (
                                    inner.preferred_output_device_id.clone(),
                                    inner.output_route.system_default_bound,
                                )
                            };

                            if should_rebind_selected_output
                                && let Some(preferred_id) = preferred_id
                                && let Err(e) = switch_output_stream(
                                    Some(preferred_id),
                                    &command_tx,
                                    &shared_state,
                                    &spectrum_producer,
                                    &mut stream,
                                    &mut current_sink,
                                    &mut crossfade_sink,
                                    &mut crossfade_state,
                                )
                            {
                                warn!(
                                    "Failed to rebind explicitly selected macOS output device after system default change: {e}"
                                );
                            }
                        }
                    }
                }

                // Crossfade volume ramping (~20Hz with 50ms timeout)
                if let Some(ref cf_state) = crossfade_state {
                    let progress = cf_state.progress();
                    let user_vol = shared_state.read_inner().volume;

                    // Equal-power crossfade curves
                    let fade_out_vol =
                        user_vol * ((progress * std::f64::consts::FRAC_PI_2).cos() as f32);
                    let fade_in_vol =
                        user_vol * ((progress * std::f64::consts::FRAC_PI_2).sin() as f32);

                    if let Some(ref old_sink) = crossfade_sink {
                        if old_sink.empty() {
                            // Old song ended before crossfade completed
                        } else {
                            old_sink.set_volume(fade_out_vol);
                        }
                    }
                    if let Some(ref cur) = current_sink {
                        cur.set_volume(fade_in_vol);
                    }

                    if cf_state.is_complete() || crossfade_sink.as_ref().is_some_and(|s| s.empty())
                    {
                        // Crossfade done: drop old sink, restore full volume
                        if let Some(cf_sink) = crossfade_sink.take() {
                            cf_sink.stop();
                        }
                        crossfade_state = None;
                        let vol = shared_state.read_inner().volume;
                        if let Some(ref cur) = current_sink {
                            cur.set_volume(vol);
                        }
                    }
                }

                // Check if current playback has ended
                if let Some(ref sink) = current_sink
                    && sink.empty()
                    && shared_state.is_playing.load(Ordering::SeqCst)
                {
                    // Set paused_position to duration so position emitter can detect end
                    {
                        let mut inner = shared_state.write_inner();
                        inner.paused_position = inner.duration;
                    }
                    shared_state.is_playing.store(false, Ordering::SeqCst);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }
}
