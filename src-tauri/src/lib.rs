mod audio;
mod cache;
mod commands;
mod db;
mod error;
mod search;
mod state;

use std::sync::Arc;

use error::MutexExt;
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let db_path = db::get_db_path(app.handle())?;
            let index_path = search::get_index_path(app.handle())?;
            let app_state = AppState::new(&db_path, index_path)?;

            // Initialize database schema
            {
                let conn = app_state.db.lock_recover();
                db::init_db(&conn)?;
            }

            // Start position emitter for audio playback
            {
                let audio_player = app_state.audio_player.lock_recover();
                audio_player.start_position_emitter(app.handle().clone());
                audio_player.start_spectrum_emitter(app.handle().clone());
            }

            // Start now playing emitter
            let emitter_running = Arc::clone(&app_state.emitter_running);

            // We need a way to get the client that doesn't hold a reference to app_state
            // So we'll store a weak reference to check for client changes
            let app_handle = app.handle().clone();
            commands::nowplaying::start_now_playing_emitter(
                app_handle.clone(),
                move || {
                    // Get the app state from the handle
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        state.get_client()
                    } else {
                        None
                    }
                },
                emitter_running,
            );

            app.manage(app_state);
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
            commands::remove_from_queue,
            commands::clear_queue,
            commands::move_queue_item,
            commands::play_queue_item,
            commands::play_next,
            commands::play_previous,
            commands::toggle_shuffle,
            commands::set_repeat_mode,
            commands::cycle_repeat_mode,
            commands::get_playlists,
            commands::get_playlist_songs,
            commands::create_playlist,
            commands::update_playlist,
            commands::delete_playlist,
            commands::add_songs_to_playlist,
            commands::remove_song_from_playlist,
            commands::reorder_playlist,
            commands::search_library,
            commands::scrobble_now_playing,
            commands::scrobble_submit,
            commands::get_cover_art,
            commands::get_cover_art_path,
            commands::get_song_cover_art,
            commands::get_audio_cache_stats,
            commands::clear_audio_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
