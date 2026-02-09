use log::{info, warn};
use tauri::{AppHandle, Manager, State};

use crate::audio::binaural::BinauralPreset;
use crate::audio::compressor::DynamicsPreset;
use crate::audio::loudness;
use crate::audio::player::CrossfadePlayRequest;
use crate::audio::queue::RepeatMode;
use crate::audio::{PlaybackStatus, SongMetadata};
use crate::cache::AudioCache;
use crate::commands::queue::persist_and_emit;
use crate::commands::settings::{
    read_normalization_settings, read_playback_settings, NormalizationMode, NormalizationSettings,
};
use crate::db;
use crate::error::{AppError, AppResult, MutexExt};
use crate::state::AppState;

/// Common song data needed for playback.
struct SongData {
    audio_data: Vec<u8>,
    metadata: SongMetadata,
    duration: f64,
    album_id: String,
    suffix: String,
    normalization_gain: Option<f32>,
    dynamics_preset: Option<DynamicsPreset>,
    binaural_preset: Option<BinauralPreset>,
}

/// Fetch all data needed to play a song: metadata, audio bytes, normalization gain.
async fn fetch_song_data(
    app_handle: &AppHandle,
    state: &AppState,
    song_id: &str,
) -> AppResult<SongData> {
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

    let cache = AudioCache::new(app_handle)?;
    let audio_data = cache.get_or_fetch(&state.client, song_id, &suffix).await?;

    let norm_settings = read_normalization_settings(app_handle);
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
    let playback_settings = read_playback_settings(app_handle);
    let binaural_preset = if playback_settings.binaural_enabled {
        Some(playback_settings.binaural_preset.clone())
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
    })
}

/// Prefetch the next song in the queue for gapless playback.
/// Also triggers normalization analysis if enabled and no data exists.
fn prefetch_next_song(app_handle: &AppHandle, state: &AppState) {
    // Check if connected
    if !state.client.is_connected() {
        return;
    }

    // Get next song info from queue
    let next_song_id: Option<String> = {
        let queue = state.queue.lock_recover();
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
            let norm_settings = read_normalization_settings(app_handle);
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
            app_handle.clone(),
            state.client.clone(),
            song_id,
            suffix,
            album_id,
        );
    }
}

#[tauri::command]
pub async fn play_song(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    song_id: String,
) -> AppResult<()> {
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    let data = fetch_song_data(&app_handle, &state, &song_id).await?;

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
        )?;
    }

    // Spawn background loudness analysis if normalization is enabled but no data exists
    spawn_loudness_analysis_if_needed(
        &app_handle,
        &song_id,
        &data.album_id,
        &data.suffix,
        data.normalization_gain,
    );

    // Prefetch next song for gapless playback
    prefetch_next_song(&app_handle, &state);

    // Check if next song is gapless-eligible and queue it on the same Sink
    check_and_queue_gapless(&app_handle, &state);

    // Report "now playing" to Subsonic server
    // Fire and forget - don't fail playback if scrobble fails
    let _ = state.client.scrobble(&song_id, None, Some(false)).await;

    Ok(())
}

/// Play a song with crossfade transition from the currently playing track.
pub async fn crossfade_play_by_id(
    app_handle: &AppHandle,
    state: &AppState,
    song_id: &str,
    crossfade_duration_ms: u32,
) -> AppResult<()> {
    let data = fetch_song_data(app_handle, state, song_id).await?;

    {
        let audio_player = state.audio_player.lock_recover();
        audio_player.crossfade_play(CrossfadePlayRequest {
            audio_data: data.audio_data,
            metadata: data.metadata,
            duration_secs: data.duration,
            normalization_gain: data.normalization_gain,
            dynamics_preset: data.dynamics_preset,
            binaural_preset: data.binaural_preset,
            crossfade_duration_ms,
        })?;
    }

    spawn_loudness_analysis_if_needed(
        app_handle,
        song_id,
        &data.album_id,
        &data.suffix,
        data.normalization_gain,
    );

    prefetch_next_song(app_handle, state);
    check_and_queue_gapless(app_handle, state);

    let _ = state.client.scrobble(song_id, None, Some(false)).await;

    Ok(())
}

