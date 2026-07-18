use log::{info, warn};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::audio::binaural::BinauralPreset;
use crate::audio::compressor::DynamicsPreset;
use crate::audio::equalizer::EqualizerSettings;
use crate::audio::loudness;
use crate::audio::player::CrossfadePlayRequest;
use crate::audio::queue::RepeatMode;
use crate::audio::{PlaybackStatus, SongMetadata};
use crate::cache::AudioCache;
use crate::error::{AppError, AppResult, MutexExt};
use crate::operations::queue::persist_and_emit;
use crate::operations::settings::{
    NormalizationMode, NormalizationSettings, manual_offline_enabled, read_normalization_settings,
    read_playback_settings,
};
use crate::state::DesktopState;

/// Common song data needed for playback.
struct SongData {
    audio_data: Arc<[u8]>,
    metadata: SongMetadata,
    duration: f64,
    album_id: String,
    suffix: String,
    normalization_gain: Option<f32>,
    dynamics_preset: Option<DynamicsPreset>,
    binaural_preset: Option<BinauralPreset>,
    equalizer_settings: Option<EqualizerSettings>,
}

#[derive(Default)]
struct SubsonicScrobbleTracker {
    observed_song_id: Option<String>,
    submitted_song_id: Option<String>,
    last_position: f64,
}

impl SubsonicScrobbleTracker {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn update(&mut self, song_id: &str, position: f64, duration: f64) -> bool {
        let restarted = self.submitted_song_id.as_deref() == Some(song_id)
            && position < self.last_position
            && position < 5.0;
        if self.observed_song_id.as_deref() != Some(song_id) || restarted {
            self.submitted_song_id = None;
        }
        self.observed_song_id = Some(song_id.to_string());
        self.last_position = position;

        if duration <= 0.0
            || position < duration * 0.5
            || self.submitted_song_id.as_deref() == Some(song_id)
        {
            return false;
        }

        self.submitted_song_id = Some(song_id.to_string());
        true
    }
}

/// Fetch all data needed to play a song: metadata, audio bytes, normalization gain.
async fn fetch_song_data(state: Arc<DesktopState>, song_id: &str) -> AppResult<SongData> {
    let (duration, title, artist, album, album_id, cover_art_id, suffix): (
        f64,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = {
        let conn = state.db.lock_recover();
        conn.query_row(
            "SELECT s.duration, s.title, a.name, al.name, s.album_id, al.cover_art_id, s.suffix
             FROM songs s
             LEFT JOIN artists a ON s.artist_id = a.id
             LEFT JOIN albums al ON s.album_id = al.id
             WHERE s.id = ?",
            [song_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0) as f64,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                ))
            },
        )
        .map_err(AppError::Database)?
    };

    let metadata = SongMetadata {
        id: song_id.to_string(),
        title,
        artist,
        album,
        cover_art_id,
    };

    let cache = AudioCache::new(Arc::clone(&state))?;
    let audio_data: Arc<[u8]> = cache
        .get_or_fetch(&state.client, song_id, &suffix)
        .await?
        .into();

    let norm_settings = read_normalization_settings(&state.settings);
    let normalization_gain = if norm_settings.enabled {
        let conn = state.db.lock_recover();
        get_normalization_gain(&conn, song_id, &album_id, &norm_settings)
    } else {
        None
    };

    let dynamics_preset = if norm_settings.dynamics_enabled {
        Some(norm_settings.dynamics_preset.clone())
    } else {
        None
    };
    let playback_settings = read_playback_settings(&state.settings);
    let binaural_preset = if playback_settings.binaural_enabled {
        Some(playback_settings.binaural_preset.clone())
    } else {
        None
    };
    let equalizer_settings = if playback_settings.equalizer_enabled {
        Some(EqualizerSettings::new(
            playback_settings.equalizer_bands_db.clone(),
        ))
    } else {
        None
    };

    Ok(SongData {
        audio_data,
        metadata,
        duration,
        album_id,
        suffix,
        normalization_gain,
        dynamics_preset,
        binaural_preset,
        equalizer_settings,
    })
}

