use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::runtime::AppHandle;
use base64::{Engine, engine::general_purpose::STANDARD};
use log::warn;
use tauri::State;

use crate::cache::cover_cache_dir;
use crate::client::SubsonicClientHandle;
use crate::error::{AppError, AppResult, MutexExt};
use crate::state::AppState;

pub(crate) const PRESERVED_COVER_ART_SIZE: i32 = 800;
const FALLBACK_COVER_ART_SIZES: [i32; 5] = [PRESERVED_COVER_ART_SIZE, 200, 128, 96, 64];

/// Get the cover art cache directory path
fn get_cache_dir(app_handle: &AppHandle) -> AppResult<PathBuf> {
    cover_cache_dir(app_handle)
}

fn sanitize_cover_art_id(cover_art_id: &str) -> String {
    cover_art_id.replace(['/', '\\'], "_")
}

fn cache_filename(cover_art_id: &str, size: Option<i32>) -> String {
    let safe_id = sanitize_cover_art_id(cover_art_id);
    match size {
        Some(size) => format!("{safe_id}_{size}.jpg"),
        None => format!("{safe_id}.jpg"),
    }
}

fn cache_path(cache_dir: &Path, cover_art_id: &str, size: Option<i32>) -> PathBuf {
    cache_dir.join(cache_filename(cover_art_id, size))
}

fn fallback_cache_paths(cache_dir: &Path, cover_art_id: &str, size: Option<i32>) -> Vec<PathBuf> {
    let mut sizes = Vec::new();
    push_unique_size(&mut sizes, size);
    push_unique_size(&mut sizes, Some(PRESERVED_COVER_ART_SIZE));
    for fallback_size in FALLBACK_COVER_ART_SIZES {
        push_unique_size(&mut sizes, Some(fallback_size));
    }
    push_unique_size(&mut sizes, None);

    sizes
        .into_iter()
        .map(|candidate_size| cache_path(cache_dir, cover_art_id, candidate_size))
        .collect()
}

fn push_unique_size(sizes: &mut Vec<Option<i32>>, size: Option<i32>) {
    if !sizes.contains(&size) {
        sizes.push(size);
    }
}

fn cached_cover_art_path(
    cache_dir: &Path,
    cover_art_id: &str,
    size: Option<i32>,
) -> Option<PathBuf> {
    let fallback_paths = fallback_cache_paths(cache_dir, cover_art_id, size);
    for path in &fallback_paths {
        if path.exists() {
            return Some(path.clone());
        }
    }

    let known_paths = fallback_paths.into_iter().collect::<HashSet<_>>();
    let sanitized_id = sanitize_cover_art_id(cover_art_id);
    let mut discovered_paths = fs::read_dir(cache_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| !known_paths.contains(path))
        .filter(|path| {
            path.file_name()
                .and_then(|file_name| file_name.to_str())
                .is_some_and(|file_name| cover_art_filename_matches(file_name, &sanitized_id))
        })
        .collect::<Vec<_>>();
    discovered_paths.sort();
    discovered_paths.into_iter().next()
}

fn cover_art_filename_matches(file_name: &str, sanitized_id: &str) -> bool {
    let Some(stem) = file_name.strip_suffix(".jpg") else {
        return false;
    };

    if stem == sanitized_id {
        return true;
    }

    stem.strip_prefix(&format!("{sanitized_id}_"))
        .is_some_and(|size| !size.is_empty() && size.chars().all(|c| c.is_ascii_digit()))
}

fn cached_cover_art_bytes(
    cache_dir: &Path,
    cover_art_id: &str,
    size: Option<i32>,
) -> AppResult<Option<Vec<u8>>> {
    if let Some(path) = cached_cover_art_path(cache_dir, cover_art_id, size) {
        return Ok(Some(fs::read(path)?));
    }
    Ok(None)
}

async fn fetch_cover_art_bytes(
    client: &SubsonicClientHandle,
    cover_art_id: &str,
    size: Option<i32>,
) -> AppResult<Vec<u8>> {
    client
        .get_cover_art(cover_art_id, size)
        .await
        .map_err(|e| AppError::Subsonic(format!("Failed to fetch cover art: {}", e)))
}

