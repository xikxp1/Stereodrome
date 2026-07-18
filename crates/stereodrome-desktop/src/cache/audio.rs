use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use crate::JsonStore;
use filetime::FileTime;
use log::{debug, warn};
use serde::Serialize;
use tokio::sync::Mutex as TokioMutex;

use crate::audio::loudness;
use crate::client::SubsonicClientHandle;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::operations::settings::manual_offline_enabled;
use crate::state::DesktopState;

/// Default maximum cache size: 5 GB
pub const DEFAULT_MAX_CACHE_SIZE: u64 = 5 * 1024 * 1024 * 1024;
/// Minimum cache size: 500 MB
pub const MIN_CACHE_SIZE: u64 = 500 * 1024 * 1024;
/// Maximum cache size: 50 GB
pub const MAX_CACHE_SIZE: u64 = 50 * 1024 * 1024 * 1024;
pub const KEY_MAX_CACHE_SIZE: &str = "max_cache_size";

#[derive(Debug, Clone, Serialize)]
pub struct AudioCacheChangedEvent {
    pub reason: &'static str,
}

/// Read the configured max cache size from settings store
fn read_max_cache_size(settings: &JsonStore) -> u64 {
    settings
        .get::<u64>(KEY_MAX_CACHE_SIZE)
        .ok()
        .flatten()
        .map(|size| size.clamp(MIN_CACHE_SIZE, MAX_CACHE_SIZE))
        .unwrap_or(DEFAULT_MAX_CACHE_SIZE)
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

/// Audio file cache with LRU eviction
pub struct AudioCache {
    state: Arc<DesktopState>,
    cache_dir: PathBuf,
    max_size: u64,
}

impl AudioCache {
    /// Create a new AudioCache instance, reading max size from settings
    pub fn new(state: Arc<DesktopState>) -> AppResult<Self> {
        let max_size = read_max_cache_size(&state.settings);
        let cache_dir = crate::cache::audio_cache_dir(&state.paths, &state.settings)?;
        Ok(Self {
            state,
            cache_dir,
            max_size,
        })
    }

    pub fn emit_changed(&self, reason: &'static str) {
        self.state.events.audio_cache_changed(reason);
    }

    /// Get the cache file path for a song
    pub fn get_cache_path(&self, song_id: &str, suffix: &str) -> PathBuf {
        // Sanitize song_id for filesystem (replace problematic characters)
        let safe_id = song_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let filename = if suffix.is_empty() {
            safe_id
        } else {
            format!("{}.{}", safe_id, suffix)
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
                    warn!("Failed to read cached audio: {}", e);
                }
            }
        }

        if manual_offline_enabled(&self.state.settings) {
            return Err(AppError::OfflineMode);
        }

        // Fetch from server via client handle
        let bytes = client
            .stream(song_id)
            .await
            .map_err(|e| AppError::Audio(format!("Failed to fetch audio: {}", e)))?;

        // Write to cache (fire-and-forget, don't fail playback if caching fails)
        if let Err(e) = fs::write(&cache_path, &bytes) {
            warn!("Failed to cache audio: {}", e);
        } else {
            // Enforce size limit after successful write
            if let Err(e) = self.enforce_size_limit() {
                warn!("Failed to enforce cache size limit: {}", e);
            }
            self.emit_changed("updated");
        }

        Ok(bytes)
    }

    /// Update the access time of a file (for LRU tracking)
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
                warn!("Failed to remove cached file {:?}: {}", entry.path, e);
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
                warn!("Failed to remove cached file {:?}: {}", entry.path, e);
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
        let conn = rusqlite::Connection::open(&self.state.paths.database)?;
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
        runtime: &tokio::runtime::Handle,
        state: Arc<DesktopState>,
        client: SubsonicClientHandle,
        song_id: String,
        suffix: String,
        album_id: Option<String>,
    ) {
        let task_runtime = runtime.clone();
        runtime.spawn(async move {
            // Check if already being prefetched
            {
                let mut in_progress = PREFETCH_IN_PROGRESS.lock().await;
                if in_progress.contains(&song_id) {
                    return;
                }
                in_progress.insert(song_id.clone());
            }

            // Create cache instance
            let cache = match AudioCache::new(Arc::clone(&state)) {
                Ok(c) => c,
                Err(_) => {
                    // Remove from in-progress on error
                    PREFETCH_IN_PROGRESS.lock().await.remove(&song_id);
                    return;
                }
            };

            let was_cached = cache.is_cached(&song_id, &suffix);

            // Fetch and cache if not already cached
            let audio_data = if was_cached {
                None
            } else {
                match cache.get_or_fetch(&client, &song_id, &suffix).await {
                    Ok(data) => {
                        debug!("Prefetch complete: {}", song_id);
                        Some(data)
                    }
                    Err(e) => {
                        warn!("Prefetch failed for {}: {}", song_id, e);
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
                    let db_path = state.paths.database.clone();
                    let _ = task_runtime
                        .spawn_blocking(move || match loudness::analyze_loudness(data) {
                            Ok(result) => {
                                let _ = db::save_normalization_result(
                                    &db_path,
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
                            Err(e) => {
                                warn!(
                                    "Prefetch loudness analysis failed for {}: {}",
                                    song_id_clone, e
                                );
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
    use std::fs::OpenOptions;

    use super::*;
    use crate::{DesktopBackend, DesktopPaths};

    #[test]
    fn preserves_suffixes_and_evicts_the_oldest_file() {
        let data_dir =
            std::env::temp_dir().join(format!("stereodrome-cache-{}", uuid::Uuid::new_v4()));
        let backend = DesktopBackend::open(DesktopPaths::from_data_dir(data_dir.clone())).unwrap();
        backend
            .state()
            .settings
            .set(KEY_MAX_CACHE_SIZE, MIN_CACHE_SIZE)
            .unwrap();
        let cache = AudioCache::new(backend.state()).unwrap();

        let old = cache.get_cache_path("old/song", "flac");
        let recent = cache.get_cache_path("recent", "mp3");
        assert_eq!(old.file_name().unwrap(), "old_song.flac");
        for path in [&old, &recent] {
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)
                .unwrap()
                .set_len(300 * 1024 * 1024)
                .unwrap();
        }
        filetime::set_file_atime(&old, FileTime::from_unix_time(1, 0)).unwrap();
        filetime::set_file_atime(&recent, FileTime::from_unix_time(2, 0)).unwrap();

        assert!(cache.enforce_size_limit().unwrap());
        assert!(!old.exists());
        assert!(recent.exists());

        backend.shutdown().unwrap();
        std::fs::remove_dir_all(data_dir).ok();
    }
}
