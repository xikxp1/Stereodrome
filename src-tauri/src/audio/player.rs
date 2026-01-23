use log::{error, warn};
use ringbuf::{traits::Split, HeapCons, HeapProd, HeapRb};
use rodio::{Decoder, OutputStreamBuilder, Sink};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::analyzer::AnalyzingSource;
use crate::error::{AppError, AppResult};
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
    },
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    Seek(f64),
    Shutdown,
}

/// Inner playback state consolidated into a single struct for efficient locking
struct PlaybackInner {
    current_song: Option<SongMetadata>,
    current_audio_data: Option<(Vec<u8>, u64)>, // (data, byte_len)
    volume: f32,
    playback_start: Option<Instant>,
    paused_position: f64,
    duration: f64,
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
        }
    }
}

/// State shared between the main thread and audio thread.
/// Uses a single RwLock for efficient concurrent reads (position emitter at 10Hz).
struct SharedState {
    is_playing: AtomicBool,
    inner: RwLock<PlaybackInner>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            is_playing: AtomicBool::new(false),
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

    fn get_state(&self) -> PlaybackState {
        let inner = self.read_inner();
        PlaybackState {
            is_playing: self.is_playing.load(Ordering::SeqCst),
            position: self.calculate_position(&inner),
            duration: inner.duration,
            volume: inner.volume,
            song: inner.current_song.clone(),
        }
    }

    fn get_status(&self) -> PlaybackStatus {
        let inner = self.read_inner();
        PlaybackStatus {
            is_playing: self.is_playing.load(Ordering::SeqCst),
            current_song_id: inner.current_song.as_ref().map(|s| s.id.clone()),
            position: self.calculate_position(&inner),
            duration: inner.duration,
            volume: inner.volume,
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
    ) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Play {
                audio_data,
                metadata,
                duration_secs,
            })
            .map_err(|e| AppError::Audio(format!("Failed to send play command: {}", e)))
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

            loop {
                thread::sleep(Duration::from_millis(100)); // 10Hz updates

                let state = shared_state.get_state();

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
                    }
                    continue;
                }

                // Check if playback ended naturally (position reached end)
                // This handles both cases:
                // 1. We catch it while is_playing is still true
                // 2. Audio thread already set is_playing to false but song still exists
                let playback_finished = state.duration > 0.0
                    && state.position >= state.duration - 0.2 // Small tolerance for timing
                    && state.song.is_some()
                    && !state.is_playing;

                if playback_finished {
                    // Clear the current song so frontend updates
                    {
                        let mut inner = shared_state.write_inner();
                        inner.current_song = None;
                        inner.paused_position = 0.0;
                        inner.duration = 0.0;
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

    loop {
        // Use recv_timeout to allow periodic checks
        match command_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(command) => match command {
                AudioCommand::Play {
                    audio_data,
                    metadata,
                    duration_secs,
                } => {
                    // Stop any existing playback
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }

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
                            sink.append(analyzing_source);

                            // Update shared state (single lock acquisition)
                            {
                                let mut inner = shared_state.write_inner();
                                inner.current_audio_data = Some((audio_data, byte_len));
                                inner.current_song = Some(metadata);
                                inner.playback_start = Some(Instant::now());
                                inner.paused_position = 0.0;
                                inner.duration = duration_secs;
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
                            shared_state.is_playing.store(false, Ordering::SeqCst);
                        }
                    }
                }
                AudioCommand::Resume => {
                    if let Some(ref sink) = current_sink {
                        if sink.is_paused() {
                            shared_state.write_inner().playback_start = Some(Instant::now());
                            sink.play();
                            shared_state.is_playing.store(true, Ordering::SeqCst);
                        }
                    }
                }
                AudioCommand::Stop => {
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }
                    shared_state.is_playing.store(false, Ordering::SeqCst);
                    // Reset all state in single lock acquisition
                    {
                        let mut inner = shared_state.write_inner();
                        inner.current_song = None;
                        inner.current_audio_data = None;
                        inner.playback_start = None;
                        inner.paused_position = 0.0;
                        inner.duration = 0.0;
                    }
                }
                AudioCommand::SetVolume(volume) => {
                    if let Some(ref sink) = current_sink {
                        sink.set_volume(volume);
                    }
                }
                AudioCommand::Seek(position_secs) => {
                    if let Some(ref sink) = current_sink {
                        let seek_duration = Duration::from_secs_f64(position_secs);
                        if let Err(e) = sink.try_seek(seek_duration) {
                            warn!("Seek failed: {:?}", e);
                        } else {
                            // Update position tracking (single lock acquisition)
                            let mut inner = shared_state.write_inner();
                            inner.paused_position = position_secs;
                            inner.playback_start = Some(Instant::now());
                        }
                    }
                }
                AudioCommand::Shutdown => {
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }
                    break;
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
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
