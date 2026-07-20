use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::SystemTime;

use filetime::FileTime;
use log::{debug, warn};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;
use tokio::sync::Mutex as TokioMutex;

use crate::audio::loudness;
use crate::client::SubsonicClientHandle;
use crate::db;
use crate::error::{AppError, AppResult};

/// Default maximum cache size: 5 GB
pub const DEFAULT_MAX_CACHE_SIZE: u64 = 5 * 1024 * 1024 * 1024;
/// Minimum cache size: 500 MB
pub const MIN_CACHE_SIZE: u64 = 500 * 1024 * 1024;
/// Maximum cache size: 50 GB
pub const MAX_CACHE_SIZE: u64 = 50 * 1024 * 1024 * 1024;
pub const STORE_FILE: &str = "settings.json";
pub const KEY_MAX_CACHE_SIZE: &str = "max_cache_size";
pub const AUDIO_CACHE_CHANGED_EVENT: &str = "audio-cache-changed";

#[derive(Debug, Clone, Serialize)]
pub struct AudioCacheChangedEvent {
    pub reason: &'static str,
}

pub(crate) fn emit_audio_cache_changed(app_handle: &AppHandle, reason: &'static str) {
    let _ = app_handle.emit(AUDIO_CACHE_CHANGED_EVENT, AudioCacheChangedEvent { reason });
}

/// Read the configured max cache size from settings store
fn read_max_cache_size(app_handle: &AppHandle) -> u64 {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Some(value) = store.get(KEY_MAX_CACHE_SIZE)
        && let Some(size) = value.as_u64()
    {
        return size.clamp(MIN_CACHE_SIZE, MAX_CACHE_SIZE);
    }
    DEFAULT_MAX_CACHE_SIZE
}

/// Statistics about the audio cache
#[derive(Debug, Clone, Serialize)]
pub struct CacheStats {
    /// Total size of cached files in bytes
    pub total_size: u64,
    /// Number of cached files
    pub file_count: u64,
    /// Maximum allowed cache size in bytes
    pub max_size: u64,
}

/// Tracks songs currently being prefetched to avoid duplicate requests
static PREFETCH_IN_PROGRESS: std::sync::LazyLock<
    Arc<TokioMutex<std::collections::HashSet<String>>>,
> = std::sync::LazyLock::new(|| Arc::new(TokioMutex::new(std::collections::HashSet::new())));
static DOWNLOADS_IN_PROGRESS: std::sync::LazyLock<StdMutex<HashMap<String, usize>>> =
    std::sync::LazyLock::new(|| StdMutex::new(HashMap::new()));
static NEXT_CACHE_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

