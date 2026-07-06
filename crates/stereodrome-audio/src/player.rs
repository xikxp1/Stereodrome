use log::{debug, error, info, warn};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::Split};
use rodio::{Decoder, DeviceSinkBuilder, Player, Source};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::analyzer::AnalyzingSource;
use crate::binaural::{BinauralPreset, BinauralSource};
use crate::compressor::DynamicsPreset;
use crate::dynamics::DynamicsSource;
use crate::equalizer::{EqualizerSettings, EqualizerSource};
use crate::error::{AudioError, AudioResult};
use crate::normalizer::NormalizingSource;

/// Ring buffer size for spectrum analysis (~370ms at 44.1kHz stereo)
/// Larger buffer prevents sample loss from lock contention
const SPECTRUM_BUFFER_SIZE: usize = 32768;

/// How long transport commands (pause/resume/stop/seek) wait for the audio
/// thread to apply them. Status reads issued right after these calls drive
/// the native media controls, so the state change must be visible before
/// the call returns. Bounded so a wedged audio thread cannot hang callers.
const TRANSPORT_ACK_TIMEOUT: Duration = Duration::from_millis(1000);

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
        audio_data: Arc<[u8]>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    },
    Pause {
        ack: Sender<()>,
    },
    Resume {
        ack: Sender<()>,
    },
    Stop {
        ack: Sender<()>,
    },
    SetVolume(f32),
    Seek {
        position_secs: f64,
        ack: Sender<()>,
    },
    /// Append a song to the existing player for gapless playback.
    /// Unlike Play, this does NOT create a new player.
    AppendGapless {
        audio_data: Arc<[u8]>,
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
        audio_data: Arc<[u8]>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
        crossfade_duration_ms: u32,
        ack: Sender<AudioResult<()>>,
    },
    Shutdown,
}

#[derive(Debug)]
pub struct CrossfadePlayRequest {
    pub audio_data: Arc<[u8]>,
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
struct GaplessSegment {
    metadata: SongMetadata,
    duration: f64,         // this segment's duration in seconds
    cumulative_start: f64, // sum of all previous segments' durations
}

/// Inner playback state consolidated into a single struct for efficient locking
struct PlaybackInner {
    current_song: Option<SongMetadata>,
    volume: f32,
    playback_start: Option<Instant>,
    paused_position: f64,
    duration: f64,
    /// Gapless playback segments. When multiple consecutive album tracks are
    /// appended to the same player, each gets a segment for position tracking.
    gapless_segments: Vec<GaplessSegment>,
}

impl Default for PlaybackInner {
    fn default() -> Self {
        Self {
            current_song: None,
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
    spectrum_enabled: Arc<AtomicBool>,
    _audio_thread: JoinHandle<()>,
}

#[derive(Clone)]
pub struct AudioStateHandle {
    shared_state: Arc<SharedState>,
}

impl AudioStateHandle {
    pub fn get_gapless_state(&self) -> (PlaybackState, usize) {
        self.shared_state.get_gapless_state()
    }

    pub fn is_crossfade_initiated(&self) -> bool {
        self.shared_state.crossfade_initiated.load(Ordering::SeqCst)
    }

    pub fn set_crossfade_initiated(&self, value: bool) {
        self.shared_state
            .crossfade_initiated
            .store(value, Ordering::SeqCst);
    }

    pub fn is_last_gapless_segment(&self, segment_idx: usize) -> bool {
        let inner = self.shared_state.read_inner();
        inner.gapless_segments.len() <= 1 || segment_idx == inner.gapless_segments.len() - 1
    }

    pub fn is_playing(&self) -> bool {
        self.shared_state.is_playing.load(Ordering::SeqCst)
    }

    pub fn clear_finished_state(&self) {
        let mut inner = self.shared_state.write_inner();
        inner.current_song = None;
        inner.paused_position = 0.0;
        inner.duration = 0.0;
        inner.gapless_segments.clear();
    }
}

// Implement Send + Sync manually since we use channels for thread communication
unsafe impl Send for AudioPlayer {}
unsafe impl Sync for AudioPlayer {}

impl AudioPlayer {
    pub fn new() -> AudioResult<Self> {
        Self::new_with_spectrum(true)
    }

    pub fn new_with_spectrum(spectrum_enabled: bool) -> AudioResult<Self> {
        let (command_tx, command_rx) = mpsc::channel::<AudioCommand>();
        let shared_state = Arc::new(SharedState::new());
        let state_clone = Arc::clone(&shared_state);
        let spectrum_enabled = Arc::new(AtomicBool::new(spectrum_enabled));

        // Create ring buffer for spectrum analysis
        let ring_buffer = HeapRb::<f32>::new(SPECTRUM_BUFFER_SIZE);
        let (producer, consumer) = ring_buffer.split();
        let spectrum_producer = Arc::new(Mutex::new(producer));
        let spectrum_consumer = Arc::new(Mutex::new(consumer));

        let producer_clone = Arc::clone(&spectrum_producer);
        let spectrum_enabled_clone = Arc::clone(&spectrum_enabled);
        let audio_thread = thread::spawn(move || {
            run_audio_thread(
                command_rx,
                state_clone,
                producer_clone,
                spectrum_enabled_clone,
            );
        });

        Ok(Self {
            command_tx,
            shared_state,
            spectrum_consumer,
            spectrum_enabled,
            _audio_thread: audio_thread,
        })
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
    ) -> AudioResult<()> {
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
            .map_err(|e| AudioError::Playback(format!("Failed to send play command: {}", e)))
    }

    /// Append a song to the existing player for gapless playback.
    /// The song's audio pipeline is decoded and appended without stopping the current player.
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
    ) -> AudioResult<()> {
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
            .map_err(|e| AudioError::Playback(format!("Failed to send gapless command: {}", e)))
    }

    /// Start a crossfade transition: fade out current song while fading in a new one.
    pub fn crossfade_play(&self, request: CrossfadePlayRequest) -> AudioResult<()> {
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

        let (ack_tx, ack_rx) = mpsc::channel();
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
                ack: ack_tx,
            })
            .map_err(|e| {
                AudioError::Playback(format!("Failed to send crossfade command: {}", e))
            })?;
        match ack_rx.recv_timeout(TRANSPORT_ACK_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AudioError::Playback(
                "Audio thread did not acknowledge crossfade command in time".to_string(),
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AudioError::Playback(
                "Audio thread disconnected while starting crossfade".to_string(),
            )),
        }
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

