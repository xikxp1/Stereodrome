use std::fs;
use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD, Engine};
use log::warn;
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult, MutexExt};
use crate::state::AppState;

/// Get the cover art cache directory path
fn get_cache_dir(app_handle: &AppHandle) -> AppResult<PathBuf> {
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, e)))?;
    let cache_dir = data_dir.join("cover_cache");
    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

/// Get cover art as base64 data URL
/// First checks local cache, then fetches from server using submarine client
#[tauri::command]
pub async fn get_cover_art(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    cover_art_id: String,
    size: Option<i32>,
) -> AppResult<String> {
    if cover_art_id.is_empty() {
        return Err(AppError::Subsonic("Empty cover art ID".to_string()));
    }

    let cache_dir = get_cache_dir(&app_handle)?;

    // Create cache filename based on cover_art_id and size
    let cache_filename = match size {
        Some(s) => format!("{}_{}.jpg", cover_art_id.replace(['/', '\\'], "_"), s),
        None => format!("{}.jpg", cover_art_id.replace(['/', '\\'], "_")),
    };
    let cache_path = cache_dir.join(&cache_filename);

    // Check cache first
    if cache_path.exists() {
        let bytes = fs::read(&cache_path)?;
        let base64 = STANDARD.encode(&bytes);
        let mime = guess_mime_type(&bytes);
        return Ok(format!("data:{};base64,{}", mime, base64));
    }

    // Check if connected
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    // Fetch cover art using client handle
    let bytes_vec = state
        .client
        .get_cover_art(&cover_art_id, size)
        .await
        .map_err(|e| AppError::Subsonic(format!("Failed to fetch cover art: {}", e)))?;

    // Cache the image
    if let Err(e) = fs::write(&cache_path, &bytes_vec) {
        warn!("Failed to cache cover art: {}", e);
    }

    let base64 = STANDARD.encode(&bytes_vec);
    let mime = guess_mime_type(&bytes_vec);
    Ok(format!("data:{};base64,{}", mime, base64))
}

/// Get cover art for a song by its ID (looks up album's cover_art_id)
#[tauri::command]
pub async fn get_song_cover_art(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    song_id: String,
    size: Option<i32>,
) -> AppResult<Option<String>> {
    // Look up the album's cover_art_id for this song
    let cover_art_id: Option<String> = {
        let conn = state.db.lock_recover();
        conn.query_row(
            "SELECT al.cover_art_id FROM songs s
             JOIN albums al ON s.album_id = al.id
             WHERE s.id = ?",
            [&song_id],
            |row| row.get(0),
        )
        .ok()
        .flatten()
    };

    match cover_art_id {
        Some(id) if !id.is_empty() => {
            let data_url = get_cover_art(app_handle, state, id, size).await?;
            Ok(Some(data_url))
        }
        _ => Ok(None),
    }
}

/// Get cover art file path (for notifications with attachments)
/// Returns the local file path if cached, otherwise fetches and caches first
#[tauri::command]
pub async fn get_cover_art_path(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    cover_art_id: String,
    size: Option<i32>,
) -> AppResult<String> {
    if cover_art_id.is_empty() {
        return Err(AppError::Subsonic("Empty cover art ID".to_string()));
    }

    let cache_dir = get_cache_dir(&app_handle)?;

    // Create cache filename based on cover_art_id and size
    let cache_filename = match size {
        Some(s) => format!("{}_{}.jpg", cover_art_id.replace(['/', '\\'], "_"), s),
        None => format!("{}.jpg", cover_art_id.replace(['/', '\\'], "_")),
    };
    let cache_path = cache_dir.join(&cache_filename);

    // Check cache first
    if cache_path.exists() {
        return Ok(cache_path.to_string_lossy().to_string());
    }

    // Check if connected
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    // Fetch cover art using client handle
    let bytes_vec = state
        .client
        .get_cover_art(&cover_art_id, size)
        .await
        .map_err(|e| AppError::Subsonic(format!("Failed to fetch cover art: {}", e)))?;

    // Cache the image
    fs::write(&cache_path, &bytes_vec)?;

    Ok(cache_path.to_string_lossy().to_string())
}

fn guess_mime_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.starts_with(b"GIF") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "image/jpeg" // Default to JPEG
    }
}
