use log::warn;
use tauri::{AppHandle, State};

use crate::audio::loudness;
use crate::audio::{PlaybackStatus, SongMetadata};
use crate::cache::AudioCache;
use crate::commands::settings::{
    read_normalization_settings, NormalizationMode, NormalizationSettings,
};
use crate::db;
use crate::error::{AppError, AppResult, MutexExt};
use crate::state::AppState;

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
    // Check if connected
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    // Get song metadata from database (join with artists and albums for names and cover art)
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
            [&song_id],
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
        id: song_id.clone(),
        title,
        artist,
        album,
        cover_art_id,
    };

    // Fetch audio bytes (from cache or server)
    let cache = AudioCache::new(&app_handle)?;
    let audio_data = cache.get_or_fetch(&state.client, &song_id, &suffix).await?;

    // Calculate normalization gain if enabled
    let norm_settings = read_normalization_settings(&app_handle);
    let normalization_gain = if norm_settings.enabled {
        let conn = state.db.lock_recover();
        get_normalization_gain(&conn, &song_id, &album_id, &norm_settings)
    } else {
        None
    };

    // Play the audio
    {
        let audio_player = state.audio_player.lock_recover();
        audio_player.play(audio_data, metadata, duration, normalization_gain)?;
    }

    // Spawn background loudness analysis if normalization is enabled but no data exists
    if normalization_gain.is_none() && norm_settings.enabled {
        let song_id_clone = song_id.clone();
        let album_id_clone = album_id;
        let app_handle_clone = app_handle.clone();
        let suffix_clone = suffix.clone();
        tauri::async_runtime::spawn(async move {
            let cache = match AudioCache::new(&app_handle_clone) {
                Ok(c) => c,
                Err(_) => return,
            };
            let cache_path = cache.get_cache_path(&song_id_clone, &suffix_clone);
            let audio_data = match tokio::fs::read(&cache_path).await {
                Ok(data) => data,
                Err(_) => return,
            };
            let song_id_inner = song_id_clone.clone();
            let app_handle_inner = app_handle_clone.clone();
            let _ =
                tauri::async_runtime::spawn_blocking(move || {
                    match loudness::analyze_loudness(audio_data) {
                        Ok(result) => {
                            if let Ok(db_path) = db::get_db_path(&app_handle_inner) {
                                let _ = db::save_normalization_result(
                                    std::path::Path::new(&db_path),
                                    &song_id_inner,
                                    &album_id_clone,
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

    // Prefetch next song for gapless playback
    prefetch_next_song(&app_handle, &state);

    // Report "now playing" to Subsonic server
    // Fire and forget - don't fail playback if scrobble fails
    let _ = state.client.scrobble(&song_id, None, Some(false)).await;

    Ok(())
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