/// Prefetch the next song in the queue for gapless playback.
/// Also triggers normalization analysis if enabled and no data exists.
fn prefetch_next_song(runtime: &tokio::runtime::Handle, state: Arc<DesktopState>) {
    if manual_offline_enabled(&state.settings) {
        return;
    }

    // Check if connected
    if !state.client.is_connected() {
        return;
    }

    // Get next song info from queue
    let next_song_id: Option<String> = {
        let mut queue = state.queue.lock_recover();
        queue.peek_next().map(|item| item.song_id.clone())
    };

    let next_song_info: Option<(String, String, Option<String>)> =
        next_song_id.and_then(|song_id| {
            let conn = state.db.lock_recover();

            // Get suffix and album_id from database
            let (suffix, album_id): (String, String) = conn
                .query_row(
                    "SELECT suffix, album_id FROM songs WHERE id = ?",
                    [&song_id],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                            row.get::<_, String>(1)?,
                        ))
                    },
                )
                .ok()?;

            // Check if normalization analysis is needed
            let norm_settings = read_normalization_settings(&state.settings);
            let needs_analysis = if norm_settings.enabled {
                let has_data: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM normalization_data WHERE song_id = ?",
                        [&song_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|count| count > 0)
                    .unwrap_or(false);
                !has_data
            } else {
                false
            };

            let album_id_for_analysis = if needs_analysis { Some(album_id) } else { None };

            Some((song_id, suffix, album_id_for_analysis))
        });

    if let Some((song_id, suffix, album_id)) = next_song_info {
        AudioCache::prefetch(
            runtime,
            Arc::clone(&state),
            state.client.clone(),
            song_id,
            suffix,
            album_id,
        );
    }
}

/// Rebuild the currently playing song with the latest persisted playback settings.
/// Uses a restart+seek strategy and preserves play/pause state.
pub async fn reapply_settings_to_current_song(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
) -> AppResult<bool> {
    let status = {
        let audio_player = state.audio_player.lock_recover();
        audio_player.get_status()
    };

    let Some(song_id) = status.current_song_id else {
        return Ok(false);
    };

    let was_playing = status.is_playing;
    let position = status.position;
    let data = fetch_song_data(Arc::clone(&state), &song_id).await?;

    {
        let audio_player = state.audio_player.lock_recover();
        audio_player.play(
            data.audio_data,
            data.metadata,
            data.duration,
            data.normalization_gain,
            data.dynamics_preset,
            data.binaural_preset,
            data.equalizer_settings,
        )?;

        if was_playing {
            audio_player.seek(position)?;
        } else {
            // Keep paused state while still restoring segment-relative position.
            audio_player.pause()?;
            audio_player.seek(position)?;
        }
    }

    spawn_loudness_analysis_if_needed(
        runtime,
        Arc::clone(&state),
        &song_id,
        &data.album_id,
        &data.suffix,
        data.normalization_gain,
    );

    prefetch_next_song(runtime, Arc::clone(&state));
    check_and_queue_gapless(runtime, Arc::clone(&state));

    Ok(true)
}

pub async fn play_song_by_id(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
    song_id: &str,
) -> AppResult<()> {
    let data = fetch_song_data(Arc::clone(&state), song_id).await?;
    let lastfm_metadata = data.metadata.clone();
    let lastfm_duration = data.duration;

    // Play the audio
    {
        let audio_player = state.audio_player.lock_recover();
        audio_player.play(
            data.audio_data,
            data.metadata,
            data.duration,
            data.normalization_gain,
            data.dynamics_preset,
            data.binaural_preset,
            data.equalizer_settings,
        )?;
    }

    // Spawn background loudness analysis if normalization is enabled but no data exists
    spawn_loudness_analysis_if_needed(
        runtime,
        Arc::clone(&state),
        song_id,
        &data.album_id,
        &data.suffix,
        data.normalization_gain,
    );

    // Prefetch next song for gapless playback
    prefetch_next_song(runtime, Arc::clone(&state));

    // Check if next song is gapless-eligible and queue it on the same Sink
    check_and_queue_gapless(runtime, Arc::clone(&state));

    // Report "now playing" to Subsonic server
    // Fire and forget - don't fail playback if scrobble fails
    if !manual_offline_enabled(&state.settings) {
        let _ = state.client.scrobble(song_id, None, Some(false)).await;
        let state = Arc::clone(&state);
        runtime.spawn(async move {
            if let Err(e) =
                crate::lastfm::report_now_playing(&state.settings, lastfm_metadata, lastfm_duration)
                    .await
            {
                warn!("Failed to report Last.fm now playing: {e}");
            }
        });
    }

    Ok(())
}

