use tauri::State;

use crate::audio::{fetch_audio_bytes, PlaybackStatus};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub async fn play_song(state: State<'_, AppState>, song_id: String) -> AppResult<()> {
    // Get server config
    let config = state
        .server_config
        .lock()
        .unwrap()
        .clone()
        .ok_or(AppError::NotConnected)?;

    // Get song duration from database
    let duration: f64 = {
        let conn = state.db.lock().unwrap();
        conn.query_row(
            "SELECT duration FROM songs WHERE id = ?",
            [&song_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(AppError::Database)?
        .unwrap_or(0) as f64
    };

    // Fetch audio bytes from server
    let audio_data = fetch_audio_bytes(&config, &song_id).await?;

    // Play the audio
    let audio_player = state.audio_player.lock().unwrap();
    audio_player.play(audio_data, song_id, duration)?;

    Ok(())
}

#[tauri::command]
pub fn pause_playback(state: State<'_, AppState>) -> AppResult<()> {
    let audio_player = state.audio_player.lock().unwrap();
    audio_player.pause()
}

#[tauri::command]
pub fn resume_playback(state: State<'_, AppState>) -> AppResult<()> {
    let audio_player = state.audio_player.lock().unwrap();
    audio_player.resume()
}

#[tauri::command]
pub fn stop_playback(state: State<'_, AppState>) -> AppResult<()> {
    let audio_player = state.audio_player.lock().unwrap();
    audio_player.stop()
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, volume: f32) -> AppResult<()> {
    let audio_player = state.audio_player.lock().unwrap();
    audio_player.set_volume(volume)
}

#[tauri::command]
pub fn get_playback_status(state: State<'_, AppState>) -> PlaybackStatus {
    let audio_player = state.audio_player.lock().unwrap();
    audio_player.get_status()
}