pub(crate) async fn preserve_cover_art_for_offline(
    app_handle: &AppHandle,
    client: &SubsonicClientHandle,
    cover_art_id: &str,
) -> AppResult<()> {
    if cover_art_id.is_empty() {
        return Ok(());
    }

    let cache_dir = get_cache_dir(app_handle)?;
    let cache_path = cache_path(&cache_dir, cover_art_id, Some(PRESERVED_COVER_ART_SIZE));
    if cache_path.exists() {
        return Ok(());
    }

    if crate::commands::settings::manual_offline_enabled(app_handle) {
        return Err(AppError::OfflineMode);
    }

    if !client.is_connected() {
        return Err(AppError::NotConnected);
    }

    let bytes = fetch_cover_art_bytes(client, cover_art_id, Some(PRESERVED_COVER_ART_SIZE)).await?;
    fs::write(cache_path, bytes)?;
    Ok(())
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

    if let Some(bytes) = cached_cover_art_bytes(&cache_dir, &cover_art_id, size)? {
        let base64 = STANDARD.encode(&bytes);
        let mime = guess_mime_type(&bytes);
        return Ok(format!("data:{};base64,{}", mime, base64));
    }

    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(AppError::OfflineMode);
    }

    // Check if connected
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    let bytes_vec = fetch_cover_art_bytes(&state.client, &cover_art_id, size).await?;

    // Cache the image
    let cache_path = cache_path(&cache_dir, &cover_art_id, size);
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

    if let Some(cache_path) = cached_cover_art_path(&cache_dir, &cover_art_id, size) {
        return Ok(cache_path.to_string_lossy().to_string());
    }

    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(AppError::OfflineMode);
    }

    // Check if connected
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    let bytes_vec = fetch_cover_art_bytes(&state.client, &cover_art_id, size).await?;

    // Cache the image
    let cache_path = cache_path(&cache_dir, &cover_art_id, size);
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

#[cfg(test)]
mod tests {
    use super::{
        PRESERVED_COVER_ART_SIZE, cache_filename, cover_art_filename_matches, fallback_cache_paths,
    };
    use std::path::Path;

    #[test]
    fn cache_filename_sanitizes_cover_art_ids() {
        assert_eq!(
            cache_filename("album/cover\\id", Some(64)),
            "album_cover_id_64.jpg"
        );
        assert_eq!(
            cache_filename("album/cover\\id", None),
            "album_cover_id.jpg"
        );
    }

    #[test]
    fn fallback_cache_paths_prefers_exact_size_then_preserved_size() {
        let paths = fallback_cache_paths(Path::new("/cache"), "cover", Some(96));

        assert_eq!(paths[0], Path::new("/cache").join("cover_96.jpg"));
        assert_eq!(
            paths[1],
            Path::new("/cache").join(format!("cover_{PRESERVED_COVER_ART_SIZE}.jpg"))
        );
        assert_eq!(
            paths.last().unwrap(),
            &Path::new("/cache").join("cover.jpg")
        );
    }

    #[test]
    fn fallback_cache_paths_deduplicates_preserved_exact_size() {
        let paths =
            fallback_cache_paths(Path::new("/cache"), "cover", Some(PRESERVED_COVER_ART_SIZE));

        assert_eq!(
            paths
                .iter()
                .filter(|path| path.ends_with("cover_800.jpg"))
                .count(),
            1
        );
        assert_eq!(paths[0], Path::new("/cache").join("cover_800.jpg"));
    }

    #[test]
    fn cover_art_filename_matches_only_same_cover_id() {
        assert!(cover_art_filename_matches("album_64.jpg", "album"));
        assert!(cover_art_filename_matches("album.jpg", "album"));
        assert!(!cover_art_filename_matches("album_large.jpg", "album"));
        assert!(!cover_art_filename_matches("album-extra_64.jpg", "album"));
        assert!(!cover_art_filename_matches("album_64.png", "album"));
    }
}
