//! Runtime-owned playback orchestration and the testable audio boundary.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::Value;
use stereodrome_audio::{
    AudioError, AudioNotification, AudioOutputState, AudioPlayer, AudioStateHandle, BinauralPreset,
    CrossfadePlayRequest, DynamicsPreset, EqualizerSettings, PlaybackIdentity,
    PlaybackLifecycleState, PlaybackState as AudioPlaybackState, PlaybackStatus, SongMetadata,
};
use url::Url;

use crate::protocol::{
    OperationId, PlaybackNavigation, PlaybackOutputState, PlaybackPhase, PlaybackProjection,
    PlaybackProjectionSong,
};
use crate::queue::{QueueItem, QueueState, RepeatMode};
use crate::{AudioProcessingSettings, CoreError, CoreResult, PlaybackProgress, StereodromeCore};

/// Clock boundary for playback progress scheduling tests.
pub trait PlaybackClock: Send + Sync {
    fn now(&self) -> Instant;
}

pub(crate) struct SystemPlaybackClock;

impl PlaybackClock for SystemPlaybackClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Fully prepared audio passed to an [`AudioPort`].
#[derive(Debug)]
pub struct PreparedAudio {
    pub audio_data: Arc<[u8]>,
    pub metadata: SongMetadata,
    pub duration_seconds: f64,
    pub normalization_gain: Option<f32>,
    pub dynamics_preset: Option<DynamicsPreset>,
    pub binaural_preset: Option<BinauralPreset>,
    pub equalizer_settings: Option<EqualizerSettings>,
}

/// Audio engine boundary used by the runtime and deterministic playback tests.
#[allow(clippy::missing_errors_doc)]
pub trait AudioPort: Send + Sync {
    fn take_notifications(&self) -> Option<Receiver<AudioNotification>>;
    fn playback_state(&self) -> AudioPlaybackState;
    fn status(&self) -> PlaybackStatus;
    fn current_identity(&self) -> Option<PlaybackIdentity>;
    fn play(&self, prepared: PreparedAudio, expected: Option<PlaybackIdentity>) -> CoreResult<()>;
    fn append_gapless(&self, prepared: PreparedAudio, expected: PlaybackIdentity)
    -> CoreResult<()>;
    fn crossfade(
        &self,
        prepared: PreparedAudio,
        expected: Option<PlaybackIdentity>,
        duration_ms: u32,
    ) -> CoreResult<()>;
    fn pause(&self) -> CoreResult<()>;
    fn resume(&self) -> CoreResult<()>;
    fn rebuild_output(&self) -> CoreResult<()>;
    fn stop(&self) -> CoreResult<()>;
    fn seek(&self, seconds: f64) -> CoreResult<()>;
    fn set_volume(&self, volume: f32) -> CoreResult<()>;
    fn gapless_state(&self) -> (AudioPlaybackState, usize);
    fn is_last_gapless_segment(&self, segment_index: usize) -> bool;
    fn is_crossfade_initiated(&self) -> bool;
    fn set_crossfade_initiated(&self, value: bool);
}

/// Production [`AudioPort`] backed by `stereodrome-audio`.
pub struct StereodromeAudioPort {
    player: AudioPlayer,
    state: AudioStateHandle,
    notifications: Mutex<Option<Receiver<AudioNotification>>>,
}

impl StereodromeAudioPort {
    /// Creates the audio engine without opening an output device until playback starts.
    ///
    /// # Errors
    /// Returns an audio error when the playback supervisor cannot be initialized.
    pub fn new() -> CoreResult<Self> {
        let (player, notifications) =
            AudioPlayer::new_with_spectrum_and_notifications(false).map_err(audio_error)?;
        let state = player.state_handle();
        Ok(Self {
            player,
            state,
            notifications: Mutex::new(Some(notifications)),
        })
    }
}

impl AudioPort for StereodromeAudioPort {
    fn take_notifications(&self) -> Option<Receiver<AudioNotification>> {
        self.notifications.lock().ok()?.take()
    }

    fn playback_state(&self) -> AudioPlaybackState {
        self.player.get_playback_state()
    }

    fn status(&self) -> PlaybackStatus {
        self.player.get_status()
    }

    fn current_identity(&self) -> Option<PlaybackIdentity> {
        self.player.current_playback_identity()
    }

