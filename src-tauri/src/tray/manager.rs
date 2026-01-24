use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use log::{debug, error, info};
use tauri::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager};

const MENU_ID_APP_INFO: &str = "app_info";
const MENU_ID_NOW_PLAYING: &str = "now_playing";
const MENU_ID_PLAY_PAUSE: &str = "play_pause";
const MENU_ID_NEXT: &str = "next";
const MENU_ID_PREVIOUS: &str = "previous";
const MENU_ID_SHOW: &str = "show";
const MENU_ID_QUIT: &str = "quit";

#[derive(Debug)]
pub enum TrayCommand {
    UpdatePlaybackState { is_playing: bool },
    UpdateSongInfo { title: String, artist: String },
    UpdateAvailable { version: Option<String> },
    Shutdown,
}

struct TrayState {
    tray: TrayIcon,
    app_info_item: MenuItem<tauri::Wry>,
    app_version: String,
    play_pause_item: MenuItem<tauri::Wry>,
    now_playing_item: MenuItem<tauri::Wry>,
}

pub struct TrayManager {
    command_tx: Sender<TrayCommand>,
    _thread: thread::JoinHandle<()>,
}

impl TrayManager {
    pub fn new(app: &tauri::App) -> Option<Self> {
        // Get app version
        let version = app.package_info().version.to_string();
        let app_info_text = format!("Stereodrome v{}", version);

        // Create menu items
        let app_info_item =
            match MenuItem::with_id(app, MENU_ID_APP_INFO, &app_info_text, false, None::<&str>) {
                Ok(item) => item,
                Err(e) => {
                    error!("Failed to create app info menu item: {:?}", e);
                    return None;
                }
            };

        let now_playing_item =
            match MenuItem::with_id(app, MENU_ID_NOW_PLAYING, "Not Playing", false, None::<&str>) {
                Ok(item) => item,
                Err(e) => {
                    error!("Failed to create now playing menu item: {:?}", e);
                    return None;
                }
            };

        let play_pause_item =
            match MenuItem::with_id(app, MENU_ID_PLAY_PAUSE, "Play", true, None::<&str>) {
                Ok(item) => item,
                Err(e) => {
                    error!("Failed to create play/pause menu item: {:?}", e);
                    return None;
                }
            };

        let next_item = match MenuItem::with_id(app, MENU_ID_NEXT, "Next Track", true, None::<&str>)
        {
            Ok(item) => item,
            Err(e) => {
                error!("Failed to create next menu item: {:?}", e);
                return None;
            }
        };

        let previous_item =
            match MenuItem::with_id(app, MENU_ID_PREVIOUS, "Previous Track", true, None::<&str>) {
                Ok(item) => item,
                Err(e) => {
                    error!("Failed to create previous menu item: {:?}", e);
                    return None;
                }
            };

        let show_item =
            match MenuItem::with_id(app, MENU_ID_SHOW, "Show Window", true, None::<&str>) {
                Ok(item) => item,
                Err(e) => {
                    error!("Failed to create show menu item: {:?}", e);
                    return None;
                }
            };

        // Platform-specific quit accelerator: Cmd+Q on macOS, none on Windows/Linux
        #[cfg(target_os = "macos")]
        let quit_accelerator = Some("Cmd+Q");
        #[cfg(not(target_os = "macos"))]
        let quit_accelerator: Option<&str> = None;

        let quit_item = match MenuItem::with_id(
            app,
            MENU_ID_QUIT,
            "Quit Stereodrome",
            true,
            quit_accelerator,
        ) {
            Ok(item) => item,
            Err(e) => {
                error!("Failed to create quit menu item: {:?}", e);
                return None;
            }
        };

        let separator = match PredefinedMenuItem::separator(app) {
            Ok(sep) => sep,
            Err(e) => {
                error!("Failed to create separator: {:?}", e);
                return None;
            }
        };

        // Build menu
        let menu = match Menu::with_items(
            app,
            &[
                &app_info_item,
                &show_item,
                &separator,
                &now_playing_item,
                &play_pause_item,
                &next_item,
                &previous_item,
                &separator,
                &quit_item,
            ],
        ) {
            Ok(menu) => menu,
            Err(e) => {
                error!("Failed to create tray menu: {:?}", e);
                return None;
            }
        };

        // Get app icon
        let icon = match app.default_window_icon() {
            Some(icon) => icon.clone(),
            None => {
                error!("No default window icon available");
                return None;
            }
        };

        // Build tray icon with event handlers
        let tray = match TrayIconBuilder::new()
            .icon(icon)
            .tooltip("Stereodrome")
            .menu(&menu)
            .on_menu_event(move |app, event| {
                handle_menu_event(app, event.id());
            })
            .build(app)
        {
            Ok(tray) => tray,
            Err(e) => {
                error!("Failed to create tray icon: {:?}", e);
                return None;
            }
        };

        // Create channel for commands
        let (command_tx, command_rx) = mpsc::channel::<TrayCommand>();

        // Store tray state for background thread
        let tray_state = Arc::new(Mutex::new(TrayState {
            tray,
            app_info_item,
            app_version: version,
            play_pause_item,
            now_playing_item,
        }));

        // Spawn background thread to handle state updates
        let thread = thread::spawn(move || {
            run_tray_thread(command_rx, tray_state);
        });

        Some(Self {
            command_tx,
            _thread: thread,
        })
    }

