use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use stereodrome_audio::PlaybackState;
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

const MENU_SHOW: &str = "show";
const MENU_SETTINGS: &str = "settings";
const MENU_PLAY_PAUSE: &str = "play_pause";
const MENU_NEXT: &str = "next";
const MENU_PREVIOUS: &str = "previous";
const MENU_QUIT: &str = "quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Settings,
    TogglePlayback,
    Next,
    Previous,
    Quit,
}

struct TrayParts {
    tray: TrayIcon,
    app_info: MenuItem,
    now_playing: MenuItem,
    play_pause: MenuItem,
    next: MenuItem,
    previous: MenuItem,
}

impl TrayParts {
    fn update(&self, state: &PlaybackState, has_queue: bool, updater_status: &str) {
        let has_song = state.song.is_some();
        self.play_pause
            .set_text(if state.is_playing { "Pause" } else { "Play" });
        self.play_pause.set_enabled(has_song);
        self.next.set_enabled(has_queue);
        self.previous.set_enabled(has_queue);

        let now_playing = state
            .song
            .as_ref()
            .map(|song| {
                if song.artist.is_empty() {
                    song.title.clone()
                } else {
                    format!("{} - {}", song.artist, song.title)
                }
            })
            .unwrap_or_else(|| "Not Playing".to_string());
        self.now_playing.set_text(&now_playing);
        let tooltip = if has_song {
            format!("Stereodrome\n{now_playing}")
        } else {
            "Stereodrome".to_string()
        };
        let _ = self.tray.set_tooltip(Some(&tooltip));

        let app_info = if updater_status == "idle" {
            format!("Stereodrome v{}", env!("CARGO_PKG_VERSION"))
        } else {
            format!(
                "Stereodrome v{} - Update {updater_status}",
                env!("CARGO_PKG_VERSION")
            )
        };
        self.app_info.set_text(app_info);
    }
}

