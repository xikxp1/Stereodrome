use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use log::{debug, error, info};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};
use tauri::{AppHandle, Emitter};

use crate::audio::SongMetadata;

#[derive(Debug)]
pub enum MediaCommand {
    UpdateMetadata {
        song: SongMetadata,
        duration_secs: f64,
        cover_art_path: Option<String>,
    },
    SetPlaybackStatus {
        is_playing: bool,
        position_secs: f64,
    },
    Clear,
    Shutdown,
}

/// Manages OS media controls integration via souvlaki
pub struct MediaControlsManager {
    command_tx: Sender<MediaCommand>,
    _thread: thread::JoinHandle<()>,
}

impl MediaControlsManager {
    pub fn new(app_handle: AppHandle) -> Option<Self> {
        let (command_tx, command_rx) = mpsc::channel::<MediaCommand>();

        let thread = thread::spawn(move || {
            run_media_controls_thread(command_rx, app_handle);
        });

        Some(Self {
            command_tx,
            _thread: thread,
        })
    }

    pub fn update_metadata(
        &self,
        song: &SongMetadata,
        duration_secs: f64,
        cover_art_path: Option<String>,
    ) {
        let _ = self.command_tx.send(MediaCommand::UpdateMetadata {
            song: song.clone(),
            duration_secs,
            cover_art_path,
        });
    }

    pub fn set_playback_status(&self, is_playing: bool, position_secs: f64) {
        let _ = self.command_tx.send(MediaCommand::SetPlaybackStatus {
            is_playing,
            position_secs,
        });
    }

    pub fn clear(&self) {
        let _ = self.command_tx.send(MediaCommand::Clear);
    }
}

impl Drop for MediaControlsManager {
    fn drop(&mut self) {
        let _ = self.command_tx.send(MediaCommand::Shutdown);
    }
}

fn run_media_controls_thread(command_rx: mpsc::Receiver<MediaCommand>, app_handle: AppHandle) {
    let config = PlatformConfig {
        dbus_name: "stereodrome",
        display_name: "Stereodrome",
        hwnd: None,
    };

    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to create media controls: {:?}", e);
            return;
        }
    };

    let event_app_handle = app_handle.clone();
    if let Err(e) = controls.attach(move |event: MediaControlEvent| {
        handle_media_event(&event_app_handle, event);
    }) {
        error!("Failed to attach media event handler: {:?}", e);
    }

    info!("Media controls initialized");

    loop {
        match command_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(command) => match command {
                MediaCommand::UpdateMetadata {
                    song,
                    duration_secs,
                    cover_art_path,
                } => {
                    debug!(
                        "Updating media metadata: {} - {}, cover_art_path: {:?}",
                        song.artist, song.title, cover_art_path
                    );

                    let cover_url =
                        cover_art_path.map(|p| format!("file://{}", p.replace(' ', "%20")));

                    let metadata = MediaMetadata {
                        title: Some(&song.title),
                        artist: Some(&song.artist),
                        album: Some(&song.album),
                        cover_url: cover_url.as_deref(),
                        duration: Some(Duration::from_secs_f64(duration_secs)),
                    };

                    if let Err(e) = controls.set_metadata(metadata) {
                        debug!("Failed to set metadata: {:?}", e);
                    }
                }
                MediaCommand::SetPlaybackStatus {
                    is_playing,
                    position_secs,
                } => {
                    let progress = Some(MediaPosition(Duration::from_secs_f64(position_secs)));
                    let playback = if is_playing {
                        MediaPlayback::Playing { progress }
                    } else {
                        MediaPlayback::Paused { progress }
                    };

                    if let Err(e) = controls.set_playback(playback) {
                        debug!("Failed to set playback status: {:?}", e);
                    }
                }
                MediaCommand::Clear => {
                    if let Err(e) = controls.set_playback(MediaPlayback::Stopped) {
                        debug!("Failed to clear playback: {:?}", e);
                    }
                }
                MediaCommand::Shutdown => {
                    info!("Shutting down media controls");
                    break;
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                info!("Media controls channel disconnected");
                break;
            }
        }
    }
}

fn handle_media_event(app_handle: &AppHandle, event: MediaControlEvent) {
    let action = match event {
        MediaControlEvent::Play => "play",
        MediaControlEvent::Pause => "pause",
        MediaControlEvent::Toggle => "play_pause",
        MediaControlEvent::Next => "next",
        MediaControlEvent::Previous => "previous",
        MediaControlEvent::Stop => "stop",
        _ => return,
    };

    debug!("Media control event: {}", action);
    let _ = app_handle.emit("media-control", serde_json::json!({ "action": action }));
}