/// Advance playback after the audio thread reports that the current track finished.
/// Gapless segment changes and crossfade handoffs advance earlier in their own paths;
/// this handles the normal sink-ended case in the backend so queue progression does not
/// depend on a frontend window being alive.
pub async fn handle_playback_finished(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
) -> AppResult<bool> {
    if state
        .navigating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(true);
    }

    let result = async {
        let next_song = {
            let mut queue = state.queue.lock_recover();
            queue.next(false).map(|item| item.song_id.clone())
        };

        persist_and_emit(&state);

        if let Some(song_id) = next_song {
            play_song_by_id(runtime, Arc::clone(&state), &song_id).await?;
            Ok(true)
        } else {
            state.events.queue_ended();
            state.events.playback_ended();
            Ok(false)
        }
    }
    .await;

    state.navigating.store(false, Ordering::SeqCst);
    result
}

/// Play a song with crossfade transition from the currently playing track.
pub async fn crossfade_play_by_id(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
    song_id: &str,
    crossfade_duration_ms: u32,
) -> AppResult<()> {
    let data = fetch_song_data(Arc::clone(&state), song_id).await?;
    let lastfm_metadata = data.metadata.clone();
    let lastfm_duration = data.duration;

    {
        let audio_player = state.audio_player.lock_recover();
        audio_player.crossfade_play(CrossfadePlayRequest {
            audio_data: data.audio_data,
            metadata: data.metadata,
            duration_secs: data.duration,
            normalization_gain: data.normalization_gain,
            dynamics_preset: data.dynamics_preset,
            binaural_preset: data.binaural_preset,
            equalizer_settings: data.equalizer_settings,
            crossfade_duration_ms,
        })?;
    }

    spawn_loudness_analysis_if_needed(
        runtime,
        Arc::clone(&state),
        song_id,
        &data.album_id,
        &data.suffix,
        data.normalization_gain,
    );

    prefetch_next_song(runtime, Arc::clone(&state));
    check_and_queue_gapless(runtime, Arc::clone(&state));

    if !manual_offline_enabled(&state.settings) {
        let _ = state.client.scrobble(song_id, None, Some(false)).await;
        let state = Arc::clone(&state);
        runtime.spawn(async move {
            if let Err(e) =
                crate::lastfm::report_now_playing(&state.settings, lastfm_metadata, lastfm_duration)
                    .await
            {
                warn!("Failed to report Last.fm now playing: {e}");
            }
        });
    }

    Ok(())
}

/// Initiate a crossfade transition to the next song in the queue.
/// Called by the position emitter when playback enters the crossfade window.
pub async fn initiate_crossfade(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
    crossfade_duration_ms: u32,
) {
    // Get current and next song IDs
    let (current_song_id, next_song_id) = {
        let mut queue = state.queue.lock_recover();
        let current_id = queue.current_item().map(|i| i.song_id.clone());
        let next_id = queue.peek_next().map(|i| i.song_id.clone());
        match (current_id, next_id) {
            (Some(curr), Some(next)) => (curr, next),
            _ => {
                reset_crossfade_initiated(&state);
                return;
            }
        }
    };

    // Don't crossfade if next song is gapless-eligible (gapless takes priority)
    let playback_settings = read_playback_settings(&state.settings);
    if playback_settings.gapless_enabled {
        let eligible = {
            let conn = state.db.lock_recover();
            is_gapless_eligible(&conn, &current_song_id, &next_song_id)
        };
        if eligible {
            reset_crossfade_initiated(&state);
            return;
        }
    }

    // Don't crossfade in repeat-one mode
    let repeat_mode = {
        let queue = state.queue.lock_recover();
        queue.repeat_mode()
    };
    if repeat_mode == RepeatMode::One {
        reset_crossfade_initiated(&state);
        return;
    }

    info!("Initiating crossfade to song {}", next_song_id);

    let data = match fetch_song_data(Arc::clone(&state), &next_song_id).await {
        Ok(d) => d,
        Err(e) => {
            warn!("Crossfade: failed to fetch song data: {e}");
            reset_crossfade_initiated(&state);
            return;
        }
    };
    let lastfm_metadata = data.metadata.clone();
    let lastfm_duration = data.duration;

    // Send CrossfadePlay command
    {
        let audio_player = state.audio_player.lock_recover();
        if let Err(e) = audio_player.crossfade_play(CrossfadePlayRequest {
            audio_data: data.audio_data,
            metadata: data.metadata,
            duration_secs: data.duration,
            normalization_gain: data.normalization_gain,
            dynamics_preset: data.dynamics_preset,
            binaural_preset: data.binaural_preset,
            equalizer_settings: data.equalizer_settings,
            crossfade_duration_ms,
        }) {
            warn!("Crossfade: failed to start: {e}");
            reset_crossfade_initiated(&state);
            return;
        }
    }

    // Advance the queue (the new song is now playing)
    {
        let mut queue = state.queue.lock_recover();
        queue.next(false);
    }
    persist_and_emit(&state);

    // Scrobble the new song
    if !manual_offline_enabled(&state.settings) {
        let _ = state
            .client
            .scrobble(&next_song_id, None, Some(false))
            .await;
        let state = Arc::clone(&state);
        runtime.spawn(async move {
            if let Err(e) =
                crate::lastfm::report_now_playing(&state.settings, lastfm_metadata, lastfm_duration)
                    .await
            {
                warn!("Failed to report Last.fm now playing: {e}");
            }
        });
    }

    // Prefetch next-next and check gapless eligibility
    prefetch_next_song(runtime, Arc::clone(&state));
    check_and_queue_gapless(runtime, Arc::clone(&state));
}