#[cfg(target_os = "linux")]
enum TrayCommand {
    Update(PlaybackState, bool, &'static str),
    Shutdown,
}

pub struct TrayService {
    #[cfg(not(target_os = "linux"))]
    parts: Option<TrayParts>,
    #[cfg(target_os = "linux")]
    commands: std::sync::mpsc::Sender<TrayCommand>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TrayService {
    pub fn new() -> Result<(Self, async_channel::Receiver<TrayAction>), String> {
        let (sender, receiver) = async_channel::unbounded();
        let stop = Arc::new(AtomicBool::new(false));

        #[cfg(not(target_os = "linux"))]
        {
            let parts = create_tray()?;
            let thread_stop = Arc::clone(&stop);
            let thread = thread::Builder::new()
                .name("stereodrome-tray-events".to_string())
                .spawn(move || menu_event_loop(thread_stop, sender))
                .map_err(|error| format!("Failed to start tray event listener: {error}"))?;
            Ok((
                Self {
                    parts: Some(parts),
                    stop,
                    thread: Some(thread),
                },
                receiver,
            ))
        }

        #[cfg(target_os = "linux")]
        {
            let (commands, command_receiver) = std::sync::mpsc::channel();
            let (ready_sender, ready_receiver) = std::sync::mpsc::sync_channel(1);
            let thread_stop = Arc::clone(&stop);
            let thread = thread::Builder::new()
                .name("stereodrome-tray".to_string())
                .spawn(move || {
                    if let Err(error) = gtk::init() {
                        let _ = ready_sender.send(Err(format!(
                            "Failed to initialize GTK tray event loop: {error}"
                        )));
                        return;
                    }
                    let parts = match create_tray() {
                        Ok(parts) => parts,
                        Err(error) => {
                            let _ = ready_sender.send(Err(error));
                            return;
                        }
                    };
                    let _ = ready_sender.send(Ok(()));
                    while !thread_stop.load(Ordering::Acquire) {
                        while gtk::events_pending() {
                            gtk::main_iteration();
                        }
                        match command_receiver.recv_timeout(Duration::from_millis(20)) {
                            Ok(TrayCommand::Update(state, has_queue, updater)) => {
                                parts.update(&state, has_queue, updater)
                            }
                            Ok(TrayCommand::Shutdown) => break,
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        }
                        while let Ok(event) = MenuEvent::receiver().try_recv() {
                            if let Some(action) = action_for_menu_id(event.id.as_ref()) {
                                let _ = sender.try_send(action);
                            }
                        }
                    }
                })
                .map_err(|error| format!("Failed to start Linux tray thread: {error}"))?;
            ready_receiver
                .recv()
                .map_err(|_| "Linux tray thread stopped during startup".to_string())??;
            Ok((
                Self {
                    commands,
                    stop,
                    thread: Some(thread),
                },
                receiver,
            ))
        }
    }

    pub fn update(&self, state: &PlaybackState, has_queue: bool, updater_status: &'static str) {
        #[cfg(not(target_os = "linux"))]
        if let Some(parts) = &self.parts {
            parts.update(state, has_queue, updater_status);
        }
        #[cfg(target_os = "linux")]
        {
            let _ = self.commands.send(TrayCommand::Update(
                state.clone(),
                has_queue,
                updater_status,
            ));
        }
    }

    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        #[cfg(target_os = "linux")]
        let _ = self.commands.send(TrayCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        #[cfg(not(target_os = "linux"))]
        self.parts.take();
    }
}

impl Drop for TrayService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn create_tray() -> Result<TrayParts, String> {
    let app_info = MenuItem::new(
        format!("Stereodrome v{}", env!("CARGO_PKG_VERSION")),
        false,
        None,
    );
    let show = MenuItem::with_id(MENU_SHOW, "Show Stereodrome", true, None);
    let settings = MenuItem::with_id(MENU_SETTINGS, "Settings", true, None);
    let now_playing = MenuItem::new("Not Playing", false, None);
    let play_pause = MenuItem::with_id(MENU_PLAY_PAUSE, "Play", false, None);
    let next = MenuItem::with_id(MENU_NEXT, "Next Track", false, None);
    let previous = MenuItem::with_id(MENU_PREVIOUS, "Previous Track", false, None);
    let quit = MenuItem::with_id(MENU_QUIT, "Quit Stereodrome", true, None);
    let separator_a = PredefinedMenuItem::separator();
    let separator_b = PredefinedMenuItem::separator();
    let menu = Menu::with_items(&[
        &app_info,
        &show,
        &settings,
        &separator_a,
        &now_playing,
        &play_pause,
        &next,
        &previous,
        &separator_b,
        &quit,
    ])
    .map_err(|error| format!("Failed to build tray menu: {error}"))?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Stereodrome")
        .with_icon(tray_icon()?)
        .with_icon_as_template(true)
        .build()
        .map_err(|error| format!("Failed to create tray icon: {error}"))?;
    Ok(TrayParts {
        tray,
        app_info,
        now_playing,
        play_pause,
        next,
        previous,
    })
}

#[cfg(not(target_os = "linux"))]
fn menu_event_loop(stop: Arc<AtomicBool>, sender: async_channel::Sender<TrayAction>) {
    while !stop.load(Ordering::Acquire) {
        if let Ok(event) = MenuEvent::receiver().recv_timeout(Duration::from_millis(100))
            && let Some(action) = action_for_menu_id(event.id.as_ref())
        {
            let _ = sender.try_send(action);
        }
    }
}

fn action_for_menu_id(id: &str) -> Option<TrayAction> {
    match id {
        MENU_SHOW => Some(TrayAction::Show),
        MENU_SETTINGS => Some(TrayAction::Settings),
        MENU_PLAY_PAUSE => Some(TrayAction::TogglePlayback),
        MENU_NEXT => Some(TrayAction::Next),
        MENU_PREVIOUS => Some(TrayAction::Previous),
        MENU_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

fn tray_icon() -> Result<Icon, String> {
    const SIDE: usize = 16;
    let mut rgba = vec![0; SIDE * SIDE * 4];
    for y in 2..14 {
        for x in 3..13 {
            let note = (x == 9 || x == 10) && y < 11
                || (y == 3 || y == 4) && (5..=10).contains(&x)
                || (7..=10).contains(&x) && (10..=13).contains(&y);
            if note {
                let offset = (y * SIDE + x) * 4;
                rgba[offset..offset + 4].copy_from_slice(&[30, 30, 30, 255]);
            }
        }
    }
    Icon::from_rgba(rgba, SIDE as u32, SIDE as u32)
        .map_err(|error| format!("Failed to create tray icon pixels: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{TrayAction, action_for_menu_id, tray_icon};

    #[test]
    fn tray_actions_are_closed_and_icon_is_valid() {
        assert_eq!(action_for_menu_id("settings"), Some(TrayAction::Settings));
        assert_eq!(action_for_menu_id("unknown"), None);
        tray_icon().unwrap();
    }
}
