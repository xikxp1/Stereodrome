use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
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
        let base64 = base64_encode(&bytes);
        let mime = guess_mime_type(&bytes);
        return Ok(format!("data:{};base64,{}", mime, base64));
    }

    // Get submarine client
    let client = state
        .client
        .lock()
        .unwrap()
        .clone()
        .ok_or(AppError::NotConnected)?;

    // Fetch cover art using submarine client
    let bytes_vec = client
        .get_cover_art(&cover_art_id, size)
        .await
        .map_err(|e| AppError::Subsonic(format!("Failed to fetch cover art: {}", e)))?;

    // Cache the image
    if let Err(e) = fs::write(&cache_path, &bytes_vec) {
        eprintln!("Failed to cache cover art: {}", e);
    }

    let base64 = base64_encode(&bytes_vec);
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
        let conn = state.db.lock().unwrap();
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

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;

        result.push(ALPHABET[b0 >> 2] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if chunk.len() > 1 {
            result.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(ALPHABET[b2 & 0x3f] as char);
        } else {
            result.push('=');
        }
    }

    result
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
