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

    // Ensure sync_state exists for incremental sync metadata.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sync_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sync_state_updated_at ON sync_state(updated_at);",
    )?;

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

/// Save normalization analysis result to the database.
/// Used by playback (background analysis), prefetch, and batch analysis.
pub fn save_normalization_result(
    db_path: &std::path::Path,
    song_id: &str,
    album_id: &str,
    integrated_lufs: f64,
    true_peak: f64,
) -> Result<(), rusqlite::Error> {
    let conn = rusqlite::Connection::open(db_path)?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO normalization_data
         (song_id, track_loudness_lufs, track_peak, album_id, source, analyzed_at)
         VALUES (?1, ?2, ?3, ?4, 'ebur128', ?5)",
        rusqlite::params![song_id, integrated_lufs, true_peak, album_id, now],
    )?;
    Ok(())
}
