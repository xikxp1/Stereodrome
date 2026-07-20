use log::{debug, error, info, warn};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::Split};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
const TRANSPORT_ACK_TIMEOUT: Duration = Duration::from_secs(1);
const STALL_GRACE_DURATION: Duration = Duration::from_millis(750);
const STALL_TIMEOUT: Duration = Duration::from_millis(2500);
const POSITION_EPSILON_SECONDS: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackLifecycleState {
    Playing,
    Paused,
    Stopped,
    Stalled,
}

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
    pub state: PlaybackLifecycleState,
    pub is_playing: bool,
    pub current_song_id: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackState {
    pub state: PlaybackLifecycleState,
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
        ack: Sender<AudioResult<()>>,
    },
    Pause {
        ack: Sender<()>,
    },
    Resume {
        ack: Sender<AudioResult<()>>,
    },
    RebuildOutput {
        ack: Sender<AudioResult<()>>,
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
        ack: Sender<AudioResult<()>>,
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

#[derive(Debug, Clone)]
struct AudioProcessingRequest {
    normalization_gain: Option<f32>,
    dynamics_preset: Option<DynamicsPreset>,
    binaural_preset: Option<BinauralPreset>,
    equalizer_settings: Option<EqualizerSettings>,
}

#[derive(Debug, Clone)]
struct ActiveAudioRequest {
    audio_data: Arc<[u8]>,
    metadata: SongMetadata,
    duration_secs: f64,
    processing: AudioProcessingRequest,
}

impl ActiveAudioRequest {
    fn new(
        audio_data: Arc<[u8]>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    ) -> Self {
        Self {
            audio_data,
            metadata,
            duration_secs,
            processing: AudioProcessingRequest {
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
            },
        }
    }
}

/// A segment within a gapless playback chain.
/// Each segment represents one song appended to the same Rodio player.
#[derive(Debug, Clone)]
struct GaplessSegment {
    metadata: SongMetadata,
    duration: f64,         // this segment's duration in seconds
    cumulative_start: f64, // sum of all previous segments' durations
    request: ActiveAudioRequest,
}

/// Inner playback state consolidated into a single struct for efficient locking
struct PlaybackInner {
    current_song: Option<SongMetadata>,
    volume: f32,
    consumed_position: f64,
    duration: f64,
    active_request: Option<ActiveAudioRequest>,
    source_generation: u64,
    /// Gapless playback segments. When multiple consecutive album tracks are
    /// appended to the same player, each gets a segment for position tracking.
    gapless_segments: Vec<GaplessSegment>,
}

impl Default for PlaybackInner {
    fn default() -> Self {
        Self {
            current_song: None,
            volume: 0.8,
            consumed_position: 0.0,
            duration: 0.0,
            active_request: None,
            source_generation: 0,
            gapless_segments: Vec::new(),
        }
    }
}

/// State shared between the main thread and audio thread.
/// Uses a single `RwLock` for efficient concurrent reads (position emitter at 10Hz).
struct SharedState {
    is_playing: AtomicBool,
    stalled: AtomicBool,
    stream_failed: AtomicBool,
    crossfade_initiated: AtomicBool,
    next_source_generation: AtomicU64,
    inner: RwLock<PlaybackInner>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            is_playing: AtomicBool::new(false),
            stalled: AtomicBool::new(false),
            stream_failed: AtomicBool::new(false),
            crossfade_initiated: AtomicBool::new(false),
            next_source_generation: AtomicU64::new(1),
            inner: RwLock::new(PlaybackInner::default()),
        }
    }

    fn read_inner(&self) -> std::sync::RwLockReadGuard<'_, PlaybackInner> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write_inner(&self) -> std::sync::RwLockWriteGuard<'_, PlaybackInner> {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn get_position(&self) -> f64 {
        self.read_inner().consumed_position
    }

    fn state_from_inner(&self, inner: &PlaybackInner) -> PlaybackLifecycleState {
        if inner.current_song.is_none() {
            PlaybackLifecycleState::Stopped
        } else if self.stalled.load(Ordering::SeqCst) {
            PlaybackLifecycleState::Stalled
        } else if self.is_playing.load(Ordering::SeqCst) {
            PlaybackLifecycleState::Playing
        } else {
            PlaybackLifecycleState::Paused
        }
    }

    fn state(&self) -> PlaybackLifecycleState {
        let inner = self.read_inner();
        self.state_from_inner(&inner)
    }

    fn is_playing(&self) -> bool {
        self.state() == PlaybackLifecycleState::Playing
    }

    fn mark_playing(&self) {
        self.stream_failed.store(false, Ordering::SeqCst);
        self.stalled.store(false, Ordering::SeqCst);
        self.is_playing.store(true, Ordering::SeqCst);
    }

    fn mark_output_ready(&self) {
        self.stream_failed.store(false, Ordering::SeqCst);
    }

    fn mark_paused(&self) {
        self.stalled.store(false, Ordering::SeqCst);
        self.is_playing.store(false, Ordering::SeqCst);
    }

    fn mark_stalled(&self) {
        if self.read_inner().current_song.is_some() {
            self.stalled.store(true, Ordering::SeqCst);
            self.is_playing.store(false, Ordering::SeqCst);
        }
    }

    fn mark_stream_failed(&self) {
        self.stream_failed.store(true, Ordering::SeqCst);
        self.mark_stalled();
    }

    fn mark_stopped(&self) {
        self.stream_failed.store(false, Ordering::SeqCst);
        self.stalled.store(false, Ordering::SeqCst);
        self.is_playing.store(false, Ordering::SeqCst);
    }

    fn next_generation(&self) -> u64 {
        self.next_source_generation.fetch_add(1, Ordering::SeqCst)
    }

    fn segment_index_for_position(inner: &PlaybackInner, position: f64) -> usize {
        if inner.gapless_segments.is_empty() {
            return 0;
        }
        let mut segment_idx = 0;
        for (index, segment) in inner.gapless_segments.iter().enumerate() {
            if position < segment.cumulative_start + segment.duration {
                segment_idx = index;
                break;
            }
            segment_idx = index;
        }
        segment_idx
    }

    fn segment_index_for_current_position(inner: &PlaybackInner) -> usize {
        Self::segment_index_for_position(inner, inner.consumed_position)
    }

    fn update_consumed_position(&self, generation: u64, source_position: f64) -> bool {
        let mut inner = self.write_inner();
        if inner.source_generation != generation {
            return false;
        }

        let previous = inner.consumed_position;
        let mut cumulative_position = source_position.max(0.0);
        if inner.gapless_segments.len() > 1 {
            let mut segment_idx = Self::segment_index_for_current_position(&inner);
            let current_segment = &inner.gapless_segments[segment_idx];
            let candidate = current_segment.cumulative_start + source_position;
            let near_segment_end =
                previous >= current_segment.cumulative_start + current_segment.duration - 0.5;
            if segment_idx + 1 < inner.gapless_segments.len()
                && near_segment_end
                && candidate + 0.25 < previous
            {
                segment_idx += 1;
            }
            cumulative_position =
                inner.gapless_segments[segment_idx].cumulative_start + source_position.max(0.0);
        }

        inner.consumed_position = cumulative_position.clamp(0.0, inner.duration);
        inner.consumed_position > previous + POSITION_EPSILON_SECONDS
    }

    fn set_consumed_position(&self, generation: u64, cumulative_position: f64) {
        let mut inner = self.write_inner();
        if inner.source_generation == generation {
            inner.consumed_position = cumulative_position.clamp(0.0, inner.duration);
        }
    }

    fn active_request_at_position(&self) -> Option<(ActiveAudioRequest, f64)> {
        let inner = self.read_inner();
        if inner.gapless_segments.is_empty() {
            return inner
                .active_request
                .clone()
                .map(|request| (request, inner.consumed_position));
        }

        let segment_idx = Self::segment_index_for_current_position(&inner);
        let segment = &inner.gapless_segments[segment_idx];
        let position = (inner.consumed_position - segment.cumulative_start)
            .max(0.0)
            .min(segment.duration);
        Some((segment.request.clone(), position))
    }

    fn set_active_request(&self, generation: u64, request: ActiveAudioRequest) {
        let mut inner = self.write_inner();
        inner.current_song = Some(request.metadata.clone());
        inner.consumed_position = 0.0;
        inner.duration = request.duration_secs;
        inner.active_request = Some(request.clone());
        inner.source_generation = generation;
        inner.gapless_segments = vec![GaplessSegment {
            metadata: request.metadata.clone(),
            duration: request.duration_secs,
            cumulative_start: 0.0,
            request,
        }];
    }

    fn replace_with_rebuilt_request(
        &self,
        generation: u64,
        request: ActiveAudioRequest,
        position: f64,
    ) {
        let mut inner = self.write_inner();
        inner.current_song = Some(request.metadata.clone());
        inner.consumed_position = position.clamp(0.0, request.duration_secs);
        inner.duration = request.duration_secs;
        inner.active_request = Some(request.clone());
        inner.source_generation = generation;
        inner.gapless_segments = vec![GaplessSegment {
            metadata: request.metadata.clone(),
            duration: request.duration_secs,
            cumulative_start: 0.0,
            request,
        }];
    }

    /// Get playback state with segment-aware position/metadata and the current segment index.
    /// When gapless segments are active, returns per-song position and metadata
    /// instead of cumulative position across the chain.
    fn get_gapless_state(&self) -> (PlaybackState, usize) {
        let inner = self.read_inner();
        let lifecycle_state = self.state_from_inner(&inner);
        let is_playing = lifecycle_state == PlaybackLifecycleState::Playing;
        let cumulative_pos = inner.consumed_position;

        if inner.gapless_segments.len() > 1 {
            // Find which segment we're in
            let seg_idx = Self::segment_index_for_current_position(&inner);
            let seg = &inner.gapless_segments[seg_idx];
            let song_pos = (cumulative_pos - seg.cumulative_start)
                .max(0.0)
                .min(seg.duration);

            (
                PlaybackState {
                    state: lifecycle_state,
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
                    state: lifecycle_state,
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
            state: state.state,
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
    #[must_use]
    pub fn get_gapless_state(&self) -> (PlaybackState, usize) {
        self.shared_state.get_gapless_state()
    }

    #[must_use]
    pub fn is_crossfade_initiated(&self) -> bool {
        self.shared_state.crossfade_initiated.load(Ordering::SeqCst)
    }

    pub fn set_crossfade_initiated(&self, value: bool) {
        self.shared_state
            .crossfade_initiated
            .store(value, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_last_gapless_segment(&self, segment_idx: usize) -> bool {
        let inner = self.shared_state.read_inner();
        inner.gapless_segments.len() <= 1 || segment_idx == inner.gapless_segments.len() - 1
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.shared_state.is_playing()
    }

    pub fn clear_finished_state(&self) {
        let mut inner = self.shared_state.write_inner();
        inner.current_song = None;
        inner.consumed_position = 0.0;
        inner.duration = 0.0;
        inner.active_request = None;
        inner.source_generation = 0;
        inner.gapless_segments.clear();
        self.shared_state.mark_stopped();
    }
}

// Implement Send + Sync manually since we use channels for thread communication
unsafe impl Send for AudioPlayer {}
unsafe impl Sync for AudioPlayer {}

impl AudioPlayer {
    /// Creates an audio player with spectrum analysis enabled.
    ///
    /// # Errors
    ///
    /// Returns an error if the audio player cannot be initialized.
    pub fn new() -> AudioResult<Self> {
        Self::new_with_spectrum(true)
    }

    /// Creates an audio player and configures spectrum analysis.
    ///
    /// # Errors
    ///
    /// Returns an error if the audio player cannot be initialized.
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
                &command_rx,
                &state_clone,
                &producer_clone,
                &spectrum_enabled_clone,
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
    /// Starts playback, replacing any current source.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot reach the audio thread, the
    /// source cannot be decoded, or no output stream can be opened.
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
        self.send_result_command("play", |ack| AudioCommand::Play {
            audio_data,
            metadata,
            duration_secs,
            normalization_gain,
            dynamics_preset,
            binaural_preset,
            equalizer_settings,
            ack,
        })
    }

    /// Append a song to the existing player for gapless playback.
    /// The song's audio pipeline is decoded and appended without stopping the current player.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no active sink, decoding fails, or the
    /// audio thread cannot apply the command.
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
        self.send_result_command("append gapless", |ack| AudioCommand::AppendGapless {
            audio_data,
            metadata,
            duration_secs,
            normalization_gain,
            dynamics_preset,
            binaural_preset,
            equalizer_settings,
            ack,
        })
    }

    /// Start a crossfade transition: fade out current song while fading in a new one.
    ///
    /// # Errors
    ///
    /// Returns an error if the new source cannot be prepared or the audio
    /// thread cannot apply the command.
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
            .map_err(|e| AudioError::Playback(format!("Failed to send crossfade command: {e}")))?;
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
    #[must_use]
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

    fn send_result_command(
        &self,
        name: &str,
        make_command: impl FnOnce(Sender<AudioResult<()>>) -> AudioCommand,
    ) -> AudioResult<()> {
        let (ack_tx, ack_rx) = mpsc::channel();
        self.command_tx
            .send(make_command(ack_tx))
            .map_err(|e| AudioError::Playback(format!("Failed to send {name} command: {e}")))?;
        match ack_rx.recv_timeout(TRANSPORT_ACK_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AudioError::Playback(format!(
                "Audio thread did not acknowledge {name} command in time"
            ))),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AudioError::Playback(format!(
                "Audio thread disconnected while applying {name} command"
            ))),
        }
    }

    /// Pauses the current source.
    ///
    /// # Errors
    ///
    /// Returns an error if the audio thread cannot apply the command.
    pub fn pause(&self) -> AudioResult<()> {
        self.send_transport_command("pause", |ack| AudioCommand::Pause { ack })
    }

    /// Resumes the current source, rebuilding output if necessary.
    ///
    /// # Errors
    ///
    /// Returns an error if the source cannot be resumed or rebuilt.
    pub fn resume(&self) -> AudioResult<()> {
        self.send_result_command("resume", |ack| AudioCommand::Resume { ack })
    }

    /// Rebuilds the output stream for the active source.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no active source or output cannot be opened.
    pub fn rebuild_output(&self) -> AudioResult<()> {
        self.send_result_command("rebuild output", |ack| AudioCommand::RebuildOutput { ack })
    }

    /// Stops playback and clears the active source.
    ///
    /// # Errors
    ///
    /// Returns an error if the audio thread cannot apply the command.
    pub fn stop(&self) -> AudioResult<()> {
        self.send_transport_command("stop", |ack| AudioCommand::Stop { ack })
    }

    /// Sets the playback volume, clamped to the inclusive range 0.0–1.0.
    ///
    /// # Errors
    ///
    /// Returns an error if the audio command channel is disconnected.
    pub fn set_volume(&self, volume: f32) -> AudioResult<()> {
        let clamped = volume.clamp(0.0, 1.0);
        self.shared_state.write_inner().volume = clamped;
        self.command_tx
            .send(AudioCommand::SetVolume(clamped))
            .map_err(|e| AudioError::Playback(format!("Failed to send volume command: {e}")))
    }

    /// Seeks to a position in seconds within the current segment.
    ///
    /// # Errors
    ///
    /// Returns an error if the audio thread cannot apply the command.
    pub fn seek(&self, position_secs: f64) -> AudioResult<()> {
        let duration = self.shared_state.read_inner().duration;
        let clamped = position_secs.clamp(0.0, duration);
        self.send_transport_command("seek", |ack| AudioCommand::Seek {
            position_secs: clamped,
            ack,
        })
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn get_volume(&self) -> f32 {
        self.shared_state.read_inner().volume
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn get_position(&self) -> f64 {
        self.shared_state.get_position()
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn get_duration(&self) -> f64 {
        self.shared_state.read_inner().duration
    }

    #[must_use]
    pub fn get_status(&self) -> PlaybackStatus {
        self.shared_state.get_status()
    }

    #[must_use]
    pub fn get_playback_state(&self) -> PlaybackState {
        self.shared_state.get_gapless_state().0
    }

    #[must_use]
    pub fn get_gapless_state(&self) -> (PlaybackState, usize) {
        self.shared_state.get_gapless_state()
    }

    #[must_use]
    pub fn state_handle(&self) -> AudioStateHandle {
        AudioStateHandle {
            shared_state: Arc::clone(&self.shared_state),
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn current_song_id(&self) -> Option<String> {
        self.shared_state
            .read_inner()
            .current_song
            .as_ref()
            .map(|s| s.id.clone())
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.shared_state.is_playing()
    }

    #[allow(dead_code)]
    /// Get the spectrum buffer consumer for the spectrum analyzer
    #[must_use]
    pub fn get_spectrum_consumer(&self) -> Arc<Mutex<HeapCons<f32>>> {
        Arc::clone(&self.spectrum_consumer)
    }

    pub fn set_spectrum_enabled(&self, enabled: bool) {
        self.spectrum_enabled.store(enabled, Ordering::SeqCst);
    }

    #[allow(dead_code)]
    /// Get the `is_playing` flag for the spectrum analyzer
    #[must_use]
    pub fn get_is_playing_flag(&self) -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(
            self.shared_state.is_playing.load(Ordering::SeqCst),
        ))
    }

    #[allow(dead_code)]
    /// Get a clone of the shared `is_playing` atomic for spectrum emitter
    #[must_use]
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
            duration_secs: f64::from(duration_ms) / 1000.0,
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

struct PlaybackWatchdog {
    last_progress_at: Instant,
    grace_until: Instant,
    last_position: f64,
}

impl PlaybackWatchdog {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            last_progress_at: now,
            grace_until: now + STALL_GRACE_DURATION,
            last_position: 0.0,
        }
    }

    fn reset(&mut self, position: f64) {
        let now = Instant::now();
        self.last_progress_at = now;
        self.grace_until = now + STALL_GRACE_DURATION;
        self.last_position = position;
    }

    fn observe(&mut self, position: f64, should_detect_stall: bool, shared_state: &SharedState) {
        let now = Instant::now();
        let advanced = position > self.last_position + POSITION_EPSILON_SECONDS;
        let jumped = (position - self.last_position).abs() > 0.5;
        if advanced || jumped {
            self.last_progress_at = now;
            self.last_position = position;
            if shared_state.stalled.load(Ordering::SeqCst) && should_detect_stall {
                shared_state.mark_playing();
            }
            return;
        }

        if should_detect_stall
            && now >= self.grace_until
            && now.duration_since(self.last_progress_at) >= STALL_TIMEOUT
        {
            shared_state.mark_stalled();
        }
    }
}

fn open_output_stream(shared_state: &Arc<SharedState>) -> AudioResult<MixerDeviceSink> {
    let callback_state = Arc::clone(shared_state);
    let mut stream = DeviceSinkBuilder::from_default_device()
        .map_err(|e| AudioError::Playback(format!("Failed to create audio stream builder: {e:?}")))?
        .with_error_callback(move |error| {
            warn!("Rust audio stream error: {error:?}");
            callback_state.mark_stream_failed();
        })
        .open_sink_or_fallback()
        .map_err(|e| AudioError::Playback(format!("Failed to open audio stream: {e:?}")))?;
    stream.log_on_drop(false);
    Ok(stream)
}

fn append_request_to_sink(
    sink: &Player,
    request: &ActiveAudioRequest,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: &Arc<AtomicBool>,
) -> AudioResult<()> {
    let byte_len = request.audio_data.len() as u64;
    let cursor = Cursor::new(Arc::clone(&request.audio_data));
    let source = Decoder::builder()
        .with_data(cursor)
        .with_byte_len(byte_len)
        .with_coarse_seek(true)
        .build()
        .map_err(|e| {
            AudioError::Playback(format!(
                "Failed to decode audio for {}: {e:?}",
                request.metadata.id
            ))
        })?;

    append_output_source(
        sink,
        source,
        spectrum_producer,
        spectrum_enabled,
        request.processing.normalization_gain,
        request.processing.dynamics_preset.as_ref(),
        request.processing.binaural_preset.as_ref(),
        request.processing.equalizer_settings.as_ref(),
    );
    Ok(())
}

fn connect_request_sink(
    stream: &MixerDeviceSink,
    request: &ActiveAudioRequest,
    volume: f32,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: &Arc<AtomicBool>,
) -> AudioResult<Player> {
    let sink = Player::connect_new(stream.mixer());
    sink.set_volume(volume);
    append_request_to_sink(&sink, request, spectrum_producer, spectrum_enabled)?;
    Ok(sink)
}

fn ensure_output_stream<'a>(
    shared_state: &Arc<SharedState>,
    stream: &'a mut Option<MixerDeviceSink>,
) -> AudioResult<&'a MixerDeviceSink> {
    if stream.is_none() {
        match open_output_stream(shared_state) {
            Ok(next_stream) => {
                info!("Rust audio output stream opened");
                *stream = Some(next_stream);
                shared_state.mark_output_ready();
            }
            Err(e) => {
                shared_state.mark_stream_failed();
                return Err(e);
            }
        }
    }

    stream
        .as_ref()
        .ok_or_else(|| AudioError::Playback("Audio output stream is unavailable".to_string()))
}

fn seek_sink_to_position(sink: &Player, position: f64, duration: f64) -> AudioResult<f64> {
    let target = if duration > 1.0 {
        position.clamp(0.0, duration - 1.0)
    } else {
        0.0
    };
    sink.try_seek(Duration::from_secs_f64(target))
        .map_err(|e| AudioError::Playback(format!("Failed to seek rebuilt audio: {e:?}")))?;
    Ok(target)
}

fn rebuild_active_output(
    shared_state: &Arc<SharedState>,
    stream: &mut Option<MixerDeviceSink>,
    current_sink: &mut Option<Player>,
    crossfade_sink: &mut Option<Player>,
    crossfade_state: &mut Option<CrossfadeState>,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: &Arc<AtomicBool>,
) -> AudioResult<(u64, f64)> {
    let mut next_stream = match open_output_stream(shared_state) {
        Ok(stream) => stream,
        Err(e) => {
            shared_state.mark_stream_failed();
            return Err(e);
        }
    };
    next_stream.log_on_drop(false);

    let Some((request, position)) = shared_state.active_request_at_position() else {
        *stream = Some(next_stream);
        shared_state.mark_output_ready();
        return Ok((0, 0.0));
    };

    let generation = shared_state.next_generation();
    let volume = shared_state.read_inner().volume;
    let sink = match connect_request_sink(
        &next_stream,
        &request,
        volume,
        spectrum_producer,
        spectrum_enabled,
    ) {
        Ok(sink) => sink,
        Err(e) => {
            if current_sink.is_none() {
                shared_state.mark_stream_failed();
            }
            return Err(e);
        }
    };
    let restored_position = match seek_sink_to_position(&sink, position, request.duration_secs) {
        Ok(position) => position,
        Err(e) => {
            if current_sink.is_none() {
                shared_state.mark_stream_failed();
            }
            return Err(e);
        }
    };

    if let Some(sink) = current_sink.take() {
        sink.stop();
    }
    if let Some(sink) = crossfade_sink.take() {
        sink.stop();
    }
    *crossfade_state = None;
    shared_state
        .crossfade_initiated
        .store(false, Ordering::SeqCst);

    *stream = Some(next_stream);
    shared_state.replace_with_rebuilt_request(generation, request, restored_position);
    shared_state.mark_playing();
    *current_sink = Some(sink);
    Ok((generation, restored_position))
}

fn should_poll_audio_thread(
    has_current_sink: bool,
    has_crossfade_state: bool,
    lifecycle_state: PlaybackLifecycleState,
) -> bool {
    has_current_sink || has_crossfade_state || lifecycle_state == PlaybackLifecycleState::Playing
}

fn mark_missing_sink_if_playing(
    shared_state: &SharedState,
    watchdog: &mut PlaybackWatchdog,
) -> bool {
    if shared_state.state() != PlaybackLifecycleState::Playing {
        return false;
    }

    warn!("Playback state was playing without an active sink; marking stalled");
    shared_state.mark_stalled();
    watchdog.reset(shared_state.get_position());
    true
}

struct AudioThread<'a> {
    shared_state: &'a Arc<SharedState>,
    spectrum_producer: &'a Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: &'a Arc<AtomicBool>,
    stream: Option<MixerDeviceSink>,
    current_sink: Option<Player>,
    crossfade_sink: Option<Player>,
    crossfade_state: Option<CrossfadeState>,
    current_generation: u64,
    watchdog: PlaybackWatchdog,
}

impl<'a> AudioThread<'a> {
    fn new(
        shared_state: &'a Arc<SharedState>,
        spectrum_producer: &'a Arc<Mutex<HeapProd<f32>>>,
        spectrum_enabled: &'a Arc<AtomicBool>,
    ) -> Self {
        let stream = match open_output_stream(shared_state) {
            Ok(stream) => {
                info!("Rust audio output stream opened");
                Some(stream)
            }
            Err(error) => {
                error!("Failed to open audio stream; continuing without output: {error}");
                shared_state.mark_stream_failed();
                None
            }
        };

        Self {
            shared_state,
            spectrum_producer,
            spectrum_enabled,
            stream,
            current_sink: None,
            crossfade_sink: None,
            crossfade_state: None,
            current_generation: 0,
            watchdog: PlaybackWatchdog::new(),
        }
    }

    fn next_event(&self, command_rx: &Receiver<AudioCommand>) -> AudioThreadEvent {
        if should_poll_audio_thread(
            self.current_sink.is_some(),
            self.crossfade_state.is_some(),
            self.shared_state.state(),
        ) {
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
        }
    }

    fn handle_command(&mut self, command: AudioCommand) -> bool {
        match command {
            AudioCommand::Play {
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
                ack,
            } => self.play(
                ActiveAudioRequest::new(
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    equalizer_settings,
                ),
                &ack,
            ),
            AudioCommand::Pause { ack } => self.pause(&ack),
            AudioCommand::Resume { ack } => self.resume(&ack),
            AudioCommand::RebuildOutput { ack } => self.rebuild_output(&ack),
            AudioCommand::Stop { ack } => self.stop(&ack),
            AudioCommand::SetVolume(volume) => self.set_volume(volume),
            AudioCommand::Seek { position_secs, ack } => self.seek(position_secs, &ack),
            AudioCommand::AppendGapless {
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
                ack,
            } => self.append_gapless(
                ActiveAudioRequest::new(
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    equalizer_settings,
                ),
                &ack,
            ),
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
            } => self.crossfade_play(
                ActiveAudioRequest::new(
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    equalizer_settings,
                ),
                crossfade_duration_ms,
                &ack,
            ),
            AudioCommand::Shutdown => {
                self.shutdown();
                return false;
            }
        }
        true
    }

    fn play(&mut self, request: ActiveAudioRequest, ack: &Sender<AudioResult<()>>) {
        info!(
            "Playback thread play: song_id={}, title={:?}, bytes={}, duration={:.3}s",
            request.metadata.id,
            request.metadata.title,
            request.audio_data.len(),
            request.duration_secs
        );
        let volume = self.shared_state.read_inner().volume;
        let result = match ensure_output_stream(self.shared_state, &mut self.stream) {
            Ok(output_stream) => connect_request_sink(
                output_stream,
                &request,
                volume,
                self.spectrum_producer,
                self.spectrum_enabled,
            ),
            Err(error) => Err(error),
        };
        match result {
            Ok(sink) => {
                if let Some(old_sink) = self.current_sink.take() {
                    debug!("Stopping existing sink before starting new track");
                    old_sink.stop();
                }
                if let Some(crossfade_sink) = self.crossfade_sink.take() {
                    debug!("Stopping active crossfade sink before starting new track");
                    crossfade_sink.stop();
                }
                self.crossfade_state = None;
                self.shared_state
                    .crossfade_initiated
                    .store(false, Ordering::SeqCst);

                let generation = self.shared_state.next_generation();
                self.shared_state.set_active_request(generation, request);
                self.shared_state.mark_playing();
                self.current_generation = generation;
                self.watchdog.reset(0.0);
                self.current_sink = Some(sink);
                debug!("Playback thread started song");
                let _ = ack.send(Ok(()));
            }
            Err(error) => {
                error!("{error}");
                let _ = ack.send(Err(error));
            }
        }
    }

    fn pause(&mut self, ack: &Sender<()>) {
        if let Some(ref sink) = self.current_sink
            && !sink.is_paused()
        {
            self.shared_state
                .update_consumed_position(self.current_generation, sink.get_pos().as_secs_f64());
            sink.pause();
            if let Some(ref crossfade_sink) = self.crossfade_sink {
                crossfade_sink.pause();
            }
            if let Some(ref mut crossfade_state) = self.crossfade_state {
                crossfade_state.pause();
            }
            self.shared_state.mark_paused();
            debug!("Playback thread paused current sink");
        }
        let _ = ack.send(());
    }

    fn resume(&mut self, ack: &Sender<AudioResult<()>>) {
        let can_resume = self
            .current_sink
            .as_ref()
            .is_some_and(|sink| !sink.empty() && !self.shared_state.stalled.load(Ordering::SeqCst));
        let result = if can_resume {
            if let Some(ref sink) = self.current_sink
                && sink.is_paused()
            {
                sink.play();
                if let Some(ref crossfade_sink) = self.crossfade_sink {
                    crossfade_sink.play();
                }
                if let Some(ref mut crossfade_state) = self.crossfade_state {
                    crossfade_state.resume();
                }
            }
            let position = self.current_sink.as_ref().map_or_else(
                || self.shared_state.get_position(),
                |sink| sink.get_pos().as_secs_f64(),
            );
            self.shared_state.mark_playing();
            self.watchdog.reset(position);
            debug!("Playback thread ensured current sink is playing");
            Ok(())
        } else {
            self.rebuild_current_output().map(|_| ())
        };
        let _ = ack.send(result);
    }

    fn rebuild_output(&mut self, ack: &Sender<AudioResult<()>>) {
        let result = self.rebuild_current_output().map(|_| ());
        let _ = ack.send(result);
    }

    fn rebuild_current_output(&mut self) -> AudioResult<(u64, f64)> {
        let result = rebuild_active_output(
            self.shared_state,
            &mut self.stream,
            &mut self.current_sink,
            &mut self.crossfade_sink,
            &mut self.crossfade_state,
            self.spectrum_producer,
            self.spectrum_enabled,
        );
        if let Ok((generation, position)) = result
            && generation != 0
        {
            self.current_generation = generation;
            self.watchdog.reset(position);
        }
        result
    }

    fn stop(&mut self, ack: &Sender<()>) {
        info!("Playback thread stop");
        if let Some(sink) = self.current_sink.take() {
            sink.stop();
        }
        if let Some(crossfade_sink) = self.crossfade_sink.take() {
            crossfade_sink.stop();
        }
        self.crossfade_state = None;
        self.shared_state
            .crossfade_initiated
            .store(false, Ordering::SeqCst);
        self.shared_state.mark_stopped();
        {
            let mut inner = self.shared_state.write_inner();
            inner.current_song = None;
            inner.consumed_position = 0.0;
            inner.duration = 0.0;
            inner.active_request = None;
            inner.source_generation = 0;
            inner.gapless_segments.clear();
        }
        self.current_generation = 0;
        self.watchdog.reset(0.0);
        let _ = ack.send(());
    }

    fn set_volume(&self, volume: f32) {
        debug!("Playback thread set volume: {volume:.3}");
        if self.crossfade_state.is_none()
            && let Some(ref sink) = self.current_sink
        {
            sink.set_volume(volume);
        }
    }

    fn seek(&mut self, position_secs: f64, ack: &Sender<()>) {
        debug!("Playback thread seek requested: {position_secs:.3}s");
        if self.crossfade_state.is_some() {
            debug!("Aborting crossfade before seek");
            if let Some(crossfade_sink) = self.crossfade_sink.take() {
                crossfade_sink.stop();
            }
            self.crossfade_state = None;
            let volume = self.shared_state.read_inner().volume;
            if let Some(ref sink) = self.current_sink {
                sink.set_volume(volume);
            }
        }

        if let Some(ref sink) = self.current_sink {
            let (seek_position, cumulative_position) = self.seek_positions(position_secs);
            if let Err(error) = sink.try_seek(Duration::from_secs_f64(seek_position)) {
                warn!("Seek failed: {error:?}");
            } else {
                self.shared_state
                    .set_consumed_position(self.current_generation, cumulative_position);
                if !sink.is_paused() {
                    self.shared_state.mark_playing();
                }
                self.watchdog.reset(cumulative_position);
                debug!(
                    "Playback thread seek complete: segment={seek_position:.3}s \
                     cumulative={cumulative_position:.3}s"
                );
            }
        }
        let _ = ack.send(());
    }

    fn seek_positions(&self, position_secs: f64) -> (f64, f64) {
        let inner = self.shared_state.read_inner();
        if inner.gapless_segments.len() > 1 {
            let cumulative = inner.consumed_position;
            let mut segment_index = 0;
            for (index, segment) in inner.gapless_segments.iter().enumerate() {
                segment_index = index;
                if cumulative < segment.cumulative_start + segment.duration {
                    break;
                }
            }
            let segment = &inner.gapless_segments[segment_index];
            let clamped = position_secs.clamp(0.0, segment.duration);
            (clamped, segment.cumulative_start + clamped)
        } else {
            let clamped = position_secs.clamp(0.0, inner.duration);
            (clamped, clamped)
        }
    }

    fn append_gapless(&self, request: ActiveAudioRequest, ack: &Sender<AudioResult<()>>) {
        info!(
            "Playback thread append gapless: song_id={}, title={:?}, bytes={}, duration={:.3}s",
            request.metadata.id,
            request.metadata.title,
            request.audio_data.len(),
            request.duration_secs
        );
        let Some(ref sink) = self.current_sink else {
            let error = AudioError::Playback(
                "Cannot append gapless track because there is no active sink".to_string(),
            );
            warn!("{error}");
            let _ = ack.send(Err(error));
            return;
        };

        match append_request_to_sink(
            sink,
            &request,
            self.spectrum_producer,
            self.spectrum_enabled,
        ) {
            Ok(()) => {
                let mut inner = self.shared_state.write_inner();
                let cumulative_start = inner
                    .gapless_segments
                    .last()
                    .map_or(inner.duration, |segment| {
                        segment.cumulative_start + segment.duration
                    });
                let duration = request.duration_secs;
                inner.gapless_segments.push(GaplessSegment {
                    metadata: request.metadata.clone(),
                    duration,
                    cumulative_start,
                    request,
                });
                inner.duration = cumulative_start + duration;
                drop(inner);
                debug!("Playback thread appended gapless segment");
                let _ = ack.send(Ok(()));
            }
            Err(error) => {
                error!("{error}");
                let _ = ack.send(Err(error));
            }
        }
    }

    fn crossfade_play(
        &mut self,
        request: ActiveAudioRequest,
        crossfade_duration_ms: u32,
        ack: &Sender<AudioResult<()>>,
    ) {
        info!(
            "Playback thread crossfade: song_id={}, title={:?}, bytes={}, duration={:.3}s, fade={}ms",
            request.metadata.id,
            request.metadata.title,
            request.audio_data.len(),
            request.duration_secs,
            crossfade_duration_ms
        );
        let result = match ensure_output_stream(self.shared_state, &mut self.stream) {
            Ok(output_stream) => connect_request_sink(
                output_stream,
                &request,
                0.0,
                self.spectrum_producer,
                self.spectrum_enabled,
            ),
            Err(error) => Err(error),
        };
        match result {
            Ok(new_sink) => {
                if let Some(old_crossfade_sink) = self.crossfade_sink.take() {
                    debug!("Stopping previous crossfade sink");
                    old_crossfade_sink.stop();
                }
                self.crossfade_sink = self.current_sink.take();
                let generation = self.shared_state.next_generation();
                self.shared_state.set_active_request(generation, request);
                self.shared_state.mark_playing();
                self.shared_state
                    .crossfade_initiated
                    .store(false, Ordering::SeqCst);
                self.current_sink = Some(new_sink);
                self.current_generation = generation;
                self.watchdog.reset(0.0);
                self.crossfade_state = Some(CrossfadeState::new(crossfade_duration_ms));
                debug!("Playback thread started crossfade");
                let _ = ack.send(Ok(()));
            }
            Err(error) => {
                error!("{error}");
                self.shared_state
                    .crossfade_initiated
                    .store(false, Ordering::SeqCst);
                let _ = ack.send(Err(error));
            }
        }
    }

    fn handle_timeout(&mut self) {
        if let Some(ref sink) = self.current_sink {
            let source_position = sink.get_pos().as_secs_f64();
            let advanced = self
                .shared_state
                .update_consumed_position(self.current_generation, source_position);
            let observed_position = self.shared_state.get_position();
            let should_detect_stall = !sink.is_paused() && !sink.empty();
            if advanced {
                if self.shared_state.stalled.load(Ordering::SeqCst) && should_detect_stall {
                    self.shared_state.mark_playing();
                }
                self.watchdog.reset(observed_position);
            } else {
                self.watchdog
                    .observe(observed_position, should_detect_stall, self.shared_state);
            }
        } else {
            mark_missing_sink_if_playing(self.shared_state, &mut self.watchdog);
        }
        self.update_crossfade();
        self.mark_finished_playback();
    }

    fn update_crossfade(&mut self) {
        let Some(ref crossfade_state) = self.crossfade_state else {
            return;
        };
        let progress = crossfade_state.progress();
        let user_volume = self.shared_state.read_inner().volume;
        let fade_out_volume =
            user_volume * crossfade_factor((progress * std::f64::consts::FRAC_PI_2).cos());
        let fade_in_volume =
            user_volume * crossfade_factor((progress * std::f64::consts::FRAC_PI_2).sin());

        if let Some(ref old_sink) = self.crossfade_sink
            && !old_sink.empty()
        {
            old_sink.set_volume(fade_out_volume);
        }
        if let Some(ref current_sink) = self.current_sink {
            current_sink.set_volume(fade_in_volume);
        }

        if crossfade_state.is_complete()
            || self
                .crossfade_sink
                .as_ref()
                .is_some_and(rodio::Player::empty)
        {
            if let Some(crossfade_sink) = self.crossfade_sink.take() {
                crossfade_sink.stop();
            }
            self.crossfade_state = None;
            let volume = self.shared_state.read_inner().volume;
            if let Some(ref current_sink) = self.current_sink {
                current_sink.set_volume(volume);
            }
        }
    }

    fn mark_finished_playback(&self) {
        if let Some(ref sink) = self.current_sink
            && sink.empty()
            && self.shared_state.state() != PlaybackLifecycleState::Paused
        {
            {
                let mut inner = self.shared_state.write_inner();
                inner.consumed_position = inner.duration;
            }
            self.shared_state.mark_paused();
            info!("Playback thread reached end of current sink");
        }
    }

    fn shutdown(&mut self) {
        info!("Rust audio playback thread shutting down");
        if let Some(sink) = self.current_sink.take() {
            sink.stop();
        }
        if let Some(crossfade_sink) = self.crossfade_sink.take() {
            crossfade_sink.stop();
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "crossfade timing uses f64 but rodio volume controls require f32"
)]
fn crossfade_factor(factor: f64) -> f32 {
    factor as f32
}

/// Main audio thread function.
fn run_audio_thread(
    command_rx: &Receiver<AudioCommand>,
    shared_state: &Arc<SharedState>,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: &Arc<AtomicBool>,
) {
    info!("Rust audio playback thread starting");
    let mut audio_thread = AudioThread::new(shared_state, spectrum_producer, spectrum_enabled);

    loop {
        match audio_thread.next_event(command_rx) {
            AudioThreadEvent::Command(command) => {
                if !audio_thread.handle_command(command) {
                    break;
                }
            }
            AudioThreadEvent::Timeout => audio_thread.handle_timeout(),
            AudioThreadEvent::Disconnected => {
                warn!("Audio command channel disconnected; playback thread exiting");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_request(song_id: &str, duration_secs: f64) -> ActiveAudioRequest {
        ActiveAudioRequest::new(
            Arc::<[u8]>::from(vec![0_u8; 4]),
            SongMetadata {
                id: song_id.to_string(),
                title: song_id.to_string(),
                artist: "artist".to_string(),
                album: "album".to_string(),
                cover_art_id: None,
            },
            duration_secs,
            None,
            None,
            None,
            None,
        )
    }

    fn disconnected_audio_player() -> AudioPlayer {
        let (command_tx, command_rx) = mpsc::channel::<AudioCommand>();
        drop(command_rx);
        let ring_buffer = HeapRb::<f32>::new(SPECTRUM_BUFFER_SIZE);
        let (_producer, consumer) = ring_buffer.split();
        AudioPlayer {
            command_tx,
            shared_state: Arc::new(SharedState::new()),
            spectrum_consumer: Arc::new(Mutex::new(consumer)),
            spectrum_enabled: Arc::new(AtomicBool::new(false)),
            _audio_thread: thread::spawn(|| {}),
        }
    }

    #[test]
    fn play_returns_error_when_audio_thread_is_disconnected() {
        let player = disconnected_audio_player();
        let result = player.play(
            Arc::<[u8]>::from(vec![0_u8; 4]),
            SongMetadata {
                id: "a".to_string(),
                title: "a".to_string(),
                artist: "artist".to_string(),
                album: "album".to_string(),
                cover_art_id: None,
            },
            30.0,
            None,
            None,
            None,
            None,
        );

        assert!(result.is_err());
        assert_eq!(player.get_status().state, PlaybackLifecycleState::Stopped);
    }

    #[test]
    fn append_gapless_returns_error_when_audio_thread_is_disconnected() {
        let player = disconnected_audio_player();
        let result = player.append_gapless(
            Arc::<[u8]>::from(vec![0_u8; 4]),
            SongMetadata {
                id: "b".to_string(),
                title: "b".to_string(),
                artist: "artist".to_string(),
                album: "album".to_string(),
                cover_art_id: None,
            },
            30.0,
            None,
            None,
            None,
            None,
        );

        assert!(result.is_err());
        assert_eq!(player.get_status().state, PlaybackLifecycleState::Stopped);
    }

    #[test]
    fn consumed_position_does_not_advance_from_wall_clock() {
        let shared = SharedState::new();
        let generation = shared.next_generation();
        shared.set_active_request(generation, test_request("a", 30.0));
        shared.mark_playing();

        std::thread::sleep(Duration::from_millis(20));

        assert!(shared.get_position().abs() < f64::EPSILON);
        assert_eq!(shared.state(), PlaybackLifecycleState::Playing);
    }

    #[test]
    fn stale_generation_position_updates_are_ignored() {
        let shared = SharedState::new();
        let generation = shared.next_generation();
        shared.set_active_request(generation, test_request("a", 30.0));

        assert!(!shared.update_consumed_position(generation + 1, 12.0));
        assert!(shared.get_position().abs() < f64::EPSILON);

        assert!(shared.update_consumed_position(generation, 12.0));
        assert!((shared.get_position() - 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn watchdog_enters_stalled_when_position_freezes() {
        let shared = SharedState::new();
        let generation = shared.next_generation();
        shared.set_active_request(generation, test_request("a", 30.0));
        shared.mark_playing();

        let now = Instant::now();
        let last_progress_at = now
            .checked_sub(STALL_TIMEOUT + Duration::from_millis(1))
            .expect("test process uptime should exceed the stall timeout");
        let grace_until = now
            .checked_sub(Duration::from_millis(1))
            .expect("test process uptime should exceed one millisecond");
        let mut watchdog = PlaybackWatchdog {
            last_progress_at,
            grace_until,
            last_position: 0.0,
        };

        watchdog.observe(0.0, true, &shared);

        assert_eq!(shared.state(), PlaybackLifecycleState::Stalled);
        assert!(!shared.is_playing());
    }

    #[test]
    fn watchdog_recovers_from_stalled_when_position_advances() {
        let shared = SharedState::new();
        let generation = shared.next_generation();
        shared.set_active_request(generation, test_request("a", 30.0));
        shared.mark_stalled();

        let mut watchdog = PlaybackWatchdog::new();
        watchdog.reset(0.0);
        watchdog.observe(1.0, true, &shared);

        assert_eq!(shared.state(), PlaybackLifecycleState::Playing);
        assert!(shared.is_playing());
    }

    #[test]
    fn audio_thread_polls_when_state_is_playing_without_sink() {
        assert!(should_poll_audio_thread(
            false,
            false,
            PlaybackLifecycleState::Playing
        ));
        assert!(!should_poll_audio_thread(
            false,
            false,
            PlaybackLifecycleState::Paused
        ));
    }

    #[test]
    fn missing_sink_guard_marks_playback_stalled() {
        let shared = SharedState::new();
        let generation = shared.next_generation();
        shared.set_active_request(generation, test_request("a", 30.0));
        shared.mark_playing();

        let mut watchdog = PlaybackWatchdog::new();

        assert!(mark_missing_sink_if_playing(&shared, &mut watchdog));
        assert_eq!(shared.state(), PlaybackLifecycleState::Stalled);
        assert!(!shared.is_playing());
    }

    #[test]
    fn gapless_source_position_rollover_becomes_cumulative() {
        let shared = SharedState::new();
        let generation = shared.next_generation();
        let first = test_request("a", 10.0);
        let second = test_request("b", 20.0);
        shared.set_active_request(generation, first);
        {
            let mut inner = shared.write_inner();
            inner.consumed_position = 9.9;
            inner.duration = 30.0;
            inner.gapless_segments.push(GaplessSegment {
                metadata: second.metadata.clone(),
                duration: second.duration_secs,
                cumulative_start: 10.0,
                request: second,
            });
        }

        assert!(shared.update_consumed_position(generation, 0.2));

        let (state, segment_idx) = shared.get_gapless_state();
        assert_eq!(segment_idx, 1);
        assert_eq!(state.song.as_ref().map(|song| song.id.as_str()), Some("b"));
        assert!((state.position - 0.2).abs() < 0.001);
    }
}
