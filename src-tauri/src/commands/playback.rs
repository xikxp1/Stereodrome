use stereodrome_audio::PlaybackStatus;
use tauri::State;

use crate::state::AppState;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_playback_status(state: State<'_, AppState>) -> PlaybackStatus {
    state.runtime_audio.status_snapshot()
}
