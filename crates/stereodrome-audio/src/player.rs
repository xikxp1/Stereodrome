use log::{debug, error, info, warn};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::Split};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::any::Any;
use std::collections::HashSet;
use std::io::Cursor;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
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
const COMMAND_APPLY_GRACE_TIMEOUT: Duration = Duration::from_millis(250);
const STALL_GRACE_DURATION: Duration = Duration::from_millis(750);
const STALL_TIMEOUT: Duration = Duration::from_millis(2500);
const POSITION_EPSILON_SECONDS: f64 = 0.01;
const AUDIO_THREAD_RESTART_INITIAL_DELAY: Duration = Duration::from_millis(50);
const AUDIO_THREAD_RESTART_MAX_DELAY: Duration = Duration::from_secs(2);
const AUDIO_THREAD_MAX_AUTOMATIC_RESTARTS: u32 = 5;
const AUDIO_THREAD_FAILURE_RESET_WINDOW: Duration = Duration::from_secs(30);
const COMMAND_PENDING: u8 = 0;
const COMMAND_COMMITTED: u8 = 1;
const COMMAND_CANCELLED: u8 = 2;
const COMMAND_APPLYING: u8 = 3;
const COMMAND_APPLIED: u8 = 4;
const COMMAND_ABORT_REQUESTED: u8 = 5;

/// Coordinates an expensive start-like command with its waiting caller.
///
/// Preparation may outlive the acknowledgement timeout, but the command may
/// only mutate playback after atomically committing. If the caller cancels
/// first, the prepared sink and any newly opened output are discarded.
#[derive(Debug, Default)]
struct CommandPermit {
    state: AtomicU8,
}