    /// Send a transport command and wait until the audio thread has applied
    /// it, so that `get_status()` immediately afterwards reflects the change.
    fn send_transport_command(
        &self,
        name: &str,
        make_command: impl FnOnce(Sender<()>) -> AudioCommand,
    ) -> AudioResult<()> {
        let (ack_tx, ack_rx) = mpsc::channel();
        self.command_tx
            .send(make_command(ack_tx))
            .map_err(|e| AudioError::Playback(format!("Failed to send {name} command: {e}")))?;
        match ack_rx.recv_timeout(TRANSPORT_ACK_TIMEOUT) {
            Ok(()) => Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AudioError::Playback(format!(
                "Audio thread did not acknowledge {name} command in time"
            ))),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AudioError::Playback(format!(
                "Audio thread disconnected while applying {name} command"
            ))),
        }
    }

    pub fn pause(&self) -> AudioResult<()> {
        self.send_transport_command("pause", |ack| AudioCommand::Pause { ack })
    }

    pub fn resume(&self) -> AudioResult<()> {
        self.send_transport_command("resume", |ack| AudioCommand::Resume { ack })
    }

    pub fn stop(&self) -> AudioResult<()> {
        self.send_transport_command("stop", |ack| AudioCommand::Stop { ack })
    }

    pub fn set_volume(&self, volume: f32) -> AudioResult<()> {
        let clamped = volume.clamp(0.0, 1.0);
        self.shared_state.write_inner().volume = clamped;
        self.command_tx
            .send(AudioCommand::SetVolume(clamped))
            .map_err(|e| AudioError::Playback(format!("Failed to send volume command: {}", e)))
    }

    pub fn seek(&self, position_secs: f64) -> AudioResult<()> {
        let duration = self.shared_state.read_inner().duration;
        let clamped = position_secs.clamp(0.0, duration);
        self.send_transport_command("seek", |ack| AudioCommand::Seek {
            position_secs: clamped,
            ack,
        })
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

    pub fn get_playback_state(&self) -> PlaybackState {
        self.shared_state.get_gapless_state().0
    }

    pub fn get_gapless_state(&self) -> (PlaybackState, usize) {
        self.shared_state.get_gapless_state()
    }

    pub fn state_handle(&self) -> AudioStateHandle {
        AudioStateHandle {
            shared_state: Arc::clone(&self.shared_state),
        }
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

    pub fn set_spectrum_enabled(&self, enabled: bool) {
        self.spectrum_enabled.store(enabled, Ordering::SeqCst);
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

#[allow(clippy::too_many_arguments)]
fn append_output_source<S>(
    sink: &Player,
    source: S,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: &Arc<AtomicBool>,
    normalization_gain: Option<f32>,
    dynamics_preset: Option<&DynamicsPreset>,
    binaural_preset: Option<&BinauralPreset>,
    equalizer_settings: Option<&EqualizerSettings>,
) where
    S: Source<Item = f32> + Send + 'static,
{
    if spectrum_enabled.load(Ordering::SeqCst) {
        append_processed_source(
            sink,
            AnalyzingSource::new(source, Arc::clone(spectrum_producer)),
            normalization_gain,
            dynamics_preset,
            binaural_preset,
            equalizer_settings,
        );
    } else {
        append_processed_source(
            sink,
            source,
            normalization_gain,
            dynamics_preset,
            binaural_preset,
            equalizer_settings,
        );
    }
}

enum AudioThreadEvent {
    Command(AudioCommand),
    Timeout,
    Disconnected,
}

/// Main audio thread function
fn run_audio_thread(
    command_rx: Receiver<AudioCommand>,
    shared_state: Arc<SharedState>,
    spectrum_producer: Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: Arc<AtomicBool>,
) {
    info!("Rust audio playback thread starting");

    // Open the default audio output stream
    let stream = match DeviceSinkBuilder::open_default_sink() {
        Ok(mut s) => {
            s.log_on_drop(false);
            info!("Rust audio output stream opened");
            s
        }
        Err(e) => {
            error!("Failed to open audio stream: {:?}", e);
            return;
        }
    };

    let mut current_sink: Option<Player> = None;
    let mut crossfade_sink: Option<Player> = None;
    let mut crossfade_state: Option<CrossfadeState> = None;

    loop {
        let event = if crossfade_state.is_some() || shared_state.is_playing.load(Ordering::SeqCst) {
            match command_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(command) => AudioThreadEvent::Command(command),
                Err(mpsc::RecvTimeoutError::Timeout) => AudioThreadEvent::Timeout,
                Err(mpsc::RecvTimeoutError::Disconnected) => AudioThreadEvent::Disconnected,
            }
        } else {
            match command_rx.recv() {
                Ok(command) => AudioThreadEvent::Command(command),
                Err(_) => AudioThreadEvent::Disconnected,
            }
        };

        match event {
            AudioThreadEvent::Command(command) => match command {
                AudioCommand::Play {
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    equalizer_settings,
                } => {
                    // Stop any existing playback including crossfade
                    if let Some(sink) = current_sink.take() {
                        debug!("Stopping existing sink before starting new track");
                        sink.stop();
                    }
                    if let Some(cf_sink) = crossfade_sink.take() {
                        debug!("Stopping active crossfade sink before starting new track");
                        cf_sink.stop();
                    }
                    crossfade_state = None;
                    shared_state
                        .crossfade_initiated
                        .store(false, Ordering::SeqCst);

                    // Decode and play with coarse seek enabled for better seeking
                    info!(
                        "Playback thread play: song_id={}, title={:?}, bytes={}, duration={:.3}s",
                        metadata.id,
                        metadata.title,
                        audio_data.len(),
                        duration_secs
                    );
                    let byte_len = audio_data.len() as u64;
                    let cursor = Cursor::new(audio_data);
                    match Decoder::builder()
                        .with_data(cursor)
                        .with_byte_len(byte_len)
                        .with_coarse_seek(true)
                        .build()
                    {
                        Ok(source) => {
                            let sink = Player::connect_new(stream.mixer());
                            let volume = shared_state.read_inner().volume;
                            sink.set_volume(volume);

                            append_output_source(
                                &sink,
                                source,
                                &spectrum_producer,
                                &spectrum_enabled,
                                normalization_gain,
                                dynamics_preset.as_ref(),
                                binaural_preset.as_ref(),
                                equalizer_settings.as_ref(),
                            );

                            // Update shared state (single lock acquisition)
                            {
                                let mut inner = shared_state.write_inner();
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
                            debug!("Playback thread started song");
                        }
                        Err(e) => {
                            error!("Failed to decode audio: {:?}", e);
                        }
                    }
                }
                AudioCommand::Pause { ack } => {
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
                        debug!("Playback thread paused current sink");
                    }
                    let _ = ack.send(());
                }
                AudioCommand::Resume { ack } => {
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
                        debug!("Playback thread resumed current sink");
                    }
                    let _ = ack.send(());
                }
                AudioCommand::Stop { ack } => {
                    info!("Playback thread stop");
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
                        inner.playback_start = None;
                        inner.paused_position = 0.0;
                        inner.duration = 0.0;
                        inner.gapless_segments.clear();
                    }
                    let _ = ack.send(());
                }
                AudioCommand::SetVolume(volume) => {
                    debug!("Playback thread set volume: {:.3}", volume);
                    // During crossfade, let the idle loop handle proportional volumes
                    if crossfade_state.is_none()
                        && let Some(ref sink) = current_sink
                    {
                        sink.set_volume(volume);
                    }
                }
                AudioCommand::Seek { position_secs, ack } => {
                    debug!("Playback thread seek requested: {:.3}s", position_secs);
                    // If crossfade is active, abort it and restore full volume
                    if crossfade_state.is_some() {
                        debug!("Aborting crossfade before seek");
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
                            debug!(
                                "Playback thread seek complete: segment={:.3}s cumulative={:.3}s",
                                seek_pos, cumulative_pos
                            );
                        }
                    }
                    let _ = ack.send(());
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
                    info!(
                        "Playback thread append gapless: song_id={}, title={:?}, bytes={}, duration={:.3}s",
                        metadata.id,
                        metadata.title,
                        audio_data.len(),
                        duration_secs
                    );
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
                                append_output_source(
                                    sink,
                                    source,
                                    &spectrum_producer,
                                    &spectrum_enabled,
                                    normalization_gain,
                                    dynamics_preset.as_ref(),
                                    binaural_preset.as_ref(),
                                    equalizer_settings.as_ref(),
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
                                debug!("Playback thread appended gapless segment");
                            }
                            Err(e) => {
                                error!("Failed to decode gapless audio: {:?}", e);
                            }
                        }
                    } else {
                        warn!("Ignoring gapless append because there is no active sink");
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
                    ack,
                } => {
                    info!(
                        "Playback thread crossfade: song_id={}, title={:?}, bytes={}, duration={:.3}s, fade={}ms",
                        metadata.id,
                        metadata.title,
                        audio_data.len(),
                        duration_secs,
                        crossfade_duration_ms
                    );
                    // Stop any previous crossfade that's still running
                    if let Some(old_cf_sink) = crossfade_sink.take() {
                        debug!("Stopping previous crossfade sink");
                        old_cf_sink.stop();
                    }
                    // Move current sink to crossfade_sink (it keeps playing, fading out)
                    crossfade_sink = current_sink.take();

                    let byte_len = audio_data.len() as u64;
                    let cursor = Cursor::new(audio_data);
                    match Decoder::builder()
                        .with_data(cursor)
                        .with_byte_len(byte_len)
                        .with_coarse_seek(true)
                        .build()
                    {
                        Ok(source) => {
                            let new_sink = Player::connect_new(stream.mixer());
                            new_sink.set_volume(0.0); // Start silent, will ramp up

                            append_output_source(
                                &new_sink,
                                source,
                                &spectrum_producer,
                                &spectrum_enabled,
                                normalization_gain,
                                dynamics_preset.as_ref(),
                                binaural_preset.as_ref(),
                                equalizer_settings.as_ref(),
                            );

                            // Update shared state for the new song
                            {
                                let mut inner = shared_state.write_inner();
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
                            debug!("Playback thread started crossfade");
                            let _ = ack.send(Ok(()));
                        }
                        Err(e) => {
                            let message = format!("Failed to decode crossfade audio: {e:?}");
                            error!("{message}");
                            // Restore original sink if decode fails
                            current_sink = crossfade_sink.take();
                            shared_state
                                .crossfade_initiated
                                .store(false, Ordering::SeqCst);
                            let _ = ack.send(Err(AudioError::Playback(message)));
                        }
                    }
                }
                AudioCommand::Shutdown => {
                    info!("Rust audio playback thread shutting down");
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }
                    if let Some(cf_sink) = crossfade_sink.take() {
                        cf_sink.stop();
                    }
                    break;
                }
            },
            AudioThreadEvent::Timeout => {
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
                    info!("Playback thread reached end of current sink");
                }
            }
            AudioThreadEvent::Disconnected => {
                warn!("Audio command channel disconnected; playback thread exiting");
                break;
            }
        }
    }

    info!("Rust audio playback thread exited");
}
