use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use stereodrome_desktop::DesktopBackend;
use tauri::{AppHandle, Emitter, Manager};

use crate::media::MediaControlsManager;
use crate::tray::TrayManager;

pub fn start_spectrum_emitter(
    backend: &DesktopBackend,
    app_handle: AppHandle,
    running: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    let mut receiver = backend.subscribe_spectrum();

    thread::spawn(move || {
        while running.load(Ordering::Acquire) {
            thread::park_timeout(Duration::from_millis(33));
            if !running.load(Ordering::Acquire) {
                break;
            }
            if receiver.has_changed().unwrap_or(false) {
                let spectrum = receiver.borrow_and_update().clone();
                let _ = app_handle.emit("spectrum-data", spectrum);
            }
        }
    })
}

pub fn start_position_emitter(
    backend: &DesktopBackend,
    app_handle: AppHandle,
    running: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    let mut receiver = backend.subscribe_playback();

    thread::spawn(move || {
        let mut last_song_id: Option<String> = None;
        let mut last_is_playing = false;
        let mut media_position_counter: u8 = 0;

        while running.load(Ordering::Acquire) {
            thread::park_timeout(Duration::from_millis(100));
            if !running.load(Ordering::Acquire) {
                break;
            }
            if !receiver.has_changed().unwrap_or(false) {
                continue;
            }

            let state = receiver.borrow_and_update().clone();
            if !state.is_playing && state.song.is_none() {
                if last_song_id.is_some() {
                    clear_shell_playback_state(&app_handle);
                    last_song_id = None;
                    last_is_playing = false;
                }
                let _ = app_handle.emit("playback-state", &state);
                continue;
            }

            let current_song_id = state.song.as_ref().map(|song| song.id.clone());
            if current_song_id != last_song_id {
                if let Some(song) = &state.song {
                    if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>() {
                        let cover_art_path = song.cover_art_id.as_ref().and_then(|id| {
                            let cache_dir = crate::cache::cover_cache_dir(&app_handle).ok()?;
                            let sanitized_id = id.replace(['/', '\\'], "_");

                            for size in [800, 128, 64] {
                                let path = cache_dir.join(format!("{sanitized_id}_{size}.jpg"));
                                if path.exists() {
                                    return Some(path.to_string_lossy().to_string());
                                }
                            }

                            let path = cache_dir.join(format!("{sanitized_id}.jpg"));
                            path.exists().then(|| path.to_string_lossy().to_string())
                        });
                        media_controls.update_metadata(song, state.duration, cover_art_path);
                    }

                    if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
                        tray_manager.update_song_info(&song.title, &song.artist);
                    }
                }
                last_song_id = current_song_id;
            }

            media_position_counter = (media_position_counter + 1) % 10;
            if state.is_playing != last_is_playing || media_position_counter == 0 {
                if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>() {
                    media_controls.set_playback_status(state.is_playing, state.position);
                }
                if state.is_playing != last_is_playing
                    && let Some(tray_manager) = app_handle.try_state::<TrayManager>()
                {
                    tray_manager.update_playback_state(state.is_playing);
                }
                last_is_playing = state.is_playing;
            }

            let _ = app_handle.emit("playback-state", &state);
        }
    })
}

pub(crate) fn clear_shell_playback_state(app_handle: &AppHandle) {
    if let Some(media_controls) = app_handle.try_state::<MediaControlsManager>() {
        media_controls.clear();
    }
    if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
        tray_manager.update_song_info("", "");
        tray_manager.update_playback_state(false);
    }
}
