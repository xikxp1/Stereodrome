use stereodrome_desktop::DesktopBackend;
use stereodrome_desktop::audio::PlaybackStatus;
use tauri::State;

use crate::error::AppResult;

#[tauri::command]
pub async fn play_song(backend: State<'_, DesktopBackend>, song_id: String) -> AppResult<()> {
    stereodrome_desktop::operations::playback::play_song_by_id(
        &backend.runtime_handle(),
        backend.state(),
        &song_id,
    )
    .await
}

#[tauri::command]
pub fn pause_playback(backend: State<'_, DesktopBackend>) -> AppResult<()> {
    stereodrome_desktop::operations::playback::pause_playback(&backend.state())
}

#[tauri::command]
pub fn resume_playback(backend: State<'_, DesktopBackend>) -> AppResult<()> {
    stereodrome_desktop::operations::playback::resume_playback(&backend.state())
}

#[tauri::command]
pub fn stop_playback(backend: State<'_, DesktopBackend>) -> AppResult<()> {
    stereodrome_desktop::operations::playback::stop_playback(&backend.state())
}

#[tauri::command]
pub fn set_volume(backend: State<'_, DesktopBackend>, volume: f32) -> AppResult<()> {
    stereodrome_desktop::operations::playback::set_volume(&backend.state(), volume)
}

#[tauri::command]
pub fn seek_playback(backend: State<'_, DesktopBackend>, position: f64) -> AppResult<()> {
    stereodrome_desktop::operations::playback::seek_playback(&backend.state(), position)
}

#[tauri::command]
pub fn get_playback_status(backend: State<'_, DesktopBackend>) -> PlaybackStatus {
    stereodrome_desktop::operations::playback::get_playback_status(&backend.state())
}