fn reset_crossfade_initiated(state: &DesktopState) {
    let audio_player = state.audio_player.lock_recover();
    audio_player.set_crossfade_initiated(false);
}

/// Spawn background loudness analysis if normalization is enabled but no data exists.
fn spawn_loudness_analysis_if_needed(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
    song_id: &str,
    album_id: &str,
    suffix: &str,
    normalization_gain: Option<f32>,
) {
    let norm_settings = read_normalization_settings(&state.settings);
    if normalization_gain.is_none() && norm_settings.enabled {
        let song_id = song_id.to_string();
        let album_id = album_id.to_string();
        let suffix = suffix.to_string();
        let runtime = runtime.clone();
        let task_runtime = runtime.clone();
        runtime.spawn(async move {
            let cache = match AudioCache::new(Arc::clone(&state)) {
                Ok(c) => c,
                Err(_) => return,
            };
            let cache_path = cache.get_cache_path(&song_id, &suffix);
            let audio_data = match tokio::fs::read(&cache_path).await {
                Ok(data) => data,
                Err(_) => return,
            };
            let song_id_inner = song_id.clone();
            let db_path = state.paths.database.clone();
            let _ = task_runtime
                .spawn_blocking(move || match loudness::analyze_loudness(audio_data) {
                    Ok(result) => {
                        let _ = crate::db::save_normalization_result(
                            &db_path,
                            &song_id_inner,
                            &album_id,
                            result.integrated_lufs,
                            result.true_peak,
                        );
                    }
                    Err(e) => {
                        warn!("Failed to analyze loudness for song: {e}");
                    }
                })
                .await;
        });
    }
}

pub fn pause_playback(state: &DesktopState) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.pause()
}

pub fn resume_playback(state: &DesktopState) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.resume()
}

pub fn stop_playback(state: &DesktopState) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.stop()
}

pub fn set_volume(state: &DesktopState, volume: f32) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.set_volume(volume)
}

pub fn seek_playback(state: &DesktopState, position: f64) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.seek(position)
}

pub fn get_playback_status(state: &DesktopState) -> PlaybackStatus {
    let audio_player = state.audio_player.lock_recover();
    audio_player.get_status()
}

/// Check if two songs should transition gaplessly.
/// Returns true if they're on the same album and consecutive tracks.
fn is_gapless_eligible(
    conn: &rusqlite::Connection,
    current_song_id: &str,
    next_song_id: &str,
) -> bool {
    let get_track_info = |song_id: &str| -> Option<(String, i32, i32)> {
        conn.query_row(
            "SELECT album_id, disc_number, track_number FROM songs WHERE id = ?",
            [song_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i32>>(1)?.unwrap_or(1),
                    row.get::<_, Option<i32>>(2)?.unwrap_or(0),
                ))
            },
        )
        .ok()
    };

    let current = match get_track_info(current_song_id) {
        Some(info) => info,
        None => return false,
    };
    let next = match get_track_info(next_song_id) {
        Some(info) => info,
        None => return false,
    };

    // Must be same album
    if current.0 != next.0 {
        return false;
    }

    // Same disc, next track number
    let same_disc_consecutive = current.1 == next.1 && next.2 == current.2 + 1;
    // Next disc, first track
    let next_disc_first_track = next.1 == current.1 + 1 && next.2 == 1;

    same_disc_consecutive || next_disc_first_track
}

