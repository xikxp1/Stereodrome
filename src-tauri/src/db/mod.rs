use rusqlite::Connection;

use crate::error::AppResult;

const SCHEMA: &str = include_str!("schema.sql");

pub fn init_db(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

pub fn get_db_path(app_handle: &tauri::AppHandle) -> AppResult<String> {
    let app_dir = app_handle.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            e.to_string(),
        ))
    })?;

    std::fs::create_dir_all(&app_dir)?;

    let db_path = app_dir.join("stereodrome.db");
    Ok(db_path.to_string_lossy().to_string())
}

use tauri::Manager;
