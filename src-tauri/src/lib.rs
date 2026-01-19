mod commands;
mod db;
mod error;
mod state;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let db_path = db::get_db_path(app.handle())?;
            let app_state = AppState::new(&db_path)?;

            // Initialize database schema
            {
                let conn = app_state.db.lock().unwrap();
                db::init_db(&conn)?;
            }

            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::connect_server,
            commands::disconnect_server,
            commands::get_connection_status,
            commands::sync_library,
            commands::get_artists,
            commands::get_albums,
            commands::get_songs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
