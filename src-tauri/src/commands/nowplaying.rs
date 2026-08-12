use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use log::warn;
use serde::{Deserialize, Serialize};
use stereodrome_core::{CoreCommand, NowPlayingEntry, StereodromeRuntimeHandle};
use tauri::{AppHandle, Emitter};

use crate::runtime::deserialize_result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowPlayingEvent {
    pub entries: Vec<NowPlayingEntry>,
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
