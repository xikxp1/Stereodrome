use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use log::warn;
use serde::{Deserialize, Serialize};
use stereodrome_core::{CoreCommand, NowPlayingEntry, PlaybackProgress, StereodromeRuntimeHandle};
use tauri::{AppHandle, Emitter, State};

use crate::error::AppResult;
use crate::runtime::{deserialize_result, dispatch_unit_async, snapshot};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowPlayingEvent {
    pub entries: Vec<NowPlayingEntry>,
}

#[tauri::command]
pub async fn scrobble_now_playing(state: State<'_, AppState>, song_id: String) -> AppResult<()> {
    report_current_progress(&state, song_id).await
}

#[tauri::command]
pub async fn scrobble_submit(
    state: State<'_, AppState>,
    song_id: String,
    _timestamp: Option<u64>,
) -> AppResult<()> {
    report_current_progress(&state, song_id).await
}

async fn report_current_progress(state: &AppState, song_id: String) -> AppResult<()> {
    let playback = snapshot(state)?.playback;
    dispatch_unit_async(
        state,
        CoreCommand::SavePlaybackPosition {
            progress: PlaybackProgress {
                song_id,
                position_seconds: playback.position_seconds,
                duration_seconds: playback.duration_seconds,
                is_playing: playback.is_playing,
            },
        },
    )
    .await
}

pub fn start_now_playing_emitter(
    app_handle: AppHandle,
    runtime: StereodromeRuntimeHandle,
    running: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(5));
            if !running.load(Ordering::SeqCst) {
                break;
            }

            match deserialize_result::<Vec<NowPlayingEntry>>(
                runtime.dispatch_command(CoreCommand::GetNowPlaying),
            ) {
                Ok(entries) => {
                    let _ = app_handle.emit("now-playing", NowPlayingEvent { entries });
                }
                Err(error) => warn!("Failed to fetch now playing through runtime: {error}"),
            }
        }
    });
}
