pub mod queue;

use log::warn;
use rusqlite::Connection;

use crate::error::AppResult;

const SCHEMA: &str = include_str!("schema.sql");

pub fn init_db(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(SCHEMA)?;
    run_migrations(conn)?;
    Ok(())
}

fn run_migrations(conn: &Connection) -> AppResult<()> {
    // Check if songs table has all required columns
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(songs)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .collect();

    let required_columns = [
        "id",
        "album_id",
        "artist_id",
        "title",
        "track_number",
        "disc_number",
        "duration",
        "bit_rate",
        "size",
        "suffix",
        "content_type",
        "path",
        "year",
        "genre",
        "synced_at",
    ];

    let missing: Vec<_> = required_columns
        .iter()
        .filter(|c| !columns.contains(&c.to_string()))
        .collect();

    // If critical columns are missing, drop and recreate tables
    // Data will be re-synced from server
    if !missing.is_empty() {
        warn!(
            "Songs table missing columns {:?}, recreating tables",
            missing
        );
        conn.execute_batch(
            "DROP TABLE IF EXISTS playlist_songs;
             DROP TABLE IF EXISTS playlists;
             DROP TABLE IF EXISTS songs;
             DROP TABLE IF EXISTS albums;
             DROP TABLE IF EXISTS artists;",
        )?;
        conn.execute_batch(SCHEMA)?;
    }

    // Check if playlists table has all required columns (owner, cover_art_id added for server sync)
    let playlist_columns: Vec<String> = conn
        .prepare("PRAGMA table_info(playlists)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .collect();

    let required_playlist_columns = ["owner", "cover_art_id"];
    let missing_playlist: Vec<_> = required_playlist_columns
        .iter()
        .filter(|c| !playlist_columns.contains(&c.to_string()))
        .collect();

    if !missing_playlist.is_empty() {
        warn!(
            "Playlists table missing columns {:?}, recreating playlist tables",
            missing_playlist
        );
        conn.execute_batch(
            "DROP TABLE IF EXISTS playlist_songs;
             DROP TABLE IF EXISTS playlists;",
        )?;
        conn.execute_batch(SCHEMA)?;
    }

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
