use log::warn;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

use crate::client::SubsonicClientHandle;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowPlayingEntry {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<i32>,
    pub cover_art: Option<String>,
    pub username: String,
    pub minutes_ago: i32,
    pub player_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowPlayingEvent {
    pub entries: Vec<NowPlayingEntry>,
}

/// Report a song as "now playing" to the Subsonic server.
/// This should be called when playback starts.
#[tauri::command]
pub async fn scrobble_now_playing(state: State<'_, AppState>, song_id: String) -> AppResult<()> {
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    // scrobble with submission=false means "now playing" notification
    state
        .client
        .scrobble(&song_id, None, Some(false))
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    Ok(())
}

/// Submit a completed scrobble to the Subsonic server.
/// This should be called when a song finishes playing (or reaches a scrobble threshold).
#[tauri::command]
pub async fn scrobble_submit(
    state: State<'_, AppState>,
    song_id: String,
    timestamp: Option<u64>,
) -> AppResult<()> {
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    // scrobble with submission=true (or None, which defaults to true) means submit the scrobble
    let time = timestamp.map(|t| t as usize);
    state
        .client
        .scrobble(&song_id, time, Some(true))
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    Ok(())
}

/// Start the now playing emitter that polls the server and emits events
pub fn start_now_playing_emitter(
    app_handle: AppHandle,
    client: SubsonicClientHandle,
    running: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for now playing emitter");

        loop {
            // Check if we should stop
            if !running.load(Ordering::SeqCst) {
                break;
            }

            // Sleep for 5 seconds between polls
            thread::sleep(Duration::from_secs(5));

            // Check connection without lock
            if !client.is_connected() {
                continue;
            }

            // Fetch now playing from server via client handle
            let result = runtime.block_on(async { client.get_now_playing().await });

            match result {
                Ok(now_playing) => {
                    let entries: Vec<NowPlayingEntry> = now_playing
                        .entry
                        .into_iter()
                        .map(|e| NowPlayingEntry {
                            id: e.child.id,
                            title: e.child.title,
                            artist: e.child.artist,
                            album: e.child.album,
                            duration: e.child.duration,
                            cover_art: e.child.cover_art,
                            username: e.username,
                            minutes_ago: e.minutes_ago,
                            player_name: e.player_name,
                        })
                        .collect();

                    let _ = app_handle.emit("now-playing", NowPlayingEvent { entries });
                }
                Err(e) => {
                    warn!("Failed to fetch now playing: {}", e);
                }
            }
        }
    });
}
