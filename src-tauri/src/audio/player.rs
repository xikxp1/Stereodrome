use log::{error, warn};
use ringbuf::{traits::Split, HeapCons, HeapProd, HeapRb};
use rodio::{Decoder, OutputStreamBuilder, Sink, Source};
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
use crate::audio::normalizer::NormalizingSource;
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
    },
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    Seek(f64),
    /// Append a song to the existing Sink for gapless playback.
    /// Unlike Play, this does NOT create a new Sink.
    AppendGapless {
        audio_data: Vec<u8>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
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
        crossfade_duration_ms: u32,
    },
    Shutdown,
}

/// A segment within a gapless playback chain.
/// Each segment represents one song appended to the same Rodio Sink.
#[derive(Debug, Clone)]
struct GaplessSegment {
    metadata: SongMetadata,
    duration: f64,         // this segment's duration in seconds
    cumulative_start: f64, // sum of all previous segments' durations
}

/// Inner playback state consolidated into a single struct for efficient locking
struct PlaybackInner {
    current_song: Option<SongMetadata>,
    current_audio_data: Option<(Vec<u8>, u64)>, // (data, byte_len)
    volume: f32,
    playback_start: Option<Instant>,
    paused_position: f64,
    duration: f64,
    /// Gapless playback segments. When multiple consecutive album tracks are
    /// appended to the same Sink, each gets a segment for position tracking.
    gapless_segments: Vec<GaplessSegment>,
}

impl Default for PlaybackInner {
    fn default() -> Self {
        Self {
            current_song: None,
            current_audio_data: None,
            volume: 0.8,
            playback_start: None,
            paused_position: 0.0,
            duration: 0.0,
            gapless_segments: Vec::new(),
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
                if cumulative_pos < seg.cumulative_start + seg.duration {
                    seg_idx = i;
                    break;
                }
                seg_idx = i; // default to last segment
            }
            let seg = &inner.gapless_segments[seg_idx];
            let song_pos = (cumulative_pos - seg.cumulative_start)
                .max(0.0)
                .min(seg.duration);

            (
                PlaybackState {
                    is_playing,
                    position: song_pos,
                    duration: seg.duration,
                    volume: inner.volume,
                    song: Some(seg.metadata.clone()),
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
        let audio_thread = thread::spawn(move || {
            run_audio_thread(command_rx, state_clone, producer_clone);
        });

        Ok(Self {
            command_tx,
            shared_state,
            spectrum_consumer,
            _audio_thread: audio_thread,
        })
    }

    pub fn play(
        &self,
        audio_data: Vec<u8>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
    ) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Play {
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
            })
            .map_err(|e| AppError::Audio(format!("Failed to send play command: {}", e)))
    }