/// Check if the next song in queue is gapless-eligible and append it to the current Sink.
fn check_and_queue_gapless(runtime: &tokio::runtime::Handle, state: Arc<DesktopState>) {
    if !state.client.is_connected() && !manual_offline_enabled(&state.settings) {
        return;
    }

    // Check if gapless playback is enabled in settings
    if !read_playback_settings(&state.settings).gapless_enabled {
        return;
    }

    // Don't attempt gapless in repeat-one mode
    let repeat_mode = {
        let queue = state.queue.lock_recover();
        queue.repeat_mode()
    };
    if repeat_mode == RepeatMode::One {
        return;
    }

    // Get current and next song IDs
    let (current_song_id, next_song_id) = {
        let mut queue = state.queue.lock_recover();
        let current_id = queue.current_item().map(|i| i.song_id.clone());
        let next_id = queue.peek_next().map(|i| i.song_id.clone());
        match (current_id, next_id) {
            (Some(curr), Some(next)) if curr != next => (curr, next),
            _ => return,
        }
    };

    // Check gapless eligibility
    let eligible = {
        let conn = state.db.lock_recover();
        is_gapless_eligible(&conn, &current_song_id, &next_song_id)
    };

    if !eligible {
        return;
    }

    info!("Gapless eligible: queuing next song {}", next_song_id);

    // Spawn async task to fetch audio and send AppendGapless command
    let state = Arc::clone(&state);
    runtime.spawn(async move {
        let data = match fetch_song_data(Arc::clone(&state), &next_song_id).await {
            Ok(d) => d,
            Err(e) => {
                warn!("Gapless: failed to fetch song data: {e}");
                return;
            }
        };

        // Append to existing Sink for gapless transition
        let audio_player = state.audio_player.lock_recover();
        if let Err(e) = audio_player.append_gapless(
            data.audio_data,
            data.metadata,
            data.duration,
            data.normalization_gain,
            data.dynamics_preset,
            data.binaural_preset,
            data.equalizer_settings,
        ) {
            warn!("Gapless: failed to append: {e}");
        }
    });
}

/// Handle the async portion of a gapless transition: scrobble, prefetch, and queue next.
/// The queue has already been advanced synchronously by the position emitter to prevent
/// a race with playback_finished detection.
pub async fn after_gapless_transition(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
    next_song_id: Option<String>,
) {
    if let Some(ref song_id) = next_song_id {
        info!("Gapless transition to song {}", song_id);

        // Scrobble the new song
        if !manual_offline_enabled(&state.settings) {
            let _ = state.client.scrobble(song_id, None, Some(false)).await;
        }

        let lastfm_song = {
            let conn = state.db.lock_recover();
            conn.query_row(
                "SELECT s.title, a.name, al.name, al.cover_art_id, s.duration
                 FROM songs s
                 LEFT JOIN artists a ON s.artist_id = a.id
                 LEFT JOIN albums al ON s.album_id = al.id
                 WHERE s.id = ?",
                [song_id],
                |row| {
                    Ok((
                        SongMetadata {
                            id: song_id.clone(),
                            title: row.get::<_, String>(0)?,
                            artist: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                            album: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                            cover_art_id: row.get::<_, Option<String>>(3)?,
                        },
                        row.get::<_, Option<i64>>(4)?.unwrap_or(0) as f64,
                    ))
                },
            )
            .ok()
        };
        if let Some((song, duration)) = lastfm_song
            && !manual_offline_enabled(&state.settings)
        {
            let state = Arc::clone(&state);
            runtime.spawn(async move {
                if let Err(e) =
                    crate::lastfm::report_now_playing(&state.settings, song, duration).await
                {
                    warn!("Failed to report Last.fm now playing: {e}");
                }
            });
        }
    }

    // Prefetch the next-next song and check if it's also gapless-eligible
    prefetch_next_song(runtime, Arc::clone(&state));
    check_and_queue_gapless(runtime, Arc::clone(&state));
}

