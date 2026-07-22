use stereodrome_audio::PlaybackStatus;
use stereodrome_core::CoreCommand;
use tauri::State;

use crate::error::AppResult;
use crate::runtime::{dispatch_unit, dispatch_unit_async};
use crate::state::AppState;

#[tauri::command]
pub async fn play_song(state: State<'_, AppState>, song_id: String) -> AppResult<()> {
    dispatch_unit_async(
        &state,
        CoreCommand::PlaySelection {
            song_id: song_id.clone(),
            song_ids: vec![song_id],
        },
    )
    .await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn pause_playback(state: State<'_, AppState>) -> AppResult<()> {
    dispatch_unit(&state, CoreCommand::PausePlayback)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn resume_playback(state: State<'_, AppState>) -> AppResult<()> {
    dispatch_unit(&state, CoreCommand::ResumePlayback)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn stop_playback(state: State<'_, AppState>) -> AppResult<()> {
    dispatch_unit(&state, CoreCommand::StopPlayback)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_volume(state: State<'_, AppState>, volume: f32) -> AppResult<()> {
    dispatch_unit(&state, CoreCommand::SetPlaybackVolume { volume })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn seek_playback(state: State<'_, AppState>, position: f64) -> AppResult<()> {
    dispatch_unit(&state, CoreCommand::SeekTo { seconds: position })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_playback_status(state: State<'_, AppState>) -> PlaybackStatus {
    state.runtime_audio.status_snapshot()
}