    fn play(&self, prepared: PreparedAudio, expected: Option<PlaybackIdentity>) -> CoreResult<()> {
        self.player
            .play_with_expected(
                expected,
                prepared.audio_data,
                prepared.metadata,
                prepared.duration_seconds,
                prepared.normalization_gain,
                prepared.dynamics_preset,
                prepared.binaural_preset,
                prepared.equalizer_settings,
            )
            .map_err(audio_error)
    }

    fn append_gapless(
        &self,
        prepared: PreparedAudio,
        expected: PlaybackIdentity,
    ) -> CoreResult<()> {
        self.player
            .append_gapless(
                expected,
                prepared.audio_data,
                prepared.metadata,
                prepared.duration_seconds,
                prepared.normalization_gain,
                prepared.dynamics_preset,
                prepared.binaural_preset,
                prepared.equalizer_settings,
            )
            .map_err(audio_error)
    }

    fn crossfade(
        &self,
        prepared: PreparedAudio,
        expected: Option<PlaybackIdentity>,
        duration_ms: u32,
    ) -> CoreResult<()> {
        self.player
            .crossfade_play(CrossfadePlayRequest {
                expected_playback: expected,
                audio_data: prepared.audio_data,
                metadata: prepared.metadata,
                duration_secs: prepared.duration_seconds,
                normalization_gain: prepared.normalization_gain,
                dynamics_preset: prepared.dynamics_preset,
                binaural_preset: prepared.binaural_preset,
                equalizer_settings: prepared.equalizer_settings,
                crossfade_duration_ms: duration_ms,
            })
            .map_err(audio_error)
    }

    fn pause(&self) -> CoreResult<()> {
        self.player.pause().map_err(audio_error)
    }

    fn resume(&self) -> CoreResult<()> {
        self.player.resume().map_err(audio_error)
    }

    fn rebuild_output(&self) -> CoreResult<()> {
        self.player.rebuild_output().map_err(audio_error)
    }

    fn stop(&self) -> CoreResult<()> {
        self.player.stop().map_err(audio_error)
    }

    fn seek(&self, seconds: f64) -> CoreResult<()> {
        self.player.seek(seconds).map_err(audio_error)
    }

    fn set_volume(&self, volume: f32) -> CoreResult<()> {
        self.player.set_volume(volume).map_err(audio_error)
    }

    fn gapless_state(&self) -> (AudioPlaybackState, usize) {
        self.state.get_gapless_state()
    }

    fn is_last_gapless_segment(&self, segment_index: usize) -> bool {
        self.state.is_last_gapless_segment(segment_index)
    }

    fn is_crossfade_initiated(&self) -> bool {
        self.state.is_crossfade_initiated()
    }