pub fn start_spectrum_publisher(
    state: Arc<DesktopState>,
    running: Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let (consumer, playback_state) = {
        let player = state.audio_player.lock_recover();
        (player.get_spectrum_consumer(), player.state_handle())
    };

    std::thread::spawn(move || {
        const DEFAULT_SAMPLE_RATE: u32 = 44_100;
        let mut analyzer = stereodrome_audio::spectrum::SpectrumAnalyzer::new(DEFAULT_SAMPLE_RATE);
        let mut published_idle_state = false;

        while running.load(Ordering::Acquire) {
            std::thread::park_timeout(std::time::Duration::from_millis(33));
            if !running.load(Ordering::Acquire) {
                break;
            }
            if !playback_state.is_playing() {
                if !published_idle_state {
                    state
                        .events
                        .publish_spectrum(stereodrome_audio::spectrum::SpectrumData::default());
                    analyzer.clear();
                    published_idle_state = true;
                }
                continue;
            }

            published_idle_state = false;
            if let Ok(mut consumer) = consumer.try_lock()
                && let Some(spectrum) = analyzer.process(&mut consumer)
            {
                state.events.publish_spectrum(spectrum);
            }
        }
    })
}

pub fn start_playback_publisher(
    runtime: tokio::runtime::Handle,
    state: Arc<DesktopState>,
    running: Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let playback_state = state.audio_player.lock_recover().state_handle();

    std::thread::spawn(move || {
        let mut progress_counter: u8 = 0;
        let mut last_segment_index = 0;
        let mut published_empty_state = true;
        let mut subsonic_scrobbles = SubsonicScrobbleTracker::default();

        while running.load(Ordering::Acquire) {
            std::thread::park_timeout(std::time::Duration::from_millis(100));
            if !running.load(Ordering::Acquire) {
                break;
            }

            let (snapshot, segment_index) = playback_state.get_gapless_state();
            if !snapshot.is_playing && snapshot.song.is_none() {
                last_segment_index = 0;
                subsonic_scrobbles.reset();
                if !published_empty_state {
                    state.events.publish_playback(snapshot);
                    published_empty_state = true;
                }
                continue;
            }
            published_empty_state = false;

            if segment_index < last_segment_index {
                last_segment_index = 0;
            }

            if segment_index > last_segment_index && snapshot.song.is_some() {
                last_segment_index = segment_index;
                let next_song_id = {
                    let mut queue = state.queue.lock_recover();
                    queue.next(false).map(|item| item.song_id.clone())
                };
                persist_and_emit(&state);
                let task_state = Arc::clone(&state);
                let task_runtime = runtime.clone();
                runtime.spawn(async move {
                    after_gapless_transition(&task_runtime, task_state, next_song_id).await;
                });
            }

            if snapshot.is_playing
                && snapshot.song.is_some()
                && playback_state.is_last_gapless_segment(segment_index)
                && !playback_state.is_crossfade_initiated()
            {
                let settings = read_playback_settings(&state.settings);
                if settings.crossfade_enabled {
                    let remaining = snapshot.duration - snapshot.position;
                    if remaining <= settings.crossfade_duration_ms as f64 / 1_000.0
                        && remaining > 0.5
                    {
                        playback_state.set_crossfade_initiated(true);
                        let task_state = Arc::clone(&state);
                        let task_runtime = runtime.clone();
                        runtime.spawn(async move {
                            initiate_crossfade(
                                &task_runtime,
                                task_state,
                                settings.crossfade_duration_ms,
                            )
                            .await;
                        });
                    }
                }
            }

            if let Some(song) = &snapshot.song
                && subsonic_scrobbles.update(&song.id, snapshot.position, snapshot.duration)
                && !manual_offline_enabled(&state.settings)
                && state.client.is_connected()
            {
                let client = state.client.clone();
                let song_id = song.id.clone();
                runtime.spawn(async move {
                    if let Err(error) = client.scrobble(&song_id, None, Some(true)).await {
                        warn!("Failed to submit Subsonic scrobble: {error}");
                    }
                });
            }

            let playback_finished = snapshot.duration > 0.0
                && snapshot.position >= snapshot.duration - 0.2
                && snapshot.song.is_some()
                && !snapshot.is_playing
                && !playback_state.is_crossfade_initiated();
            if playback_finished {
                playback_state.clear_finished_state();
                last_segment_index = 0;
                let task_state = Arc::clone(&state);
                let task_runtime = runtime.clone();
                runtime.spawn(async move {
                    if let Err(error) =
                        handle_playback_finished(&task_runtime, Arc::clone(&task_state)).await
                    {
                        warn!("Failed to advance playback after track ended: {error}");
                        task_state.events.playback_ended();
                    }
                });
                continue;
            }

            progress_counter = (progress_counter + 1) % 10;
            if progress_counter == 0
                && snapshot.is_playing
                && let Some(song) = &snapshot.song
                && crate::lastfm::handle_playback_progress(
                    &state,
                    song,
                    snapshot.position,
                    snapshot.duration,
                )
            {
                let task_state = Arc::clone(&state);
                runtime.spawn(async move {
                    let _ = crate::lastfm::retry_lastfm_queue_inner(
                        &task_state.settings,
                        &task_state,
                        false,
                    )
                    .await;
                });
            }

            state.events.publish_playback(snapshot);
        }
    })
}

