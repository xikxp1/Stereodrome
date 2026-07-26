mod audio;
mod cache;
mod commands;
mod credentials;
mod db;
mod error;
mod media;
mod runtime;
mod state;
mod tray;

use std::sync::Arc;

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
#[allow(clippy::exit, clippy::too_many_lines)]
/// Starts the desktop application and blocks until it exits.
///
/// # Panics
///
/// Panics if the Tauri application cannot be built or run.
pub fn run() {
    let builder = tauri::Builder::default();

    let builder = builder
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
                .level_for("stereodrome_audio", LevelFilter::Debug)
                .level_for("stereodrome_core", LevelFilter::Debug)
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
        .setup(move |app| {
            let db_path = db::get_db_path(app.handle())?;
            let data_dir = std::path::Path::new(&db_path)
                .parent()
                .ok_or_else(|| std::io::Error::other("database path has no parent"))?;

            if cache::current_cache_root(app.handle())? != cache::default_cache_root(app.handle())?
            {
                info!("Migrating desktop cache back to the shared runtime data directory");
                cache::set_cache_root(app.handle(), None)?;
            }

            let app_state = AppState::new(data_dir)?;
            commands::settings::apply_desktop_runtime_settings(app.handle(), &app_state)?;
            commands::cache::migrate_desktop_cache_settings(app.handle(), &app_state)?;

            // Migrate the legacy desktop volume store into runtime persistence.
            if let Some(persisted_volume) =
                commands::ui_state::take_legacy_persisted_volume(app.handle())
                && let Err(e) = runtime::dispatch::<()>(
                    &app_state,
                    stereodrome_core::CoreCommand::SetPlaybackVolume {
                        volume: persisted_volume,
                    },
                )
            {
                warn!("Failed to apply persisted runtime volume: {e}");
            }

            let desktop_runtime = app_state.runtime.clone();
            let runtime_audio = Arc::clone(&app_state.runtime_audio);
            let emitter_running = Arc::clone(&app_state.emitter_running);
            audio::player::start_position_emitter(
                Arc::clone(&runtime_audio),
                app.handle().clone(),
                Arc::clone(&emitter_running),
            );
            audio::player::start_spectrum_emitter(
                runtime_audio,
                app.handle().clone(),
                Arc::clone(&emitter_running),
            );

            // Project the server's now-playing list through the runtime.
            let emitter_running = Arc::clone(&app_state.emitter_running);
            commands::nowplaying::start_now_playing_emitter(
                app.handle().clone(),
                desktop_runtime.clone(),
                emitter_running,
            );

            app.manage(app_state);

            commands::library::start_library_sync_scheduler(app.handle());

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

            runtime::start_event_bridge(app.handle().clone(), desktop_runtime);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::core_dispatch,
            commands::connect_server,
            commands::disconnect_server,
            commands::restore_session,
            commands::import_portable_backup,
            commands::sync_library,
            commands::reconcile_library_state,
            commands::get_album_count,
            commands::get_playback_status,
            commands::sync_playlists,
            commands::get_playlist_songs,
            commands::scrobble_now_playing,
            commands::scrobble_submit,
            commands::get_cover_art,
            commands::get_cover_art_path,
            commands::get_song_cover_art,
            commands::get_cache_locations,
            commands::set_cache_root,
            commands::get_downloading_song_ids,
            commands::set_tray_update_available,
            commands::get_normalization_settings,
            commands::set_normalization_settings,
            commands::get_normalization_stats,
            commands::get_analysis_progress,
            commands::analyze_all_songs,
            commands::clear_normalization_data,
            commands::get_playback_settings,
            commands::set_playback_settings,
            commands::get_connectivity_settings,
            commands::set_connectivity_settings,
            commands::set_persisted_volume,
            commands::get_mini_player_position,
            commands::set_mini_player_position,
            commands::get_notification_settings,
            commands::set_notification_settings,
            commands::send_now_playing_notification,
            commands::get_sync_settings,
            commands::set_sync_settings,
            commands::get_system_time_preferences,
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
            } if label == "main" => {
                // Hide window instead of closing (minimize to tray)
                api.prevent_close();
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.hide();
                    info!("Window hidden to tray");
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    state
                        .emitter_running
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    info!("Shutting down desktop runtime");
                    state.runtime.shutdown();
                }
            }
            _ => {}
        });
}