    pub fn update_playback_state(&self, is_playing: bool) {
        let _ = self
            .command_tx
            .send(TrayCommand::UpdatePlaybackState { is_playing });
    }

    pub fn update_song_info(&self, title: &str, artist: &str) {
        let _ = self.command_tx.send(TrayCommand::UpdateSongInfo {
            title: title.to_string(),
            artist: artist.to_string(),
        });
    }

    pub fn update_update_available(&self, version: Option<&str>) {
        let _ = self.command_tx.send(TrayCommand::UpdateAvailable {
            version: version.map(|v| v.to_string()),
        });
    }
}

impl Drop for TrayManager {
    fn drop(&mut self) {
        let _ = self.command_tx.send(TrayCommand::Shutdown);
    }
}

fn run_tray_thread(command_rx: mpsc::Receiver<TrayCommand>, tray_state: Arc<Mutex<TrayState>>) {
    info!("Tray manager thread started");

    loop {
        match command_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(command) => match command {
                TrayCommand::UpdatePlaybackState { is_playing } => {
                    if let Ok(state) = tray_state.lock() {
                        let text = if is_playing { "Pause" } else { "Play" };
                        if let Err(e) = state.play_pause_item.set_text(text) {
                            debug!("Failed to update play/pause text: {:?}", e);
                        }
                    }
                }
                TrayCommand::UpdateSongInfo { title, artist } => {
                    if let Ok(state) = tray_state.lock() {
                        let now_playing = if title.is_empty() {
                            "Not Playing".to_string()
                        } else {
                            format!("{} - {}", artist, title)
                        };

                        if let Err(e) = state.now_playing_item.set_text(&now_playing) {
                            debug!("Failed to update now playing text: {:?}", e);
                        }

                        // Also update tooltip
                        if let Err(e) = state.tray.set_tooltip(Some(&format!(
                            "Stereodrome{}",
                            if title.is_empty() {
                                String::new()
                            } else {
                                format!("\n{} - {}", artist, title)
                            }
                        ))) {
                            debug!("Failed to update tooltip: {:?}", e);
                        }
                    }
                }
                TrayCommand::UpdateAvailable { version } => {
                    if let Ok(state) = tray_state.lock() {
                        match version {
                            Some(_) => {
                                let text = format!(
                                    "Stereodrome v{} - Update Available",
                                    state.app_version
                                );
                                if let Err(e) = state.app_info_item.set_text(&text) {
                                    debug!("Failed to update app info text: {:?}", e);
                                }
                            }
                            None => {
                                let text = format!("Stereodrome v{}", state.app_version);
                                if let Err(e) = state.app_info_item.set_text(&text) {
                                    debug!("Failed to reset app info text: {:?}", e);
                                }
                            }
                        }
                    }
                }
                TrayCommand::Shutdown => {
                    info!("Tray manager shutting down");
                    break;
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                info!("Tray command channel disconnected");
                break;
            }
        }
    }
}

fn handle_menu_event(app: &AppHandle, menu_id: &MenuId) {
    match menu_id.as_ref() {
        MENU_ID_PLAY_PAUSE => {
            debug!("Tray: play_pause clicked");
            let _ = app.emit(
                "tray-control",
                serde_json::json!({ "action": "play_pause" }),
            );
        }
        MENU_ID_NEXT => {
            debug!("Tray: next clicked");
            let _ = app.emit("tray-control", serde_json::json!({ "action": "next" }));
        }
        MENU_ID_PREVIOUS => {
            debug!("Tray: previous clicked");
            let _ = app.emit("tray-control", serde_json::json!({ "action": "previous" }));
        }
        MENU_ID_SHOW => {
            debug!("Tray: show clicked");
            show_main_window(app);
        }
        MENU_ID_QUIT => {
            debug!("Tray: quit clicked");
            app.exit(0);
        }
        _ => {}
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // On macOS, activate the app first so set_focus() works
        #[cfg(target_os = "macos")]
        {
            use objc2::MainThreadMarker;
            use objc2_app_kit::NSApplication;
            // Safety: Tauri menu events run on the main thread
            let mtm = unsafe { MainThreadMarker::new_unchecked() };
            let app = NSApplication::sharedApplication(mtm);
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }

        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
