mod audio;
mod cache;
mod client;
mod commands;
mod credentials;
mod db;
mod error;
mod media;
mod search;
mod state;
mod tray;

use std::sync::Arc;

use error::MutexExt as _;
use log::{LevelFilter, info, warn};
use media::MediaControlsManager;
use state::AppState;
use tauri::{AppHandle, Manager};
use tauri_plugin_log::{Target, TargetKind};
use tray::TrayManager;

/// Focus and show the main window when a second instance tries to open
fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // Unminimize if minimized
        let _ = window.unminimize();
        // Show if hidden
        let _ = window.show();
        // Bring to front and focus
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                    Target::new(TargetKind::Webview),
                ])
                .level(LevelFilter::Info)
                .level_for("stereodrome", LevelFilter::Debug)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            info!("Second instance detected, focusing existing window");
            focus_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_prevent_default::debug());

    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .setup(|app| {
            let db_path = db::get_db_path(app.handle())?;
            let index_path = search::get_index_path(app.handle())?;

            // Spawn the submarine client thread
            let client_handle = client::spawn();

            let app_state = AppState::new(&db_path, index_path, client_handle.clone())?;

            // Initialize database schema
            {
                let conn = app_state.db.lock_recover();
                db::init_db(&conn)?;
            }

            // Restore persisted runtime volume before UI starts consuming playback state.
            {
                let persisted_volume = commands::read_persisted_volume(app.handle());
                let audio_player = app_state.audio_player.lock_recover();
                if let Err(e) = audio_player.set_volume(persisted_volume) {
                    warn!("Failed to apply persisted runtime volume: {e}");
                }
            }

            // Start position emitter for audio playback
            {
                let audio_player = app_state.audio_player.lock_recover();
                audio_player.start_position_emitter(app.handle().clone());
                audio_player.start_spectrum_emitter(app.handle().clone());
            }

            // Start now playing emitter with the client handle
            let emitter_running = Arc::clone(&app_state.emitter_running);
            commands::nowplaying::start_now_playing_emitter(
                app.handle().clone(),
                client_handle,
                emitter_running,
            );

            app.manage(app_state);

            // Initialize media controls for OS integration (Control Center, media keys)
            if let Some(media_controls) = MediaControlsManager::new(app.handle().clone()) {
                info!("Media controls initialized");
                app.manage(media_controls);
            } else {
                info!("Media controls not available on this platform");
            }

            // Initialize system tray icon
            if let Some(tray_manager) = TrayManager::new(app) {
                info!("Tray icon initialized");
                app.manage(tray_manager);
            } else {
                info!("Tray icon not available on this platform");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::connect_server,
            commands::disconnect_server,
            commands::get_connection_status,
            commands::restore_session,
            commands::sync_library,
            commands::get_artists,
            commands::get_albums,
            commands::get_songs,
            commands::play_song,
            commands::pause_playback,
            commands::resume_playback,
            commands::stop_playback,
            commands::set_volume,
            commands::seek_playback,
            commands::get_playback_status,
            commands::get_queue,
            commands::add_to_queue,
            commands::add_songs_to_queue,
            commands::insert_next_in_queue,
            commands::insert_next_songs_in_queue,
            commands::remove_from_queue,
            commands::clear_queue,
            commands::move_queue_item,
            commands::play_queue_item,
            commands::play_next,
            commands::play_previous,
            commands::toggle_shuffle,
            commands::set_repeat_mode,
            commands::cycle_repeat_mode,
            commands::sync_playlists,
            commands::get_playlists,
            commands::get_playlist_songs,
            commands::create_playlist,
            commands::update_playlist,
            commands::delete_playlist,
            commands::add_songs_to_playlist,
            commands::remove_song_from_playlist,
            commands::search_library,
            commands::scrobble_now_playing,
            commands::scrobble_submit,
            commands::get_cover_art,
            commands::get_cover_art_path,
            commands::get_song_cover_art,
            commands::get_audio_cache_stats,
            commands::clear_audio_cache,
            commands::set_max_cache_size,
            commands::get_scan_status,
            commands::start_scan,
            commands::set_tray_update_available,
            commands::get_normalization_settings,
            commands::set_normalization_settings,
            commands::get_normalization_stats,
            commands::get_analysis_progress,
            commands::analyze_all_songs,
            commands::clear_normalization_data,
            commands::get_playback_settings,
            commands::set_playback_settings,
            commands::set_persisted_volume,
            commands::get_mini_player_position,
            commands::set_mini_player_position,
            commands::get_notification_settings,
            commands::set_notification_settings,
            commands::open_mini_player,
            commands::set_mini_player_mode,
            commands::close_mini_player,
            commands::restore_main_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } => {
                if label == "main" {
                    // Hide window instead of closing (minimize to tray)
                    api.prevent_close();
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.hide();
                        info!("Window hidden to tray");
                    }
                }
            }
            tauri::RunEvent::Exit => {
                // Shutdown the client thread gracefully
                if let Some(state) = app_handle.try_state::<AppState>() {
                    info!("Shutting down client thread");
                    state.client.shutdown();
                }
            }
            _ => {}
        });
}