fn create_cache_temp_file(destination: &Path) -> io::Result<(PathBuf, File)> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "cache destination has no parent directory",
        )
    })?;
    let file_name = destination.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "cache destination has no file name",
        )
    })?;

    loop {
        let id = NEXT_CACHE_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let temp_name = format!(
            ".{}.{}.{}.part",
            file_name.to_string_lossy(),
            std::process::id(),
            id
        );
        let temp_path = parent.join(temp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

fn write_cache_file_atomically(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let (temp_path, mut temp_file) = create_cache_temp_file(destination)?;
    let write_result = temp_file
        .write_all(bytes)
        .and_then(|()| temp_file.sync_all());
    drop(temp_file);

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    if let Err(error) = fs::rename(&temp_path, destination) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(())
}

struct DownloadInProgressGuard {
    app_handle: AppHandle,
    song_id: String,
}

impl DownloadInProgressGuard {
    fn new(app_handle: &AppHandle, song_id: &str) -> Self {
        let should_emit = DOWNLOADS_IN_PROGRESS.lock().is_ok_and(|mut downloads| {
            let count = downloads.entry(song_id.to_string()).or_default();
            *count += 1;
            *count == 1
        });
        if should_emit {
            emit_audio_cache_changed(app_handle, "download_started");
        }
        Self {
            app_handle: app_handle.clone(),
            song_id: song_id.to_string(),
        }
    }
}

impl Drop for DownloadInProgressGuard {
    fn drop(&mut self) {
        let should_emit = DOWNLOADS_IN_PROGRESS.lock().is_ok_and(|mut downloads| {
            let Some(count) = downloads.get_mut(&self.song_id) else {
                return false;
            };
            *count -= 1;
            if *count == 0 {
                downloads.remove(&self.song_id);
                true
            } else {
                false
            }
        });
        if should_emit {
            emit_audio_cache_changed(&self.app_handle, "download_finished");
        }
    }
}

pub fn downloading_song_ids() -> Vec<String> {
    let mut song_ids = DOWNLOADS_IN_PROGRESS
        .lock()
        .map(|downloads| downloads.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    song_ids.sort_unstable();
    song_ids
}

/// Audio file cache with LRU eviction
pub struct AudioCache {
    app_handle: AppHandle,
    cache_dir: PathBuf,
    max_size: u64,
}

impl AudioCache {
    /// Create a new `AudioCache` instance, reading max size from settings
    pub fn new(app_handle: &AppHandle) -> AppResult<Self> {
        let max_size = read_max_cache_size(app_handle);
        let cache_dir = crate::cache::audio_cache_dir(app_handle)?;
        Ok(Self {
            app_handle: app_handle.clone(),
            cache_dir,
            max_size,
        })
    }

    pub(crate) fn emit_changed(&self, reason: &'static str) {
        emit_audio_cache_changed(&self.app_handle, reason);
    }

    /// Get the cache file path for a song
    pub fn get_cache_path(&self, song_id: &str, suffix: &str) -> PathBuf {
        // Sanitize song_id for filesystem (replace problematic characters)
        let safe_id = song_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let filename = if suffix.is_empty() {
            safe_id
        } else {
            format!("{safe_id}.{suffix}")
        };
        self.cache_dir.join(filename)
    }

    /// Check if a song is cached
    pub fn is_cached(&self, song_id: &str, suffix: &str) -> bool {
        self.get_cache_path(song_id, suffix).exists()
    }

    /// Get audio bytes, either from cache or by fetching from server
    pub async fn get_or_fetch(
        &self,
        client: &SubsonicClientHandle,
        song_id: &str,
        suffix: &str,
    ) -> AppResult<Vec<u8>> {
        let cache_path = self.get_cache_path(song_id, suffix);

        // Try to read from cache
        if cache_path.exists() {
            match fs::read(&cache_path) {
                Ok(bytes) => {
                    // Touch the file to update access time for LRU
                    self.touch_file(&cache_path);
                    return Ok(bytes);
                }
                Err(e) => {
                    // Log error but continue to fetch from server
                    warn!("Failed to read cached audio: {e}");
                }
            }
        }

        if crate::commands::settings::manual_offline_enabled(&self.app_handle) {
            return Err(AppError::OfflineMode);
        }

        let _download_guard = DownloadInProgressGuard::new(&self.app_handle, song_id);

        // Fetch from server via client handle
        let bytes = client
            .stream(song_id)
            .await
            .map_err(|e| AppError::Audio(format!("Failed to fetch audio: {e}")))?;

        // Publish the cache entry only after the complete file has reached disk.
        // Caching remains best-effort so a write failure does not stop playback.
        if let Err(e) = write_cache_file_atomically(&cache_path, &bytes) {
            warn!("Failed to cache audio: {e}");
        } else {
            // Enforce size limit after successful write
            if let Err(e) = self.enforce_size_limit() {
                warn!("Failed to enforce cache size limit: {e}");
            }
            self.emit_changed("updated");
        }

        Ok(bytes)
    }

    /// Update the access time of a file (for LRU tracking)
    #[allow(clippy::unused_self)]
    fn touch_file(&self, path: &Path) {
        // Use filetime to update access time without opening the file handle
        // This is much more efficient on Windows where file handle operations are expensive
        let now = FileTime::now();
        let _ = filetime::set_file_atime(path, now);
    }

    /// Enforce the maximum cache size by removing least recently accessed files
    pub fn enforce_size_limit(&self) -> AppResult<bool> {
        let mut entries = self.get_cache_entries()?;
        let protected_paths = self.protected_cache_paths()?;

        // Calculate total size
        let total_size: u64 = entries.iter().map(|e| e.size).sum();

        if total_size <= self.max_size {
            return Ok(false);
        }

        // Sort by access time (oldest first)
        entries.sort_by_key(|a| a.accessed);

        // Remove files until under the limit
        let mut current_size = total_size;
        let mut removed_any = false;
        for entry in entries {
            if current_size <= self.max_size {
                break;
            }
            if protected_paths.contains(&entry.path) {
                continue;
            }

            if let Err(e) = fs::remove_file(&entry.path) {
                warn!("Failed to remove cached file {}: {e}", entry.path.display());
            } else {
                current_size = current_size.saturating_sub(entry.size);
                removed_any = true;
            }
        }

        Ok(removed_any)
    }

    /// Get statistics about the cache
    pub fn get_stats(&self) -> AppResult<CacheStats> {
        let entries = self.get_cache_entries()?;
        let total_size: u64 = entries.iter().map(|e| e.size).sum();
        let file_count = entries.len() as u64;

        Ok(CacheStats {
            total_size,
            file_count,
            max_size: self.max_size,
        })
    }

    /// Clear all cached audio files
    pub fn clear(&self) -> AppResult<()> {
        let entries = self.get_cache_entries()?;
        let protected_paths = self.protected_cache_paths()?;
        let mut removed_any = false;
        for entry in entries {
            if protected_paths.contains(&entry.path) {
                continue;
            }
            if let Err(e) = fs::remove_file(&entry.path) {
                warn!("Failed to remove cached file {}: {e}", entry.path.display());
            } else {
                removed_any = true;
            }
        }
        if removed_any {
            self.emit_changed("cleared");
        }
        Ok(())
    }

    pub fn remove_if_unprotected(&self, song_id: &str, suffix: &str) -> AppResult<bool> {
        let path = self.get_cache_path(song_id, suffix);
        if self.protected_cache_paths()?.contains(&path) {
            return Ok(false);
        }
        match fs::remove_file(&path) {
            Ok(()) => {
                self.emit_changed("removed");
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// Get all cache entries with metadata
    fn get_cache_entries(&self) -> AppResult<Vec<CacheEntry>> {
        let mut entries = Vec::new();

        let read_dir = fs::read_dir(&self.cache_dir)?;
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Ok(metadata) = entry.metadata()
            {
                let size = metadata.len();
                // Use modified time as a fallback for accessed time
                // (some filesystems don't update atime)
                let accessed = metadata
                    .accessed()
                    .or_else(|_| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                entries.push(CacheEntry {
                    path,
                    size,
                    accessed,
                });
            }
        }

        Ok(entries)
    }

    fn protected_cache_paths(&self) -> AppResult<HashSet<PathBuf>> {
        let db_path = db::get_db_path(&self.app_handle)?;
        let conn = rusqlite::Connection::open(db_path)?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT s.id, COALESCE(s.suffix, '')
             FROM playlist_songs ps
             JOIN playlists p ON p.id = ps.playlist_id
             JOIN songs s ON s.id = ps.song_id
             WHERE p.offline_saved_at IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut paths = HashSet::new();
        for row in rows {
            let (song_id, suffix) = row?;
            paths.insert(self.get_cache_path(&song_id, &suffix));
        }
        Ok(paths)
    }

    /// Prefetch a song to cache in the background.
    /// Returns immediately - the fetch happens asynchronously.
    /// Skips if the song is already cached or currently being prefetched.
    /// If `album_id` is provided, also runs loudness analysis for normalization.
    pub fn prefetch(
        app_handle: AppHandle,
        client: SubsonicClientHandle,
        song_id: String,
        suffix: String,
        album_id: Option<String>,
    ) {
        tauri::async_runtime::spawn(async move {
            // Check if already being prefetched
            {
                let mut in_progress = PREFETCH_IN_PROGRESS.lock().await;
                if in_progress.contains(&song_id) {
                    return;
                }
                in_progress.insert(song_id.clone());
            }

            // Create cache instance
            let Ok(cache) = AudioCache::new(&app_handle) else {
                // Remove from in-progress on error
                PREFETCH_IN_PROGRESS.lock().await.remove(&song_id);
                return;
            };

            let was_cached = cache.is_cached(&song_id, &suffix);

            // Fetch and cache if not already cached
            let audio_data = if was_cached {
                None
            } else {
                match cache.get_or_fetch(&client, &song_id, &suffix).await {
                    Ok(data) => {
                        debug!("Prefetch complete: {song_id}");
                        Some(data)
                    }
                    Err(e) => {
                        warn!("Prefetch failed for {song_id}: {e}");
                        PREFETCH_IN_PROGRESS.lock().await.remove(&song_id);
                        return;
                    }
                }
            };

            // Run loudness analysis if album_id is provided (normalization needed)
            if let Some(album_id) = album_id {
                // Get audio data: use what we just fetched, or read from cache
                let data_for_analysis = match audio_data {
                    Some(data) => Some(data),
                    None => std::fs::read(cache.get_cache_path(&song_id, &suffix)).ok(),
                };

                if let Some(data) = data_for_analysis {
                    let song_id_clone = song_id.clone();
                    let app_handle_clone = app_handle.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        match loudness::analyze_loudness(data) {
                            Ok(result) => {
                                if let Ok(db_path) = db::get_db_path(&app_handle_clone) {
                                    let _ = db::save_normalization_result(
                                        std::path::Path::new(&db_path),
                                        &song_id_clone,
                                        &album_id,
                                        result.integrated_lufs,
                                        result.true_peak,
                                    );
                                    debug!(
                                        "Prefetch normalization complete: {} ({:.1} LUFS)",
                                        song_id_clone, result.integrated_lufs
                                    );
                                }
                            }
                            Err(e) => {
                                warn!("Prefetch loudness analysis failed for {song_id_clone}: {e}");
                            }
                        }
                    })
                    .await;
                }
            }

            // Remove from in-progress
            PREFETCH_IN_PROGRESS.lock().await.remove(&song_id);
        });
    }
}

/// Internal struct for tracking cache entries
struct CacheEntry {
    path: PathBuf,
    size: u64,
    accessed: SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cache_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "stereodrome-audio-cache-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn interrupted_cache_write_does_not_publish_destination() {
        let cache_dir = test_cache_dir("interrupted-write");
        fs::create_dir_all(&cache_dir).expect("create test cache directory");
        let destination = cache_dir.join("song.flac");

        let (temp_path, mut temp_file) =
            create_cache_temp_file(&destination).expect("create cache temp file");
        temp_file
            .write_all(b"partial audio")
            .expect("write partial cache file");
        drop(temp_file);

        assert!(!destination.exists());
        assert!(temp_path.exists());

        fs::remove_dir_all(cache_dir).expect("remove test cache directory");
    }

    #[test]
    fn completed_cache_write_is_atomically_published() {
        let cache_dir = test_cache_dir("completed-write");
        fs::create_dir_all(&cache_dir).expect("create test cache directory");
        let destination = cache_dir.join("song.flac");
        let bytes = b"complete audio";

        write_cache_file_atomically(&destination, bytes).expect("write cache file atomically");

        assert_eq!(fs::read(&destination).expect("read cache file"), bytes);
        assert_eq!(
            fs::read_dir(&cache_dir)
                .expect("read test cache directory")
                .count(),
            1
        );

        fs::remove_dir_all(cache_dir).expect("remove test cache directory");
    }
}