impl CommandPermit {
    fn try_commit(&self) -> bool {
        self.state
            .compare_exchange(
                COMMAND_PENDING,
                COMMAND_COMMITTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn cancel(&self) -> bool {
        self.state
            .compare_exchange(
                COMMAND_PENDING,
                COMMAND_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn begin_apply(&self) -> bool {
        self.state
            .compare_exchange(
                COMMAND_COMMITTED,
                COMMAND_APPLYING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn finish_apply(&self) {
        self.state.store(COMMAND_APPLIED, Ordering::Release);
    }

    fn abort_committed(&self) -> bool {
        self.state
            .compare_exchange(
                COMMAND_COMMITTED,
                COMMAND_ABORT_REQUESTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn is_applied(&self) -> bool {
        self.state.load(Ordering::Acquire) == COMMAND_APPLIED
    }
}

fn wait_for_start_result(
    name: &str,
    permit: &CommandPermit,
    ack_rx: &Receiver<AudioResult<()>>,
) -> AudioResult<()> {
    match ack_rx.recv_timeout(TRANSPORT_ACK_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) if permit.cancel() => Err(AudioError::Playback(
            format!("Audio thread did not acknowledge {name} command in time; command cancelled"),
        )),
        Err(mpsc::RecvTimeoutError::Timeout) => match ack_rx.recv_timeout(TRANSPORT_ACK_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) if permit.abort_committed() => {
                Err(AudioError::Playback(format!(
                    "Audio thread did not apply committed {name} command in time; command aborted"
                )))
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected)
                if permit.is_applied() =>
            {
                Ok(())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                match ack_rx.recv_timeout(COMMAND_APPLY_GRACE_TIMEOUT) {
                    Ok(result) => result,
                    Err(_) if permit.is_applied() => Ok(()),
                    Err(_) => Err(AudioError::Playback(format!(
                        "Audio thread did not finish {name} command apply phase in time"
                    ))),
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AudioError::Playback(format!(
                "Audio thread disconnected while committing {name} command"
            ))),
        },
        Err(mpsc::RecvTimeoutError::Disconnected) if permit.is_applied() => Ok(()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(AudioError::Playback(format!(
            "Audio thread disconnected while applying {name} command"
        ))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackLifecycleState {
    Playing,
    Paused,
    Stopped,
    Stalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioOutputState {
    Closed,
    Ready,
    Failed,
    Unavailable,
}

impl AudioOutputState {
    const fn as_u8(self) -> u8 {
        match self {
            Self::Closed => 0,
            Self::Ready => 1,
            Self::Failed => 2,
            Self::Unavailable => 3,
        }
    }

    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Ready,
            2 => Self::Failed,
            3 => Self::Unavailable,
            _ => Self::Closed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioNotification {
    PlaybackChanged {
        identity: Option<PlaybackIdentity>,
        state: PlaybackLifecycleState,
    },
    GaplessSegmentChanged {
        identity: PlaybackIdentity,
        segment_index: usize,
    },
    EndOfTrack {
        identity: PlaybackIdentity,
    },
    PositionChanged {
        identity: PlaybackIdentity,
    },
    OutputStateChanged {
        state: AudioOutputState,
        message: Option<String>,
    },
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
    pub output_state: AudioOutputState,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackState {
    pub state: PlaybackLifecycleState,
    pub is_playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub song: Option<SongMetadata>,
    pub output_state: AudioOutputState,
}

/// Identifies the exact source and song that were active when asynchronous
/// playback work was started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackIdentity {
    generation: u64,
    song_id: String,
}

impl PlaybackIdentity {
    #[must_use]
    pub fn song_id(&self) -> &str {
        &self.song_id
    }
}

/// Commands sent to the audio thread
enum AudioCommand {
    Play {
        expected_playback: Option<PlaybackIdentity>,
        audio_data: Arc<[u8]>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
        permit: Arc<CommandPermit>,
        ack: Sender<AudioResult<()>>,
    },
    Pause {
        ack: Sender<AudioResult<()>>,
    },
    Resume {
        permit: Arc<CommandPermit>,
        ack: Sender<AudioResult<()>>,
    },
    RebuildOutput {
        permit: Arc<CommandPermit>,
        ack: Sender<AudioResult<()>>,
    },
    Stop {
        ack: Sender<AudioResult<()>>,
    },
    SetVolume(f32),
    Seek {
        position_secs: f64,
        ack: Sender<AudioResult<()>>,
    },
    /// Append a song to the existing player for gapless playback.
    /// Unlike Play, this does NOT create a new player.
    AppendGapless {
        request: Box<GaplessAppendRequest>,
        ack: Sender<AudioResult<()>>,
    },
    /// Crossfade to a new song: keep the current sink fading out
    /// while a new sink fades in over the specified duration.
    CrossfadePlay {
        expected_playback: Option<PlaybackIdentity>,
        audio_data: Arc<[u8]>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
        crossfade_duration_ms: u32,
        permit: Arc<CommandPermit>,
        ack: Sender<AudioResult<()>>,
    },
    Shutdown,
}

#[derive(Debug)]
pub struct CrossfadePlayRequest {
    pub expected_playback: Option<PlaybackIdentity>,
    pub audio_data: Arc<[u8]>,
    pub metadata: SongMetadata,
    pub duration_secs: f64,
    pub normalization_gain: Option<f32>,
    pub dynamics_preset: Option<DynamicsPreset>,
    pub binaural_preset: Option<BinauralPreset>,
    pub equalizer_settings: Option<EqualizerSettings>,
    pub crossfade_duration_ms: u32,
}

struct GaplessAppendRequest {
    expected_playback: PlaybackIdentity,
    audio_data: Arc<[u8]>,
    metadata: SongMetadata,
    duration_secs: f64,
    normalization_gain: Option<f32>,
    dynamics_preset: Option<DynamicsPreset>,
    binaural_preset: Option<BinauralPreset>,
    equalizer_settings: Option<EqualizerSettings>,
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
    output_state: AtomicU8,
    crossfade_initiated: AtomicBool,
    next_source_generation: AtomicU64,
    next_output_generation: AtomicU64,
    output_generations: Mutex<OutputGenerationState>,
    notifications: Sender<AudioNotification>,
    inner: RwLock<PlaybackInner>,
}

#[derive(Default)]
struct OutputGenerationState {
    active: u64,
    failed: HashSet<u64>,
}

impl SharedState {
    fn new(notifications: Sender<AudioNotification>) -> Self {
        Self {
            is_playing: AtomicBool::new(false),
            stalled: AtomicBool::new(false),
            output_state: AtomicU8::new(AudioOutputState::Closed.as_u8()),
            crossfade_initiated: AtomicBool::new(false),
            next_source_generation: AtomicU64::new(1),
            next_output_generation: AtomicU64::new(1),
            output_generations: Mutex::new(OutputGenerationState::default()),
            notifications,
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
        self.stalled.store(false, Ordering::SeqCst);
        self.is_playing.store(true, Ordering::SeqCst);
    }

    fn mark_output_ready_for(&self, generation: u64) -> bool {
        let mut generations = self
            .output_generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if generations.failed.remove(&generation) {
            return false;
        }
        generations.active = generation;
        let changed = self
            .output_state
            .swap(AudioOutputState::Ready.as_u8(), Ordering::SeqCst)
            != AudioOutputState::Ready.as_u8();
        drop(generations);
        if changed {
            self.notify(AudioNotification::OutputStateChanged {
                state: AudioOutputState::Ready,
                message: None,
            });
        }
        true
    }

    fn begin_output_apply(&self, generation: u64, permit: &CommandPermit) -> bool {
        let mut generations = self
            .output_generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if generations.failed.remove(&generation) || !permit.begin_apply() {
            return false;
        }
        generations.active = generation;
        let changed = self
            .output_state
            .swap(AudioOutputState::Ready.as_u8(), Ordering::SeqCst)
            != AudioOutputState::Ready.as_u8();
        drop(generations);
        if changed {
            self.notify(AudioNotification::OutputStateChanged {
                state: AudioOutputState::Ready,
                message: None,
            });
        }
        true
    }

    fn commit_current_output(&self, permit: &CommandPermit) -> bool {
        let generations = self
            .output_generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        generations.active != 0
            && !generations.failed.contains(&generations.active)
            && self.output_state() == AudioOutputState::Ready
            && permit.try_commit()
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
        self.set_output_state(AudioOutputState::Failed, None);
        self.mark_stalled();
        self.notify_playback_changed();
    }

    fn mark_stream_failed_for(&self, generation: u64, message: String) {
        let mut generations = self
            .output_generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        generations.failed.insert(generation);
        if generations.active != generation {
            return;
        }
        self.output_state
            .store(AudioOutputState::Failed.as_u8(), Ordering::SeqCst);
        self.mark_stalled();
        drop(generations);
        self.notify(AudioNotification::OutputStateChanged {
            state: AudioOutputState::Failed,
            message: Some(message),
        });
        self.notify_playback_changed();
    }

    fn next_output_generation(&self) -> u64 {
        self.next_output_generation.fetch_add(1, Ordering::SeqCst)
    }

    fn invalidate_output_generation(&self) {
        self.output_generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active = 0;
    }

    fn mark_output_closed(&self) {
        self.invalidate_output_generation();
        self.set_output_state(AudioOutputState::Closed, None);
    }

    fn mark_output_unavailable(&self, message: String) {
        self.set_output_state(AudioOutputState::Unavailable, Some(message));
        self.mark_stalled();
        self.notify_playback_changed();
    }

    fn output_state(&self) -> AudioOutputState {
        AudioOutputState::from_u8(self.output_state.load(Ordering::SeqCst))
    }

    fn set_output_state(&self, state: AudioOutputState, message: Option<String>) {
        let previous = self.output_state.swap(state.as_u8(), Ordering::SeqCst);
        if previous != state.as_u8() || message.is_some() {
            self.notify(AudioNotification::OutputStateChanged { state, message });
        }
    }

    fn notify(&self, event: AudioNotification) {
        let _ = self.notifications.send(event);
    }

    fn notify_playback_changed(&self) {
        self.notify(AudioNotification::PlaybackChanged {
            identity: self.playback_identity(),
            state: self.state(),
        });
    }

    fn mark_stopped(&self) {
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

    fn has_active_request(&self) -> bool {
        self.read_inner().active_request.is_some()
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
                    output_state: self.output_state(),
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
                    output_state: self.output_state(),
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
            output_state: state.output_state,
        }
    }

    fn playback_identity(&self) -> Option<PlaybackIdentity> {
        let inner = self.read_inner();
        let song_id = if inner.gapless_segments.is_empty() {
            inner.current_song.as_ref().map(|song| song.id.clone())
        } else {
            let segment_idx = Self::segment_index_for_current_position(&inner);
            Some(inner.gapless_segments[segment_idx].metadata.id.clone())
        }?;

        Some(PlaybackIdentity {
            generation: inner.source_generation,
            song_id,
        })
    }
}

pub struct AudioPlayer {
    command_tx: Sender<AudioCommand>,
    shared_state: Arc<SharedState>,
    spectrum_consumer: Arc<Mutex<HeapCons<f32>>>,
    spectrum_enabled: Arc<AtomicBool>,
    audio_thread: Option<JoinHandle<()>>,
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
        Self::new_with_spectrum_and_notifications(spectrum_enabled).map(|(player, _)| player)
    }

    /// Creates an audio player and a transition-only notification receiver.
    ///
    /// Position advancement is intentionally not emitted; consumers receive
    /// only lifecycle, segment, terminal, discontinuity, and output changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the audio player cannot be initialized.
    pub fn new_with_spectrum_and_notifications(
        spectrum_enabled: bool,
    ) -> AudioResult<(Self, Receiver<AudioNotification>)> {
        let (command_tx, command_rx) = mpsc::channel::<AudioCommand>();
        let (notification_tx, notification_rx) = mpsc::channel::<AudioNotification>();
        let shared_state = Arc::new(SharedState::new(notification_tx));
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
            supervise_audio_thread(
                &command_rx,
                &state_clone,
                &producer_clone,
                &spectrum_enabled_clone,
            );
        });

        Ok((
            Self {
                command_tx,
                shared_state,
                spectrum_consumer,
                spectrum_enabled,
                audio_thread: Some(audio_thread),
            },
            notification_rx,
        ))
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
        self.play_with_expected(
            None,
            audio_data,
            metadata,
            duration_secs,
            normalization_gain,
            dynamics_preset,
            binaural_preset,
            equalizer_settings,
        )
    }

    #[allow(clippy::too_many_arguments)]
    /// Starts playback only if the supplied identity is still current.
    ///
    /// # Errors
    ///
    /// Returns an error if playback changed, preparation was cancelled, or
    /// the source/output cannot be started.
    pub fn play_with_expected(
        &self,
        expected_playback: Option<PlaybackIdentity>,
        audio_data: Arc<[u8]>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    ) -> AudioResult<()> {
        self.send_start_command("play", |permit, ack| AudioCommand::Play {
            expected_playback,
            audio_data,
            metadata,
            duration_secs,
            normalization_gain,
            dynamics_preset,
            binaural_preset,
            equalizer_settings,
            permit,
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
        expected_playback: PlaybackIdentity,
        audio_data: Arc<[u8]>,
        metadata: SongMetadata,
        duration_secs: f64,
        normalization_gain: Option<f32>,
        dynamics_preset: Option<DynamicsPreset>,
        binaural_preset: Option<BinauralPreset>,
        equalizer_settings: Option<EqualizerSettings>,
    ) -> AudioResult<()> {
        self.send_result_command("append gapless", |ack| AudioCommand::AppendGapless {
            request: Box::new(GaplessAppendRequest {
                expected_playback,
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
            }),
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
            expected_playback,
            audio_data,
            metadata,
            duration_secs,
            normalization_gain,
            dynamics_preset,
            binaural_preset,
            equalizer_settings,
            crossfade_duration_ms,
        } = request;

        let permit = Arc::new(CommandPermit::default());
        let (ack_tx, ack_rx) = mpsc::channel();
        self.command_tx
            .send(AudioCommand::CrossfadePlay {
                expected_playback,
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
                crossfade_duration_ms,
                permit: Arc::clone(&permit),
                ack: ack_tx,
            })
            .map_err(|e| AudioError::Playback(format!("Failed to send crossfade command: {e}")))?;
        wait_for_start_result("crossfade", &permit, &ack_rx)
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

    fn send_start_command(
        &self,
        name: &str,
        make_command: impl FnOnce(Arc<CommandPermit>, Sender<AudioResult<()>>) -> AudioCommand,
    ) -> AudioResult<()> {
        let permit = Arc::new(CommandPermit::default());
        let (ack_tx, ack_rx) = mpsc::channel();
        self.command_tx
            .send(make_command(Arc::clone(&permit), ack_tx))
            .map_err(|e| AudioError::Playback(format!("Failed to send {name} command: {e}")))?;
        wait_for_start_result(name, &permit, &ack_rx)
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
        self.send_result_command("pause", |ack| AudioCommand::Pause { ack })
    }

    /// Resumes the current source, rebuilding output if necessary.
    ///
    /// # Errors
    ///
    /// Returns an error if the source cannot be resumed or rebuilt.
    pub fn resume(&self) -> AudioResult<()> {
        self.send_start_command("resume", |permit, ack| AudioCommand::Resume { permit, ack })
    }

    /// Rebuilds the output stream for the active source.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no active source or output cannot be opened.
    pub fn rebuild_output(&self) -> AudioResult<()> {
        self.send_start_command("rebuild output", |permit, ack| {
            AudioCommand::RebuildOutput { permit, ack }
        })
    }

    /// Stops playback and clears the active source.
    ///
    /// # Errors
    ///
    /// Returns an error if the audio thread cannot apply the command.
    pub fn stop(&self) -> AudioResult<()> {
        self.send_result_command("stop", |ack| AudioCommand::Stop { ack })
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
        self.send_result_command("seek", |ack| AudioCommand::Seek {
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

    /// Returns an identity token for the exact source and song currently active.
    #[must_use]
    pub fn current_playback_identity(&self) -> Option<PlaybackIdentity> {
        self.shared_state.playback_identity()
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
        if let Some(audio_thread) = self.audio_thread.take()
            && let Err(payload) = audio_thread.join()
        {
            error!(
                "Rust audio playback supervisor panicked during shutdown: {}",
                panic_payload_message(payload.as_ref())
            );
        }
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

    fn requires_poll(&self) -> bool {
        !self.paused
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
    Command(Box<AudioCommand>),
    Timeout,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioThreadExit {
    Shutdown,
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

struct OpenedOutput {
    stream: MixerDeviceSink,
    generation: u64,
}

fn open_output_stream(shared_state: &Arc<SharedState>) -> AudioResult<OpenedOutput> {
    let generation = shared_state.next_output_generation();
    let callback_state = Arc::clone(shared_state);
    let mut stream = DeviceSinkBuilder::from_default_device()
        .map_err(|e| AudioError::Playback(format!("Failed to create audio stream builder: {e:?}")))?
        .with_error_callback(move |error| {
            warn!("Rust audio stream error: {error:?}");
            callback_state.mark_stream_failed_for(generation, format!("{error:?}"));
        })
        .open_sink_or_fallback()
        .map_err(|e| AudioError::Playback(format!("Failed to open audio stream: {e:?}")))?;
    stream.log_on_drop(false);
    Ok(OpenedOutput { stream, generation })
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
            AudioError::Decode(format!(
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
    // Keep preparation silent until the command atomically commits. This
    // prevents a timed-out command from becoming audible before cancellation.
    sink.pause();
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
            Ok(next_output) => {
                info!("Rust audio output stream opened");
                if !shared_state.mark_output_ready_for(next_output.generation) {
                    return Err(AudioError::Playback(
                        "Audio output failed before it could be activated".to_string(),
                    ));
                }
                *stream = Some(next_output.stream);
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

struct RebuiltOutput {
    generation: u64,
    position: f64,
    previous_stream: Option<MixerDeviceSink>,
    previous_sink: Option<Player>,
    previous_crossfade_sink: Option<Player>,
}

impl RebuiltOutput {
    fn cleanup(self) {
        if let Some(sink) = self.previous_sink {
            sink.stop();
        }
        if let Some(sink) = self.previous_crossfade_sink {
            sink.stop();
        }
        drop(self.previous_stream);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "rebuild prepares replacement output across the audio thread's owned slots"
)]
fn rebuild_active_output(
    shared_state: &Arc<SharedState>,
    stream: &mut Option<MixerDeviceSink>,
    current_sink: &mut Option<Player>,
    crossfade_sink: &mut Option<Player>,
    crossfade_state: &mut Option<CrossfadeState>,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: &Arc<AtomicBool>,
    permit: &CommandPermit,
) -> AudioResult<RebuiltOutput> {
    let Some((request, position)) = shared_state.active_request_at_position() else {
        return Err(AudioError::Playback(
            "Cannot rebuild audio output without an active source".to_string(),
        ));
    };

    let next_output = match open_output_stream(shared_state) {
        Ok(output) => output,
        Err(e) => {
            shared_state.mark_stream_failed();
            return Err(e);
        }
    };
    let volume = shared_state.read_inner().volume;
    let sink = match connect_request_sink(
        &next_output.stream,
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

    if !permit.try_commit() {
        return Err(AudioError::Playback(
            "Audio output rebuild was cancelled before commit".to_string(),
        ));
    }
    sink.play();
    if !shared_state.begin_output_apply(next_output.generation, permit) {
        sink.stop();
        return Err(AudioError::Playback(
            "Audio output rebuild was aborted or failed before apply".to_string(),
        ));
    }

    let generation = shared_state.next_generation();

    let previous_sink = current_sink.take();
    let previous_crossfade_sink = crossfade_sink.take();
    *crossfade_state = None;
    shared_state
        .crossfade_initiated
        .store(false, Ordering::SeqCst);

    let previous_stream = stream.replace(next_output.stream);
    shared_state.replace_with_rebuilt_request(generation, request, restored_position);
    shared_state.mark_playing();
    *current_sink = Some(sink);
    permit.finish_apply();
    Ok(RebuiltOutput {
        generation,
        position: restored_position,
        previous_stream,
        previous_sink,
        previous_crossfade_sink,
    })
}

fn should_poll_audio_thread(
    current_sink_active: bool,
    crossfade_active: bool,
    lifecycle_state: PlaybackLifecycleState,
) -> bool {
    current_sink_active || crossfade_active || lifecycle_state == PlaybackLifecycleState::Playing
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

fn matches_expected_playback(
    shared_state: &SharedState,
    current_generation: u64,
    expected_playback: &PlaybackIdentity,
) -> bool {
    current_generation == expected_playback.generation
        && shared_state.playback_identity().as_ref() == Some(expected_playback)
}

impl<'a> AudioThread<'a> {
    fn new(
        shared_state: &'a Arc<SharedState>,
        spectrum_producer: &'a Arc<Mutex<HeapProd<f32>>>,
        spectrum_enabled: &'a Arc<AtomicBool>,
    ) -> Self {
        Self {
            shared_state,
            spectrum_producer,
            spectrum_enabled,
            // Platform audio focus/session is acquired by the native caller
            // before a start command. Open the device only when that command
            // reaches this worker.
            stream: None,
            current_sink: None,
            crossfade_sink: None,
            crossfade_state: None,
            current_generation: 0,
            watchdog: PlaybackWatchdog::new(),
        }
    }

    fn next_event(&self, command_rx: &Receiver<AudioCommand>) -> AudioThreadEvent {
        let output_ready = self.shared_state.output_state() == AudioOutputState::Ready;
        let current_sink_active = output_ready
            && self
                .current_sink
                .as_ref()
                .is_some_and(|sink| !sink.is_paused() && !sink.empty());
        let crossfade_active = output_ready
            && (self
                .crossfade_state
                .as_ref()
                .is_some_and(CrossfadeState::requires_poll)
                || self
                    .crossfade_sink
                    .as_ref()
                    .is_some_and(|sink| !sink.is_paused() && !sink.empty()));
        if should_poll_audio_thread(
            current_sink_active,
            crossfade_active,
            self.shared_state.state(),
        ) {
            match command_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(command) => AudioThreadEvent::Command(Box::new(command)),
                Err(mpsc::RecvTimeoutError::Timeout) => AudioThreadEvent::Timeout,
                Err(mpsc::RecvTimeoutError::Disconnected) => AudioThreadEvent::Disconnected,
            }
        } else {
            match command_rx.recv() {
                Ok(command) => AudioThreadEvent::Command(Box::new(command)),
                Err(_) => AudioThreadEvent::Disconnected,
            }
        }
    }

    fn handle_command(&mut self, command: AudioCommand) -> bool {
        match command {
            AudioCommand::Play {
                expected_playback,
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
                permit,
                ack,
            } => self.play(
                expected_playback.as_ref(),
                ActiveAudioRequest::new(
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    equalizer_settings,
                ),
                &permit,
                &ack,
            ),
            AudioCommand::Pause { ack } => self.pause(&ack),
            AudioCommand::Resume { permit, ack } => self.resume(&permit, &ack),
            AudioCommand::RebuildOutput { permit, ack } => self.rebuild_output(&permit, &ack),
            AudioCommand::Stop { ack } => self.stop(&ack),
            AudioCommand::SetVolume(volume) => self.set_volume(volume),
            AudioCommand::Seek { position_secs, ack } => self.seek(position_secs, &ack),
            AudioCommand::AppendGapless { request, ack } => {
                let GaplessAppendRequest {
                    expected_playback,
                    audio_data,
                    metadata,
                    duration_secs,
                    normalization_gain,
                    dynamics_preset,
                    binaural_preset,
                    equalizer_settings,
                } = *request;
                self.append_gapless(
                    &expected_playback,
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
                );
            }
            AudioCommand::CrossfadePlay {
                expected_playback,
                audio_data,
                metadata,
                duration_secs,
                normalization_gain,
                dynamics_preset,
                binaural_preset,
                equalizer_settings,
                crossfade_duration_ms,
                permit,
                ack,
            } => self.crossfade_play(
                expected_playback.as_ref(),
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
                &permit,
                &ack,
            ),
            AudioCommand::Shutdown => {
                self.shutdown();
                return false;
            }
        }
        true
    }

    fn play(
        &mut self,
        expected_playback: Option<&PlaybackIdentity>,
        request: ActiveAudioRequest,
        permit: &CommandPermit,
        ack: &Sender<AudioResult<()>>,
    ) {
        info!(
            "Playback thread play: song_id={}, title={:?}, bytes={}, duration={:.3}s",
            request.metadata.id,
            request.metadata.title,
            request.audio_data.len(),
            request.duration_secs
        );
        let volume = self.shared_state.read_inner().volume;
        let had_output = self.stream.is_some();
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
                if expected_playback.is_some_and(|expected| {
                    !matches_expected_playback(self.shared_state, self.current_generation, expected)
                }) {
                    let _ = ack.send(Err(AudioError::Playback(
                        "Play command was rejected because playback changed".to_string(),
                    )));
                    return;
                }
                if !self.shared_state.commit_current_output(permit) {
                    if !had_output && self.current_sink.is_none() {
                        self.stream.take();
                        self.shared_state.mark_output_closed();
                    }
                    let _ = ack.send(Err(AudioError::Playback(
                        "Play command was cancelled before commit".to_string(),
                    )));
                    return;
                }
                sink.play();
                if !permit.begin_apply() {
                    sink.stop();
                    if !had_output && self.current_sink.is_none() {
                        self.stream.take();
                        self.shared_state.mark_output_closed();
                    }
                    let _ = ack.send(Err(AudioError::Playback(
                        "Play command was aborted before apply".to_string(),
                    )));
                    return;
                }
                let old_sink = self.current_sink.take();
                let old_crossfade_sink = self.crossfade_sink.take();
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
                self.shared_state.notify_playback_changed();
                permit.finish_apply();
                debug!("Playback thread started song");
                let _ = ack.send(Ok(()));
                if let Some(old_sink) = old_sink {
                    debug!("Stopping replaced sink after starting new track");
                    old_sink.stop();
                }
                if let Some(old_crossfade_sink) = old_crossfade_sink {
                    debug!("Stopping replaced crossfade sink after starting new track");
                    old_crossfade_sink.stop();
                }
            }
            Err(error) => {
                if !had_output && self.current_sink.is_none() {
                    self.stream.take();
                    self.shared_state.mark_output_closed();
                }
                error!("{error}");
                let _ = ack.send(Err(error));
            }
        }
    }

    fn pause(&mut self, ack: &Sender<AudioResult<()>>) {
        let previous_state = self.shared_state.state();
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
        if self.shared_state.state() != previous_state {
            self.shared_state.notify_playback_changed();
        }
        let _ = ack.send(Ok(()));
    }

    fn resume(&mut self, permit: &CommandPermit, ack: &Sender<AudioResult<()>>) {
        let can_resume = self
            .current_sink
            .as_ref()
            .is_some_and(|sink| !sink.empty() && !self.shared_state.stalled.load(Ordering::SeqCst));
        if can_resume {
            if !permit.try_commit() {
                let _ = ack.send(Err(AudioError::Playback(
                    "Resume command was cancelled before commit".to_string(),
                )));
                return;
            }
            let was_paused = self
                .current_sink
                .as_ref()
                .is_some_and(rodio::Player::is_paused);
            if was_paused {
                let sink = self
                    .current_sink
                    .as_ref()
                    .expect("resumable sink checked above");
                sink.play();
                if let Some(ref crossfade_sink) = self.crossfade_sink {
                    crossfade_sink.play();
                }
                if let Some(ref mut crossfade_state) = self.crossfade_state {
                    crossfade_state.resume();
                }
            }
            if !permit.begin_apply() {
                if was_paused {
                    if let Some(ref sink) = self.current_sink {
                        sink.pause();
                    }
                    if let Some(ref crossfade_sink) = self.crossfade_sink {
                        crossfade_sink.pause();
                    }
                    if let Some(ref mut crossfade_state) = self.crossfade_state {
                        crossfade_state.pause();
                    }
                }
                let _ = ack.send(Err(AudioError::Playback(
                    "Resume command was aborted before apply".to_string(),
                )));
                return;
            }
            let position = self.current_sink.as_ref().map_or_else(
                || self.shared_state.get_position(),
                |sink| sink.get_pos().as_secs_f64(),
            );
            self.shared_state.mark_playing();
            self.watchdog.reset(position);
            self.shared_state.notify_playback_changed();
            permit.finish_apply();
            debug!("Playback thread ensured current sink is playing");
            let _ = ack.send(Ok(()));
        } else {
            self.rebuild_output_and_ack(permit, ack);
        }
    }

    fn rebuild_output(&mut self, permit: &CommandPermit, ack: &Sender<AudioResult<()>>) {
        self.rebuild_output_and_ack(permit, ack);
    }

    fn rebuild_output_and_ack(&mut self, permit: &CommandPermit, ack: &Sender<AudioResult<()>>) {
        let result = rebuild_active_output(
            self.shared_state,
            &mut self.stream,
            &mut self.current_sink,
            &mut self.crossfade_sink,
            &mut self.crossfade_state,
            self.spectrum_producer,
            self.spectrum_enabled,
            permit,
        );
        match result {
            Ok(rebuilt) => {
                self.current_generation = rebuilt.generation;
                self.watchdog.reset(rebuilt.position);
                self.shared_state.notify_playback_changed();
                let _ = ack.send(Ok(()));
                rebuilt.cleanup();
            }
            Err(error) => {
                let _ = ack.send(Err(error));
            }
        }
    }

    fn stop(&mut self, ack: &Sender<AudioResult<()>>) {
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
        self.stream.take();
        self.shared_state.mark_output_closed();
        self.shared_state.notify_playback_changed();
        let _ = ack.send(Ok(()));
    }

    fn set_volume(&self, volume: f32) {
        debug!("Playback thread set volume: {volume:.3}");
        if self.crossfade_state.is_none()
            && let Some(ref sink) = self.current_sink
        {
            sink.set_volume(volume);
        }
    }

    fn seek(&mut self, position_secs: f64, ack: &Sender<AudioResult<()>>) {
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

        let result = if let Some(ref sink) = self.current_sink {
            let (seek_position, cumulative_position) = self.seek_positions(position_secs);
            if let Err(error) = sink.try_seek(Duration::from_secs_f64(seek_position)) {
                warn!("Seek failed: {error:?}");
                Err(AudioError::Playback(format!(
                    "Failed to seek audio: {error:?}"
                )))
            } else {
                self.shared_state
                    .set_consumed_position(self.current_generation, cumulative_position);
                if !sink.is_paused() {
                    self.shared_state.mark_playing();
                }
                self.watchdog.reset(cumulative_position);
                if let Some(identity) = self.shared_state.playback_identity() {
                    self.shared_state
                        .notify(AudioNotification::PositionChanged { identity });
                }
                debug!(
                    "Playback thread seek complete: segment={seek_position:.3}s \
                     cumulative={cumulative_position:.3}s"
                );
                Ok(())
            }
        } else {
            Err(AudioError::Playback(
                "Cannot seek because there is no active sink".to_string(),
            ))
        };
        let _ = ack.send(result);
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

    fn append_gapless(
        &self,
        expected_playback: &PlaybackIdentity,
        request: ActiveAudioRequest,
        ack: &Sender<AudioResult<()>>,
    ) {
        info!(
            "Playback thread append gapless: song_id={}, title={:?}, bytes={}, duration={:.3}s",
            request.metadata.id,
            request.metadata.title,
            request.audio_data.len(),
            request.duration_secs
        );
        if !matches_expected_playback(
            self.shared_state,
            self.current_generation,
            expected_playback,
        ) {
            let error = AudioError::Playback(format!(
                "Cannot append gapless track {} because playback changed",
                request.metadata.id
            ));
            debug!("{error}");
            let _ = ack.send(Err(error));
            return;
        }

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
        expected_playback: Option<&PlaybackIdentity>,
        request: ActiveAudioRequest,
        crossfade_duration_ms: u32,
        permit: &CommandPermit,
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
        let had_output = self.stream.is_some();
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
                if expected_playback.is_some_and(|expected| {
                    !matches_expected_playback(self.shared_state, self.current_generation, expected)
                }) {
                    self.shared_state
                        .crossfade_initiated
                        .store(false, Ordering::SeqCst);
                    let _ = ack.send(Err(AudioError::Playback(
                        "Crossfade command was rejected because playback changed".to_string(),
                    )));
                    return;
                }
                if !self.shared_state.commit_current_output(permit) {
                    if !had_output && self.current_sink.is_none() {
                        self.stream.take();
                        self.shared_state.mark_output_closed();
                    }
                    self.shared_state
                        .crossfade_initiated
                        .store(false, Ordering::SeqCst);
                    let _ = ack.send(Err(AudioError::Playback(
                        "Crossfade command was cancelled before commit".to_string(),
                    )));
                    return;
                }
                new_sink.play();
                if !permit.begin_apply() {
                    new_sink.stop();
                    if !had_output && self.current_sink.is_none() {
                        self.stream.take();
                        self.shared_state.mark_output_closed();
                    }
                    self.shared_state
                        .crossfade_initiated
                        .store(false, Ordering::SeqCst);
                    let _ = ack.send(Err(AudioError::Playback(
                        "Crossfade command was aborted before apply".to_string(),
                    )));
                    return;
                }
                let old_crossfade_sink = self.crossfade_sink.take();
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
                self.shared_state.notify_playback_changed();
                permit.finish_apply();
                debug!("Playback thread started crossfade");
                let _ = ack.send(Ok(()));
                if let Some(old_crossfade_sink) = old_crossfade_sink {
                    debug!("Stopping previous crossfade sink");
                    old_crossfade_sink.stop();
                }
            }
            Err(error) => {
                if !had_output && self.current_sink.is_none() {
                    self.stream.take();
                    self.shared_state.mark_output_closed();
                }
                error!("{error}");
                self.shared_state
                    .crossfade_initiated
                    .store(false, Ordering::SeqCst);
                let _ = ack.send(Err(error));
            }
        }
    }

    fn handle_timeout(&mut self) {
        let previous_state = self.shared_state.state();
        let previous_segment = self.shared_state.get_gapless_state().1;
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
        let current_segment = self.shared_state.get_gapless_state().1;
        if current_segment != previous_segment
            && let Some(identity) = self.shared_state.playback_identity()
        {
            self.shared_state
                .notify(AudioNotification::GaplessSegmentChanged {
                    identity,
                    segment_index: current_segment,
                });
        }
        if self.shared_state.state() != previous_state {
            self.shared_state.notify_playback_changed();
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
            if let Some(identity) = self.shared_state.playback_identity() {
                self.shared_state
                    .notify(AudioNotification::EndOfTrack { identity });
            }
            self.shared_state.notify_playback_changed();
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
        self.stream.take();
        self.shared_state.mark_stopped();
        self.shared_state.mark_output_closed();
        self.shared_state.notify_playback_changed();
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "crossfade timing uses f64 but rodio volume controls require f32"
)]
fn crossfade_factor(factor: f64) -> f32 {
    factor as f32
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "non-string panic payload"
    }
}

enum SupervisorWait {
    Retry(Option<Box<AudioCommand>>),
    Shutdown,
    Disconnected,
}

fn audio_restart_delay(failures: u32) -> Duration {
    let exponent = failures.saturating_sub(1).min(16);
    AUDIO_THREAD_RESTART_INITIAL_DELAY
        .saturating_mul(1_u32 << exponent)
        .min(AUDIO_THREAD_RESTART_MAX_DELAY)
}

fn clear_playback_after_worker_loss(shared_state: &SharedState) {
    {
        let mut inner = shared_state.write_inner();
        inner.current_song = None;
        inner.consumed_position = 0.0;
        inner.duration = 0.0;
        inner.active_request = None;
        inner.source_generation = 0;
        inner.gapless_segments.clear();
    }
    shared_state
        .crossfade_initiated
        .store(false, Ordering::SeqCst);
    shared_state.mark_stopped();
    shared_state.mark_output_closed();
    shared_state.notify_playback_changed();
}

fn reject_supervisor_command(command: AudioCommand, message: &str) {
    let error = || AudioError::OutputUnavailable(message.to_string());
    match command {
        AudioCommand::Play { ack, .. }
        | AudioCommand::Resume { ack, .. }
        | AudioCommand::RebuildOutput { ack, .. }
        | AudioCommand::AppendGapless { ack, .. }
        | AudioCommand::CrossfadePlay { ack, .. }
        | AudioCommand::Pause { ack }
        | AudioCommand::Stop { ack }
        | AudioCommand::Seek { ack, .. } => {
            let _ = ack.send(Err(error()));
        }
        AudioCommand::SetVolume(_) | AudioCommand::Shutdown => {}
    }
}

fn wait_for_audio_recovery(
    command_rx: &Receiver<AudioCommand>,
    shared_state: &SharedState,
    delay: Option<Duration>,
    allow_explicit_retry: bool,
) -> SupervisorWait {
    let deadline = delay.map(|delay| Instant::now() + delay);
    loop {
        let command = match deadline {
            Some(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return SupervisorWait::Retry(None);
                }
                match command_rx.recv_timeout(remaining) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) => return SupervisorWait::Retry(None),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return SupervisorWait::Disconnected;
                    }
                }
            }
            None => match command_rx.recv() {
                Ok(command) => command,
                Err(_) => return SupervisorWait::Disconnected,
            },
        };

        match command {
            AudioCommand::Shutdown => return SupervisorWait::Shutdown,
            command @ (AudioCommand::Play { .. }
            | AudioCommand::Resume { .. }
            | AudioCommand::RebuildOutput { .. }) => {
                if allow_explicit_retry {
                    return SupervisorWait::Retry(Some(Box::new(command)));
                }
                reject_supervisor_command(command, "audio worker restart backoff is active");
            }
            AudioCommand::Stop { ack } => {
                clear_playback_after_worker_loss(shared_state);
                let _ = ack.send(Ok(()));
            }
            AudioCommand::SetVolume(_) => {}
            command => reject_supervisor_command(
                command,
                "audio worker is recovering from repeated failures",
            ),
        }
    }
}

fn automatic_recovery_command(shared_state: &SharedState) -> Option<AudioCommand> {
    if !shared_state.has_active_request() {
        shared_state.mark_output_closed();
        return None;
    }
    let (ack, _receiver) = mpsc::channel();
    Some(AudioCommand::RebuildOutput {
        permit: Arc::new(CommandPermit::default()),
        ack,
    })
}

fn supervise_audio_thread(
    command_rx: &Receiver<AudioCommand>,
    shared_state: &Arc<SharedState>,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: &Arc<AtomicBool>,
) {
    let mut failures = 0_u32;
    let mut last_failure: Option<Instant> = None;
    let mut initial_command = None;

    loop {
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            run_audio_thread(
                command_rx,
                shared_state,
                spectrum_producer,
                spectrum_enabled,
                initial_command.take(),
            )
        }));
        match result {
            Ok(AudioThreadExit::Shutdown) => break,
            Ok(AudioThreadExit::Disconnected) => {
                warn!("Audio command channel disconnected; playback thread exiting");
                break;
            }
            Err(payload) => {
                let now = Instant::now();
                if last_failure.is_none_or(|last| {
                    now.duration_since(last) >= AUDIO_THREAD_FAILURE_RESET_WINDOW
                }) {
                    failures = 0;
                }
                failures = failures.saturating_add(1);
                last_failure = Some(now);
                let message = panic_payload_message(payload.as_ref()).to_string();
                let max_attempts = AUDIO_THREAD_MAX_AUTOMATIC_RESTARTS;
                error!(
                    "Rust audio playback thread panicked (attempt {failures}/{max_attempts}): \
                     {message}"
                );
                shared_state.mark_stream_failed();

                let terminal = failures >= AUDIO_THREAD_MAX_AUTOMATIC_RESTARTS;
                if terminal {
                    shared_state.mark_output_unavailable(message);
                }
                let delay = (!terminal).then(|| audio_restart_delay(failures));
                match wait_for_audio_recovery(command_rx, shared_state, delay, terminal) {
                    SupervisorWait::Retry(command) => {
                        if terminal && command.is_some() {
                            failures = 0;
                            last_failure = None;
                        }
                        initial_command = command
                            .map(|command| *command)
                            .or_else(|| automatic_recovery_command(shared_state));
                    }
                    SupervisorWait::Shutdown => break,
                    SupervisorWait::Disconnected => {
                        warn!("Audio command channel disconnected during recovery");
                        break;
                    }
                }
            }
        }
    }
}

/// Runs one audio-thread lifecycle while the supervisor retains the command receiver.
fn run_audio_thread(
    command_rx: &Receiver<AudioCommand>,
    shared_state: &Arc<SharedState>,
    spectrum_producer: &Arc<Mutex<HeapProd<f32>>>,
    spectrum_enabled: &Arc<AtomicBool>,
    initial_command: Option<AudioCommand>,
) -> AudioThreadExit {
    info!("Rust audio playback thread starting");
    let mut audio_thread = AudioThread::new(shared_state, spectrum_producer, spectrum_enabled);

    if let Some(command) = initial_command
        && !audio_thread.handle_command(command)
    {
        return AudioThreadExit::Shutdown;
    }

    loop {
        match audio_thread.next_event(command_rx) {
            AudioThreadEvent::Command(command) => {
                if !audio_thread.handle_command(*command) {
                    return AudioThreadExit::Shutdown;
                }
            }
            AudioThreadEvent::Timeout => audio_thread.handle_timeout(),
            AudioThreadEvent::Disconnected => return AudioThreadExit::Disconnected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_shared_state() -> SharedState {
        let (notifications, _receiver) = mpsc::channel();
        SharedState::new(notifications)
    }

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
            shared_state: Arc::new(test_shared_state()),
            spectrum_consumer: Arc::new(Mutex::new(consumer)),
            spectrum_enabled: Arc::new(AtomicBool::new(false)),
            audio_thread: Some(thread::spawn(|| {})),
        }
    }

    #[test]
    fn command_permit_cancellation_prevents_late_commit() {
        let permit = CommandPermit::default();

        assert!(permit.cancel());
        assert!(!permit.try_commit());
    }

    #[test]
    fn command_permit_commit_wins_timeout_race() {
        let permit = CommandPermit::default();

        assert!(permit.try_commit());
        assert!(!permit.cancel());
        assert!(permit.begin_apply());
        permit.finish_apply();
        assert!(permit.is_applied());
    }

    #[test]
    fn command_permit_can_abort_committed_work_before_apply() {
        let permit = CommandPermit::default();

        assert!(permit.try_commit());
        assert!(permit.abort_committed());
        assert!(!permit.begin_apply());
    }

    #[test]
    fn audio_thread_starts_without_opening_output() {
        let shared = Arc::new(test_shared_state());
        let ring_buffer = HeapRb::<f32>::new(SPECTRUM_BUFFER_SIZE);
        let (producer, _consumer) = ring_buffer.split();
        let producer = Arc::new(Mutex::new(producer));
        let spectrum_enabled = Arc::new(AtomicBool::new(false));

        let audio_thread = AudioThread::new(&shared, &producer, &spectrum_enabled);

        assert!(audio_thread.stream.is_none());
        assert_eq!(shared.state(), PlaybackLifecycleState::Stopped);
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
            PlaybackIdentity {
                generation: 1,
                song_id: "a".to_string(),
            },
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
    fn stale_gapless_playback_identity_is_rejected() {
        let shared = test_shared_state();
        let first_generation = shared.next_generation();
        shared.set_active_request(first_generation, test_request("a", 30.0));
        let expected = shared
            .playback_identity()
            .expect("active playback should have an identity");

        assert!(matches_expected_playback(
            &shared,
            first_generation,
            &expected
        ));

        let next_generation = shared.next_generation();
        shared.set_active_request(next_generation, test_request("c", 30.0));
        assert!(!matches_expected_playback(
            &shared,
            next_generation,
            &expected
        ));

        shared.set_active_request(next_generation, test_request("a", 30.0));
        assert!(!matches_expected_playback(
            &shared,
            next_generation,
            &expected
        ));
    }

    #[test]
    fn audio_restart_backoff_grows_and_caps() {
        assert_eq!(audio_restart_delay(1), Duration::from_millis(50));
        assert_eq!(audio_restart_delay(2), Duration::from_millis(100));
        assert_eq!(audio_restart_delay(10), AUDIO_THREAD_RESTART_MAX_DELAY);
    }

    #[test]
    fn unavailable_supervisor_keeps_receiver_alive_for_stop() {
        let shared = test_shared_state();
        let generation = shared.next_generation();
        shared.set_active_request(generation, test_request("a", 30.0));
        shared.mark_output_unavailable("repeated panic".to_string());
        let (command_tx, command_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        command_tx
            .send(AudioCommand::Stop { ack: stop_tx })
            .expect("stop queues");
        command_tx
            .send(AudioCommand::Shutdown)
            .expect("shutdown queues");

        let result = wait_for_audio_recovery(&command_rx, &shared, None, true);

        assert!(matches!(result, SupervisorWait::Shutdown));
        assert!(stop_rx.recv().expect("stop acknowledged").is_ok());
        assert_eq!(shared.state(), PlaybackLifecycleState::Stopped);
        assert_eq!(shared.output_state(), AudioOutputState::Closed);
    }

    #[test]
    fn consumed_position_does_not_advance_from_wall_clock() {
        let shared = test_shared_state();
        let generation = shared.next_generation();
        shared.set_active_request(generation, test_request("a", 30.0));
        shared.mark_playing();

        std::thread::sleep(Duration::from_millis(20));

        assert!(shared.get_position().abs() < f64::EPSILON);
        assert_eq!(shared.state(), PlaybackLifecycleState::Playing);
    }

    #[test]
    fn stale_generation_position_updates_are_ignored() {
        let shared = test_shared_state();
        let generation = shared.next_generation();
        shared.set_active_request(generation, test_request("a", 30.0));

        assert!(!shared.update_consumed_position(generation + 1, 12.0));
        assert!(shared.get_position().abs() < f64::EPSILON);

        assert!(shared.update_consumed_position(generation, 12.0));
        assert!((shared.get_position() - 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn watchdog_enters_stalled_when_position_freezes() {
        let shared = test_shared_state();
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
        let shared = test_shared_state();
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
    fn paused_crossfade_does_not_require_audio_polling() {
        let mut crossfade = CrossfadeState::new(5_000);
        crossfade.pause();

        assert!(!crossfade.requires_poll());
        assert!(!should_poll_audio_thread(
            false,
            crossfade.requires_poll(),
            PlaybackLifecycleState::Paused
        ));
    }

    #[test]
    fn stale_output_failure_does_not_poison_replacement_output() {
        let (notifications, receiver) = mpsc::channel();
        let shared = SharedState::new(notifications);
        shared.mark_output_ready_for(2);
        let _ = receiver.recv().expect("ready transition is emitted");

        shared.mark_stream_failed_for(1, "stale output failed".to_string());

        assert_eq!(shared.output_state(), AudioOutputState::Ready);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn missing_sink_guard_marks_playback_stalled() {
        let shared = test_shared_state();
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
        let shared = test_shared_state();
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
