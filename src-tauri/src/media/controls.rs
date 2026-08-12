use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use log::{debug, error, info, warn};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use stereodrome_core::{CoreCommand, PlaybackNavigation};
use tauri::{AppHandle, Manager};

use crate::audio::SongMetadata;
use crate::state::AppState;

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
        #[cfg(target_os = "windows")]
        let hwnd = Some(media_controls_hwnd(&app_handle)?);
        #[cfg(not(target_os = "windows"))]
        let hwnd = None;

        let (command_tx, command_rx) = mpsc::channel::<MediaCommand>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        let thread = thread::spawn(move || {
            run_media_controls_thread(command_rx, app_handle, hwnd, init_tx);
        });

        match init_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Some(Self {
                command_tx,
                _thread: thread,
            }),
            Ok(Err(e)) => {
                error!("Media controls initialization failed: {e}");
                let _ = thread.join();
                None
            }
            Err(e) => {
                error!("Media controls initialization did not complete: {e}");
                None
            }
        }
    }

    pub fn update_metadata(
        &self,
        song: &SongMetadata,
        duration_secs: f64,
        cover_art_path: Option<String>,
    ) {
        if let Err(e) = self.command_tx.send(MediaCommand::UpdateMetadata {
            song: song.clone(),
            duration_secs,
            cover_art_path,
        }) {
            warn!("Failed to send media metadata update: {e}");
        }
    }

    pub fn set_playback_status(&self, is_playing: bool, position_secs: f64) {
        if let Err(e) = self.command_tx.send(MediaCommand::SetPlaybackStatus {
            is_playing,
            position_secs,
        }) {
            warn!("Failed to send media playback status update: {e}");
        }
    }

    pub fn clear(&self) {
        if let Err(e) = self.command_tx.send(MediaCommand::Clear) {
            warn!("Failed to send media controls clear command: {e}");
        }
    }
}

impl Drop for MediaControlsManager {
    fn drop(&mut self) {
        let _ = self.command_tx.send(MediaCommand::Shutdown);
    }
}

#[cfg(target_os = "windows")]
fn media_controls_hwnd(app_handle: &AppHandle) -> Option<usize> {
    use tauri::Manager as _;

    let Some(window) = app_handle.get_webview_window("main") else {
        error!("Failed to initialize media controls: main window not found");
        return None;
    };

    match window.hwnd() {
        Ok(hwnd) => Some(hwnd.0 as usize),
        Err(e) => {
            error!("Failed to initialize media controls: could not get main window HWND: {e}");
            None
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn run_media_controls_thread(
    command_rx: mpsc::Receiver<MediaCommand>,
    app_handle: AppHandle,
    hwnd: Option<usize>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
) {
    let config = PlatformConfig {
        dbus_name: "stereodrome",
        display_name: "Stereodrome",
        hwnd: hwnd.map(|value| value as *mut std::ffi::c_void),
    };

    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to create media controls: {e:?}");
            let _ = init_tx.send(Err(format!("failed to create media controls: {e:?}")));
            return;
        }
    };

    let event_app_handle = app_handle.clone();
    if let Err(e) = controls.attach(move |event: MediaControlEvent| {
        handle_media_event(&event_app_handle, event);
    }) {
        error!("Failed to attach media event handler: {e:?}");
        let _ = init_tx.send(Err(format!("failed to attach media event handler: {e:?}")));
        return;
    }

    info!("Media controls initialized");
    let _ = init_tx.send(Ok(()));

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

                    let cover_url = cover_art_path.as_deref().map(cover_art_file_url);

                    let metadata = MediaMetadata {
                        title: Some(&song.title),
                        artist: Some(&song.artist),
                        album: Some(&song.album),
                        cover_url: cover_url.as_deref(),
                        duration: Some(Duration::from_secs_f64(duration_secs)),
                    };

                    if let Err(e) = controls.set_metadata(metadata) {
                        debug!("Failed to set metadata: {e:?}");
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
                        debug!("Failed to set playback status: {e:?}");
                    }
                }
                MediaCommand::Clear => {
                    if let Err(e) = controls.set_playback(MediaPlayback::Stopped) {
                        debug!("Failed to clear playback: {e:?}");
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

#[allow(clippy::needless_pass_by_value)]
fn handle_media_event(app_handle: &AppHandle, event: MediaControlEvent) {
    let command = match event {
        MediaControlEvent::Play => CoreCommand::ResumePlayback,
        MediaControlEvent::Pause => CoreCommand::PausePlayback,
        MediaControlEvent::Toggle => CoreCommand::TogglePlayback,
        MediaControlEvent::Next => CoreCommand::NavigatePlayback {
            navigation: PlaybackNavigation::Next { force: true },
        },
        MediaControlEvent::Previous => CoreCommand::NavigatePlayback {
            navigation: PlaybackNavigation::Previous,
        },
        MediaControlEvent::Stop => CoreCommand::StopPlayback,
        MediaControlEvent::Seek(direction) => CoreCommand::SeekBy {
            seconds: seek_delta_secs(direction, Duration::from_secs(10)),
        },
        MediaControlEvent::SeekBy(direction, duration) => CoreCommand::SeekBy {
            seconds: seek_delta_secs(direction, duration),
        },
        MediaControlEvent::SetPosition(MediaPosition(position)) => CoreCommand::SeekTo {
            seconds: position.as_secs_f64(),
        },
        _ => return,
    };

    debug!("Dispatching media control intent");
    if let Some(state) = app_handle.try_state::<AppState>() {
        let _ = state.runtime.dispatch_command(command);
    }
}

fn seek_delta_secs(direction: SeekDirection, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    match direction {
        SeekDirection::Forward => seconds,
        SeekDirection::Backward => -seconds,
    }
}

fn cover_art_file_url(path: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!("file://{path}")
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!("file://{}", path.replace(' ', "%20"))
    }
}