    fn set_crossfade_initiated(&self, value: bool) {
        self.state.set_crossfade_initiated(value);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn audio_error(error: AudioError) -> CoreError {
    CoreError::Audio(error.to_string())
}

#[derive(Clone)]
pub(crate) enum PlaybackCommit {
    Current {
        seek_seconds: Option<f64>,
        pause_after_start: bool,
    },
    Navigation {
        navigation: PlaybackNavigation,
        expected_current_song_id: Option<String>,
        expected_playback: Option<PlaybackIdentity>,
    },
    Gapless {
        expected_playback: PlaybackIdentity,
    },
    Crossfade {
        expected_playback: PlaybackIdentity,
        current_song_id: String,
        duration_ms: u32,
    },
}

pub(crate) struct PreparedPlayback {
    pub target_song_id: String,
    pub prepared: PreparedAudio,
    pub commit: PlaybackCommit,
}

pub(crate) async fn prepare(
    core: &StereodromeCore,
    item: QueueItem,
    commit: PlaybackCommit,
) -> CoreResult<PreparedPlayback> {
    let target_song_id = item.song_id.clone();
    let status = core.download_song(item.song_id.clone()).await?;
    let path = status.path.as_deref().ok_or_else(|| {
        CoreError::Audio(format!(
            "song {} did not produce a cached audio path",
            item.song_id
        ))
    })?;
    let audio_path = file_uri_to_path(path)?;
    let audio_data = std::fs::read(audio_path)?;
    let settings = core.get_audio_processing_settings()?;
    let processing = audio_processing(&settings)?;

    Ok(PreparedPlayback {
        target_song_id,
        prepared: PreparedAudio {
            audio_data: Arc::from(audio_data),
            metadata: SongMetadata {
                id: item.song_id,
                title: item.title,
                artist: item.artist,
                album: item.album,
                cover_art_id: None,
            },
            duration_seconds: duration_seconds(item.duration),
            normalization_gain: processing.normalization_gain,
            dynamics_preset: processing.dynamics,
            binaural_preset: processing.binaural,
            equalizer_settings: processing.equalizer,
        },
        commit,
    })
}

pub(crate) fn commit(
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    prepared: PreparedPlayback,
) -> CoreResult<Value> {
    let target_song_id = prepared.target_song_id.clone();
    let result = match prepared.commit {
        PlaybackCommit::Current {
            seek_seconds,
            pause_after_start,
        } => {
            audio.play(prepared.prepared, None)?;
            if let Some(position) = seek_seconds {
                audio.seek(position)?;
            }
            if pause_after_start {
                audio.pause()?;
            }
            serde_json::to_value(core.get_queue()?).map_err(CoreError::from)
        }
        PlaybackCommit::Navigation {
            navigation,
            expected_current_song_id,
            expected_playback,
        } => {
            if expected_playback
                .as_ref()
                .is_some_and(|expected| audio.current_identity().as_ref() != Some(expected))
            {
                return Err(CoreError::Audio(
                    "playback changed while queue navigation was being prepared".to_string(),
                ));
            }
            audio.play(prepared.prepared, expected_playback)?;
            let committed = commit_navigation(
                core,
                navigation,
                expected_current_song_id.as_deref(),
                &target_song_id,
            );
            match committed {
                Ok(Some(item)) if item.song_id == target_song_id => {
                    serde_json::to_value(core.get_queue()?).map_err(CoreError::from)
                }
                Ok(_) => {
                    let _ = audio.stop();
                    Err(CoreError::Audio(
                        "queue navigation changed while playback was being prepared".to_string(),
                    ))
                }
                Err(error) => {
                    let _ = audio.stop();
                    Err(error)
                }
            }
        }
        PlaybackCommit::Gapless { expected_playback } => {
            audio.append_gapless(prepared.prepared, expected_playback)?;
            Ok(Value::Null)
        }
        PlaybackCommit::Crossfade {
            expected_playback,
            current_song_id,
            duration_ms,
        } => {
            audio.crossfade(prepared.prepared, Some(expected_playback), duration_ms)?;
            if let Err(error) =
                core.play_next_if_matches(Some(false), Some(&current_song_id), &target_song_id)
            {
                let _ = audio.stop();
                return Err(error);
            }
            serde_json::to_value(core.get_queue()?).map_err(CoreError::from)
        }
    };
    if result.is_err() {
        let _ = core.invalidate_cached_song(&target_song_id);
    }
    result
}

pub(crate) fn preview_navigation(
    core: &StereodromeCore,
    navigation: PlaybackNavigation,
) -> CoreResult<Option<QueueItem>> {
    match navigation {
        PlaybackNavigation::Index { index } => core
            .get_queue()?
            .items
            .get(index)
            .cloned()
            .map(Some)
            .ok_or_else(|| CoreError::InvalidInput(format!("queue index {index} is out of range"))),
        PlaybackNavigation::Next { force } => core.preview_next_queue_item(Some(force)),
        PlaybackNavigation::Previous => core.preview_previous_queue_item(),
    }
}

fn commit_navigation(
    core: &StereodromeCore,
    navigation: PlaybackNavigation,
    expected_current_song_id: Option<&str>,
    expected_target_song_id: &str,
) -> CoreResult<Option<QueueItem>> {
    match navigation {
        PlaybackNavigation::Index { index } => core.play_queue_item_if_matches(
            index,
            expected_current_song_id,
            expected_target_song_id,
        ),
        PlaybackNavigation::Next { force } => core.play_next_if_matches(
            Some(force),
            expected_current_song_id,
            expected_target_song_id,
        ),
        PlaybackNavigation::Previous => {
            core.play_previous_if_matches(expected_current_song_id, expected_target_song_id)
        }
    }
}

pub(crate) fn projection(
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    preparing_operation_id: Option<OperationId>,
) -> CoreResult<PlaybackProjection> {
    let queue = core.get_queue()?;
    let audio_state = audio.playback_state();
    let persisted = core.get_playback_state()?;
    let audio_loaded = audio_state.song.is_some();

    let (song, position_seconds, duration_seconds) = if let Some(song) = audio_state.song {
        let duration = if audio_state.duration > 0.0 {
            audio_state.duration
        } else {
            queue
                .items
                .iter()
                .find(|item| item.song_id == song.id)
                .map_or(0.0, |item| duration_seconds(item.duration))
        };
        (
            Some(project_audio_song(core, song, duration)),
            audio_state.position,
            duration,
        )
    } else {
        let persisted_song_id = persisted.current_song_id.as_deref();
        let item = persisted_song_id
            .and_then(|song_id| queue.items.iter().find(|item| item.song_id == song_id))
            .or_else(|| queue.current_index.and_then(|index| queue.items.get(index)));
        item.map_or((None, 0.0, 0.0), |item| {
            let same_song = persisted_song_id == Some(item.song_id.as_str());
            let duration = if same_song && persisted.duration_seconds > 0.0 {
                persisted.duration_seconds
            } else {
                duration_seconds(item.duration)
            };
            (
                Some(project_queue_song(core, item, duration)),
                if same_song {
                    persisted.position_seconds.max(0.0)
                } else {
                    0.0
                },
                duration,
            )
        })
    };

    let state = match audio_state.state {
        PlaybackLifecycleState::Playing => PlaybackPhase::Playing,
        PlaybackLifecycleState::Paused => PlaybackPhase::Paused,
        PlaybackLifecycleState::Stopped => PlaybackPhase::Stopped,
        PlaybackLifecycleState::Stalled => PlaybackPhase::Stalled,
    };
    let queue_index = queue.current_index;
    let queue_length = queue.items.len();
    let can_play = song.is_some();
    Ok(PlaybackProjection {
        state,
        is_playing: audio_state.is_playing,
        audio_loaded,
        output_state: project_output_state(audio_state.output_state),
        song,
        position_seconds,
        duration_seconds,
        volume: audio_state.volume,
        queue: queue.clone(),
        queue_index,
        queue_length,
        can_play,
        can_next: next_queue_item_exists(&queue),
        can_previous: queue_length > 1 && queue_index.is_some(),
        can_seek: duration_seconds > 0.0,
        preparing_operation_id,
    })
}

pub(crate) fn persist_live_progress(
    core: &StereodromeCore,
    audio: &dyn AudioPort,
) -> CoreResult<()> {
    let state = audio.playback_state();
    if let Some(song) = state.song {
        core.save_playback_position(PlaybackProgress {
            song_id: song.id,
            position_seconds: state.position,
            duration_seconds: state.duration,
            is_playing: state.is_playing,
        })?;
    }
    Ok(())
}

pub(crate) fn gapless_target(
    core: &StereodromeCore,
    audio: &dyn AudioPort,
) -> CoreResult<Option<(QueueItem, PlaybackIdentity)>> {
    let settings = core.get_audio_processing_settings()?;
    if !settings.gapless_enabled || audio.status().current_song_id.is_none() {
        return Ok(None);
    }
    let (_, segment_index) = audio.gapless_state();
    if !audio.is_last_gapless_segment(segment_index) {
        return Ok(None);
    }
    let queue = core.get_queue()?;
    if queue.repeat_mode == RepeatMode::One {
        return Ok(None);
    }
    let Some(current) = queue.current_index.and_then(|index| queue.items.get(index)) else {
        return Ok(None);
    };
    let Some(next) = core.peek_next_queue_item()? else {
        return Ok(None);
    };
    if current.song_id == next.song_id
        || !core.songs_are_gapless_eligible(&current.song_id, &next.song_id)?
    {
        return Ok(None);
    }
    let Some(identity) = audio.current_identity() else {
        return Ok(None);
    };
    if identity.song_id() != current.song_id {
        return Ok(None);
    }
    Ok(Some((next, identity)))
}

fn project_audio_song(
    core: &StereodromeCore,
    song: SongMetadata,
    duration_seconds: f64,
) -> PlaybackProjectionSong {
    PlaybackProjectionSong {
        artwork_uri: cached_artwork_uri(core, &song.id),
        id: song.id,
        title: song.title,
        artist: song.artist,
        album: song.album,
        duration_seconds,
    }
}

fn project_queue_song(
    core: &StereodromeCore,
    item: &QueueItem,
    duration_seconds: f64,
) -> PlaybackProjectionSong {
    PlaybackProjectionSong {
        artwork_uri: cached_artwork_uri(core, &item.song_id),
        id: item.song_id.clone(),
        title: item.title.clone(),
        artist: item.artist.clone(),
        album: item.album.clone(),
        duration_seconds,
    }
}

fn cached_artwork_uri(core: &StereodromeCore, song_id: &str) -> Option<String> {
    core.cached_song_cover_art_uri(song_id, Some(512))
        .ok()
        .flatten()
}

fn next_queue_item_exists(queue: &QueueState) -> bool {
    if queue.items.is_empty() || queue.prepared_next_item.is_some() {
        return queue.prepared_next_item.is_some();
    }
    if queue.repeat_mode == RepeatMode::One && queue.current_index.is_some() {
        return true;
    }
    match queue.current_index {
        None => true,
        Some(index) if index + 1 < queue.items.len() => true,
        Some(_) => queue.repeat_mode == RepeatMode::All,
    }
}

fn project_output_state(state: AudioOutputState) -> PlaybackOutputState {
    match state {
        AudioOutputState::Closed => PlaybackOutputState::Closed,
        AudioOutputState::Ready => PlaybackOutputState::Ready,
        AudioOutputState::Failed => PlaybackOutputState::Failed,
        AudioOutputState::Unavailable => PlaybackOutputState::Unavailable,
    }
}

struct AudioProcessing {
    normalization_gain: Option<f32>,
    dynamics: Option<DynamicsPreset>,
    binaural: Option<BinauralPreset>,
    equalizer: Option<EqualizerSettings>,
}

fn audio_processing(settings: &AudioProcessingSettings) -> CoreResult<AudioProcessing> {
    let normalization_gain = if settings.normalization_enabled || settings.preamp_db.abs() > 0.01 {
        Some(10.0_f32.powf(narrow_f64_to_f32(settings.preamp_db, "preamp_db")? / 20.0))
    } else {
        None
    };
    let dynamics = if settings.dynamics_enabled {
        Some(match settings.dynamics_preset.as_str() {
            "light" => DynamicsPreset::Light,
            "medium" => DynamicsPreset::Medium,
            "heavy" => DynamicsPreset::Heavy,
            value => {
                return Err(CoreError::InvalidInput(format!(
                    "unknown dynamics preset: {value}"
                )));
            }
        })
    } else {
        None
    };
    let binaural = if settings.binaural_enabled {
        Some(match settings.binaural_preset.as_str() {
            "light" => BinauralPreset::Default,
            "medium" => BinauralPreset::Jmeier,
            "strong" => BinauralPreset::Aggressive,
            value => {
                return Err(CoreError::InvalidInput(format!(
                    "unknown binaural preset: {value}"
                )));
            }
        })
    } else {
        None
    };
    let equalizer = if settings.equalizer_enabled {
        Some(EqualizerSettings::new(
            settings
                .equalizer_bands_db
                .iter()
                .map(|value| narrow_f64_to_f32(*value, "equalizer band"))
                .collect::<CoreResult<Vec<_>>>()?,
        ))
    } else {
        None
    };
    Ok(AudioProcessing {
        normalization_gain,
        dynamics,
        binaural,
        equalizer,
    })
}

fn narrow_f64_to_f32(value: f64, name: &str) -> CoreResult<f32> {
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(CoreError::InvalidInput(format!(
            "{name} is outside the supported f32 range"
        )));
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(value as f32)
}

fn duration_seconds(duration: i64) -> f64 {
    let duration = duration.clamp(0, i64::from(u32::MAX));
    f64::from(u32::try_from(duration).expect("clamped duration fits in u32"))
}

fn file_uri_to_path(value: &str) -> CoreResult<PathBuf> {
    if value.starts_with("file://") {
        Url::parse(value)
            .map_err(|error| CoreError::InvalidInput(error.to_string()))?
            .to_file_path()
            .map_err(|()| CoreError::InvalidInput(format!("invalid file URI: {value}")))
    } else {
        Ok(PathBuf::from(value))
    }
}
