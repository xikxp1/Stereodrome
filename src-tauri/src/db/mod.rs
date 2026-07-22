use crate::error::AppResult;

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