    /// Append a song to the existing Sink for gapless playback.
    /// The song's audio pipeline is decoded and appended without stopping the current Sink.
    pub fn append_gapless(
        &self,
        audio_data: Vec<u8>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
    ) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::AppendGapless {
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
            })
            .map_err(|e| AppError::Audio(format!("Failed to send gapless command: {}", e)))
    }

    /// Start a crossfade transition: fade out current song while fading in a new one.
    pub fn crossfade_play(
        &self,
        audio_data: Vec<u8>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        crossfade_duration_ms: u32,
    ) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::CrossfadePlay {
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
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

    #[allow(dead_code)]
    pub fn current_song_id(&self) -> Option<String> {
        self.shared_state
            .read_inner()
            .current_song
            .as_ref()
            .map(|s| s.id.clone())
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
                if let Ok(mut cons) = consumer.try_lock() {
                    if let Some(spectrum_data) = analyzer.process(&mut cons) {
                        let _ = app_handle.emit("spectrum-data", spectrum_data);
                    }
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

                // Detect gapless segment transition (song changed within the same Sink)
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
                if position_update_counter == 0 && state.is_playing {
                    if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>() {
                        media_controls.set_playback_status(state.is_playing, state.position);
                    }
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
    sink: &Sink,
    source: S,
    normalization_gain: Option<f32>,
    dynamics_preset: Option<&DynamicsPreset>,
    binaural_preset: Option<&BinauralPreset>,
) where
    S: Source<Item = f32> + Send + 'static,
{
    let gain = normalization_gain.unwrap_or(1.0);

    match (dynamics_preset, binaural_preset) {
        (Some(dynamics), Some(binaural)) => {
            let normalizing_source = NormalizingSource::with_clamp(source, gain, false);
            let dynamics_source = DynamicsSource::new(normalizing_source, dynamics);
            let binaural_source = BinauralSource::new(dynamics_source, binaural);
            sink.append(binaural_source);
        }
        (Some(dynamics), None) => {
            let normalizing_source = NormalizingSource::with_clamp(source, gain, false);
            let dynamics_source = DynamicsSource::new(normalizing_source, dynamics);
            sink.append(dynamics_source);
        }
        (None, Some(binaural)) => {
            let normalizing_source = NormalizingSource::new(source, gain);
            let binaural_source = BinauralSource::new(normalizing_source, binaural);
            sink.append(binaural_source);
        }
        (None, None) => {
            let normalizing_source = NormalizingSource::new(source, gain);
            sink.append(normalizing_source);
        }
    }
}

/// Main audio thread function
fn run_audio_thread(
    command_rx: Receiver<AudioCommand>,
    shared_state: Arc<SharedState>,
    spectrum_producer: Arc<Mutex<HeapProd<f32>>>,
) {
    // Open the default audio output stream
    let stream = match OutputStreamBuilder::open_default_stream() {
        Ok(mut s) => {
            s.log_on_drop(false);
            s
        }
        Err(e) => {
            error!("Failed to open audio stream: {:?}", e);
            return;
        }
    };

    let mut current_sink: Option<Sink> = None;
    let mut crossfade_sink: Option<Sink> = None;
    let mut crossfade_state: Option<CrossfadeState> = None;

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
                } => {
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

                    // Decode and play with coarse seek enabled for better seeking
                    let byte_len = audio_data.len() as u64;
                    let cursor = Cursor::new(audio_data.clone());
                    match Decoder::builder()
                        .with_data(cursor)
                        .with_byte_len(byte_len)
                        .with_coarse_seek(true)
                        .build()
                    {
                        Ok(source) => {
                            // Wrap source with analyzer for spectrum analysis
                            // Rodio 0.21+ uses f32 samples natively
                            let analyzing_source =
                                AnalyzingSource::new(source, Arc::clone(&spectrum_producer));

                            let sink = Sink::connect_new(stream.mixer());
                            let volume = shared_state.read_inner().volume;
                            sink.set_volume(volume);

                            append_processed_source(
                                &sink,
                                analyzing_source,
                                normalization_gain,
                                dynamics_preset.as_ref(),
                                binaural_preset.as_ref(),
                            );

                            // Update shared state (single lock acquisition)
                            {
                                let mut inner = shared_state.write_inner();
                                inner.current_audio_data = Some((audio_data, byte_len));
                                inner.current_song = Some(metadata.clone());
                                inner.playback_start = Some(Instant::now());
                                inner.paused_position = 0.0;
                                inner.duration = duration_secs;
                                // Initialize gapless segments with this first song
                                inner.gapless_segments = vec![GaplessSegment {
                                    metadata,
                                    duration: duration_secs,
                                    cumulative_start: 0.0,
                                }];
                            }
                            shared_state.is_playing.store(true, Ordering::SeqCst);

                            current_sink = Some(sink);
                        }
                        Err(e) => {
                            error!("Failed to decode audio: {:?}", e);
                        }
                    }
                }
                AudioCommand::Pause => {
                    if let Some(ref sink) = current_sink {
                        if !sink.is_paused() {
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
                }
                AudioCommand::Resume => {
                    if let Some(ref sink) = current_sink {
                        if sink.is_paused() {
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
                        inner.current_audio_data = None;
                        inner.playback_start = None;
                        inner.paused_position = 0.0;
                        inner.duration = 0.0;
                        inner.gapless_segments.clear();
                    }
                }
                AudioCommand::SetVolume(volume) => {
                    // During crossfade, let the idle loop handle proportional volumes
                    if crossfade_state.is_none() {
                        if let Some(ref sink) = current_sink {
                            sink.set_volume(volume);
                        }
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
                                    if cumulative < seg.cumulative_start + seg.duration {
                                        seg_idx = i;
                                        break;
                                    }
                                    seg_idx = i;
                                }
                                let seg = &inner.gapless_segments[seg_idx];
                                let clamped = position_secs.clamp(0.0, seg.duration);
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
                } => {
                    if let Some(ref sink) = current_sink {
                        let byte_len = audio_data.len() as u64;
                        let cursor = Cursor::new(audio_data);
                        match Decoder::builder()
                            .with_data(cursor)
                            .with_byte_len(byte_len)
                            .with_coarse_seek(true)
                            .build()
                        {
                            Ok(source) => {
                                let analyzing_source =
                                    AnalyzingSource::new(source, Arc::clone(&spectrum_producer));

                                append_processed_source(
                                    sink,
                                    analyzing_source,
                                    normalization_gain,
                                    dynamics_preset.as_ref(),
                                    binaural_preset.as_ref(),
                                );

                                // Add gapless segment and update total duration
                                {
                                    let mut inner = shared_state.write_inner();
                                    let cumulative_start = inner
                                        .gapless_segments
                                        .last()
                                        .map(|s| s.cumulative_start + s.duration)
                                        .unwrap_or(inner.duration);
                                    inner.gapless_segments.push(GaplessSegment {
                                        metadata,
                                        duration: duration_secs,
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
                }
                AudioCommand::CrossfadePlay {
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    crossfade_duration_ms,
                } => {
                    // Stop any previous crossfade that's still running
                    if let Some(old_cf_sink) = crossfade_sink.take() {
                        old_cf_sink.stop();
                    }
                    // Move current sink to crossfade_sink (it keeps playing, fading out)
                    crossfade_sink = current_sink.take();

                    let byte_len = audio_data.len() as u64;
                    let cursor = Cursor::new(audio_data.clone());
                    match Decoder::builder()
                        .with_data(cursor)
                        .with_byte_len(byte_len)
                        .with_coarse_seek(true)
                        .build()
                    {
                        Ok(source) => {
                            let analyzing_source =
                                AnalyzingSource::new(source, Arc::clone(&spectrum_producer));

                            let new_sink = Sink::connect_new(stream.mixer());
                            new_sink.set_volume(0.0); // Start silent, will ramp up

                            append_processed_source(
                                &new_sink,
                                analyzing_source,
                                normalization_gain,
                                dynamics_preset.as_ref(),
                                binaural_preset.as_ref(),
                            );

                            // Update shared state for the new song
                            {
                                let mut inner = shared_state.write_inner();
                                inner.current_audio_data = Some((audio_data, byte_len));
                                inner.current_song = Some(metadata.clone());
                                inner.playback_start = Some(Instant::now());
                                inner.paused_position = 0.0;
                                inner.duration = duration_secs;
                                inner.gapless_segments = vec![GaplessSegment {
                                    metadata,
                                    duration: duration_secs,
                                    cumulative_start: 0.0,
                                }];
                            }
                            shared_state.is_playing.store(true, Ordering::SeqCst);
                            shared_state
                                .crossfade_initiated
                                .store(false, Ordering::SeqCst);

                            current_sink = Some(new_sink);
                            crossfade_state = Some(CrossfadeState::new(crossfade_duration_ms));
                        }
                        Err(e) => {
                            error!("Failed to decode crossfade audio: {:?}", e);
                            // Restore original sink if decode fails
                            current_sink = crossfade_sink.take();
                        }
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
                if let Some(ref sink) = current_sink {
                    if sink.empty() && shared_state.is_playing.load(Ordering::SeqCst) {
                        // Set paused_position to duration so position emitter can detect end
                        {
                            let mut inner = shared_state.write_inner();
                            inner.paused_position = inner.duration;
                        }
                        shared_state.is_playing.store(false, Ordering::SeqCst);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }
}