/// Read normalization data from DB and compute gain for a song.
/// Returns None if no normalization data exists for this song.
fn get_normalization_gain(
    conn: &rusqlite::Connection,
    song_id: &str,
    album_id: &str,
    settings: &NormalizationSettings,
) -> Option<f32> {
    // Query normalization data for this song
    let norm_data: Option<(f64, f64)> = conn
        .query_row(
            "SELECT track_loudness_lufs, track_peak FROM normalization_data WHERE song_id = ?",
            [song_id],
            |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
        )
        .ok();

    let (track_lufs, track_peak) = norm_data?;

    // For album mode, use average album loudness to preserve relative dynamics
    let lufs = if settings.mode == NormalizationMode::Album {
        conn.query_row(
            "SELECT AVG(track_loudness_lufs) FROM normalization_data WHERE album_id = ?",
            [album_id],
            |row| row.get::<_, f64>(0),
        )
        .unwrap_or(track_lufs)
    } else {
        track_lufs
    };

    Some(loudness::calculate_gain(
        lufs,
        track_peak,
        settings.target_lufs,
        settings.pre_amp_db,
        settings.prevent_clipping,
    ))
}

#[cfg(test)]
mod tests {
    use super::{SubsonicScrobbleTracker, is_gapless_eligible};
    use rusqlite::{Connection, params};
    #[test]
    fn subsonic_scrobble_submits_once_at_half_duration() {
        let mut tracker = SubsonicScrobbleTracker::default();
        assert!(!tracker.update("song", 49.9, 100.0));
        assert!(tracker.update("song", 50.0, 100.0));
        assert!(!tracker.update("song", 75.0, 100.0));
    }

    #[test]
    fn subsonic_scrobble_resets_for_repeat_and_song_change() {
        let mut tracker = SubsonicScrobbleTracker::default();
        assert!(tracker.update("song", 50.0, 100.0));
        assert!(!tracker.update("song", 2.0, 100.0));
        assert!(tracker.update("song", 50.0, 100.0));
        assert!(tracker.update("next", 50.0, 100.0));
    }

    #[test]
    fn gapless_requires_consecutive_tracks_on_the_same_album() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE songs (
                    id TEXT PRIMARY KEY,
                    album_id TEXT NOT NULL,
                    disc_number INTEGER,
                    track_number INTEGER
                );",
            )
            .unwrap();
        for (id, album, disc, track) in [
            ("a1", "album-a", 1, 1),
            ("a2", "album-a", 1, 2),
            ("a3", "album-a", 2, 1),
            ("a4", "album-a", 1, 4),
            ("b1", "album-b", 1, 1),
        ] {
            connection
                .execute(
                    "INSERT INTO songs (id, album_id, disc_number, track_number)
                     VALUES (?, ?, ?, ?)",
                    params![id, album, disc, track],
                )
                .unwrap();
        }

        assert!(is_gapless_eligible(&connection, "a1", "a2"));
        assert!(is_gapless_eligible(&connection, "a2", "a3"));
        assert!(!is_gapless_eligible(&connection, "a1", "a4"));
        assert!(!is_gapless_eligible(&connection, "a1", "b1"));
    }
}
