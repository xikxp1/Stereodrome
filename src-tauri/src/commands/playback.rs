use tauri::{AppHandle, State};

use crate::audio::{PlaybackStatus, SongMetadata};
use crate::cache::{AudioCache, DEFAULT_MAX_CACHE_SIZE};
use crate::error::{AppError, AppResult, MutexExt};
use crate::state::AppState;

#[tauri::command]
pub async fn play_song(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    song_id: String,
) -> AppResult<()> {
    // Get server config
    let config = state
        .server_config
        .lock()
        .unwrap()
        .clone()
        .ok_or(AppError::NotConnected)?;

    // Get song metadata from database (join with artists and albums for names and cover art)
    let (duration, title, artist, album, cover_art_id, suffix): (
        f64,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = {
        let conn = state.db.lock_recover();
        conn.query_row(
            "SELECT s.duration, s.title, a.name, al.name, al.cover_art_id, s.suffix
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
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?.unwrap_or_default(),
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
    let cache = AudioCache::new(&app_handle, DEFAULT_MAX_CACHE_SIZE)?;
    let audio_data = cache.get_or_fetch(&config, &song_id, &suffix).await?;

    // Play the audio
    {
        let audio_player = state.audio_player.lock_recover();
        audio_player.play(audio_data, metadata, duration)?;
    }

    // Report "now playing" to Subsonic server
    let client_opt = state.client.lock_recover().clone();
    if let Some(client) = client_opt {
        // Fire and forget - don't fail playback if scrobble fails
        let _ = client
            .scrobble(vec![(song_id, None::<usize>)], Some(false))
            .await;
    }

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