/// Initiate a crossfade transition to the next song in the queue.
/// Called by the position emitter when playback enters the crossfade window.
pub async fn initiate_crossfade(app_handle: &AppHandle, crossfade_duration_ms: u32) {
    let state: State<'_, AppState> = app_handle.state();

    // Get current and next song IDs
    let (current_song_id, next_song_id) = {
        let queue = state.queue.lock_recover();
        let current_id = queue.current_item().map(|i| i.song_id.clone());
        let next_id = queue.peek_next().map(|i| i.song_id.clone());
        match (current_id, next_id) {
            (Some(curr), Some(next)) => (curr, next),
            _ => return,
        }
    };

    // Don't crossfade if next song is gapless-eligible (gapless takes priority)
    let playback_settings = read_playback_settings(app_handle);
    if playback_settings.gapless_enabled {
        let eligible = {
            let conn = state.db.lock_recover();
            is_gapless_eligible(&conn, &current_song_id, &next_song_id)
        };
        if eligible {
            return;
        }
    }

    // Don't crossfade in repeat-one mode
    let repeat_mode = {
        let queue = state.queue.lock_recover();
        queue.repeat_mode()
    };
    if repeat_mode == RepeatMode::One {
        return;
    }

    info!("Initiating crossfade to song {}", next_song_id);

    let data = match fetch_song_data(app_handle, &state, &next_song_id).await {
        Ok(d) => d,
        Err(e) => {
            warn!("Crossfade: failed to fetch song data: {e}");
            return;
        }
    };

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
            crossfade_duration_ms,
        }) {
            warn!("Crossfade: failed to start: {e}");
            return;
        }
    }

    // Advance the queue (the new song is now playing)
    {
        let mut queue = state.queue.lock_recover();
        queue.next(false);
    }
    persist_and_emit(&state, app_handle);

    // Scrobble the new song
    let _ = state
        .client
        .scrobble(&next_song_id, None, Some(false))
        .await;

    // Prefetch next-next and check gapless eligibility
    prefetch_next_song(app_handle, &state);
    check_and_queue_gapless(app_handle, &state);
}

/// Spawn background loudness analysis if normalization is enabled but no data exists.
fn spawn_loudness_analysis_if_needed(
    app_handle: &AppHandle,
    song_id: &str,
    album_id: &str,
    suffix: &str,
    normalization_gain: Option<f32>,
) {
    let norm_settings = read_normalization_settings(app_handle);
    if normalization_gain.is_none() && norm_settings.enabled {
        let song_id = song_id.to_string();
        let album_id = album_id.to_string();
        let suffix = suffix.to_string();
        let app_handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let cache = match AudioCache::new(&app_handle) {
                Ok(c) => c,
                Err(_) => return,
            };
            let cache_path = cache.get_cache_path(&song_id, &suffix);
            let audio_data = match tokio::fs::read(&cache_path).await {
                Ok(data) => data,
                Err(_) => return,
            };
            let song_id_inner = song_id.clone();
            let app_handle_inner = app_handle.clone();
            let _ =
                tauri::async_runtime::spawn_blocking(move || {
                    match loudness::analyze_loudness(audio_data) {
                        Ok(result) => {
                            if let Ok(db_path) = db::get_db_path(&app_handle_inner) {
                                let _ = db::save_normalization_result(
                                    std::path::Path::new(&db_path),
                                    &song_id_inner,
                                    &album_id,
                                    result.integrated_lufs,
                                    result.true_peak,
                                );
                            }
                        }
                        Err(e) => {
                            warn!("Failed to analyze loudness for song: {e}");
                        }
                    }
                })
                .await;
        });
    }
}

#[tauri::command]
pub fn pause_playback(state: State<'_, AppState>) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.pause()
}

#[tauri::command]
pub fn resume_playback(state: State<'_, AppState>) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.resume()
}

#[tauri::command]
pub fn stop_playback(state: State<'_, AppState>) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.stop()
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, volume: f32) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.set_volume(volume)
}

#[tauri::command]
pub fn seek_playback(state: State<'_, AppState>, position: f64) -> AppResult<()> {
    let audio_player = state.audio_player.lock_recover();
    audio_player.seek(position)
}

#[tauri::command]
pub fn get_playback_status(state: State<'_, AppState>) -> PlaybackStatus {
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
fn check_and_queue_gapless(app_handle: &AppHandle, state: &AppState) {
    if !state.client.is_connected() {
        return;
    }

    // Check if gapless playback is enabled in settings
    if !read_playback_settings(app_handle).gapless_enabled {
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
        let queue = state.queue.lock_recover();
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
    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let state: State<'_, AppState> = app.state();

        let data = match fetch_song_data(&app, &state, &next_song_id).await {
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
        ) {
            warn!("Gapless: failed to append: {e}");
        }
    });
}

/// Handle the async portion of a gapless transition: scrobble, prefetch, and queue next.
/// The queue has already been advanced synchronously by the position emitter to prevent
/// a race with playback_finished detection.
pub async fn after_gapless_transition(app_handle: &AppHandle, next_song_id: Option<String>) {
    let state: State<'_, AppState> = app_handle.state();

    if let Some(ref song_id) = next_song_id {
        info!("Gapless transition to song {}", song_id);

        // Scrobble the new song
        let _ = state.client.scrobble(song_id, None, Some(false)).await;
    }

    // Prefetch the next-next song and check if it's also gapless-eligible
    prefetch_next_song(app_handle, &state);
    check_and_queue_gapless(app_handle, &state);
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
