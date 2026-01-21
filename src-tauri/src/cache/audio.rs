use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use serde::Serialize;
use submarine::Client;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex as TokioMutex;

use crate::audio::fetch_audio_bytes;
use crate::error::{AppError, AppResult};

/// Default maximum cache size: 5 GB
pub const DEFAULT_MAX_CACHE_SIZE: u64 = 5 * 1024 * 1024 * 1024;

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
    cache_dir: PathBuf,
    max_size: u64,
}

impl AudioCache {
    /// Create a new AudioCache instance
    pub fn new(app_handle: &AppHandle, max_size: u64) -> AppResult<Self> {
        let data_dir = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, e)))?;
        let cache_dir = data_dir.join("audio_cache");
        fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            cache_dir,
            max_size,
        })
    }

    /// Get the cache file path for a song
    fn get_cache_path(&self, song_id: &str, suffix: &str) -> PathBuf {
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
        client: &Client,
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
                    eprintln!("Failed to read cached audio: {}", e);
                }
            }
        }

        // Fetch from server
        let bytes = fetch_audio_bytes(client, song_id).await?;

        // Write to cache (fire-and-forget, don't fail playback if caching fails)
        if let Err(e) = fs::write(&cache_path, &bytes) {
            eprintln!("Failed to cache audio: {}", e);
        } else {
            // Enforce size limit after successful write
            if let Err(e) = self.enforce_size_limit() {
                eprintln!("Failed to enforce cache size limit: {}", e);
            }
        }

        Ok(bytes)
    }

    /// Update the access time of a file (for LRU tracking)
    fn touch_file(&self, path: &Path) {
        // On Unix, we can use filetime crate or just open and close the file
        // For simplicity, we'll use a platform-independent approach
        if let Ok(file) = fs::OpenOptions::new().read(true).open(path) {
            drop(file);
        }
    }

    /// Enforce the maximum cache size by removing least recently accessed files
    fn enforce_size_limit(&self) -> AppResult<()> {
        let mut entries = self.get_cache_entries()?;

        // Calculate total size
        let total_size: u64 = entries.iter().map(|e| e.size).sum();

        if total_size <= self.max_size {
            return Ok(());
        }

        // Sort by access time (oldest first)
        entries.sort_by(|a, b| a.accessed.cmp(&b.accessed));

        // Remove files until under the limit
        let mut current_size = total_size;
        for entry in entries {
            if current_size <= self.max_size {
                break;
            }

            if let Err(e) = fs::remove_file(&entry.path) {
                eprintln!("Failed to remove cached file {:?}: {}", entry.path, e);
            } else {
                current_size = current_size.saturating_sub(entry.size);
            }
        }

        Ok(())
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
        for entry in entries {
            if let Err(e) = fs::remove_file(&entry.path) {
                eprintln!("Failed to remove cached file {:?}: {}", entry.path, e);
            }
        }
        Ok(())
    }

    /// Get all cache entries with metadata
    fn get_cache_entries(&self) -> AppResult<Vec<CacheEntry>> {
        let mut entries = Vec::new();

        let read_dir = fs::read_dir(&self.cache_dir)?;
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Ok(metadata) = entry.metadata() {
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
        }

        Ok(entries)
    }

    /// Prefetch a song to cache in the background.
    /// Returns immediately - the fetch happens asynchronously.
    /// Skips if the song is already cached or currently being prefetched.
    pub fn prefetch(
        app_handle: AppHandle,
        client: Client,
        song_id: String,
        suffix: String,
        max_size: u64,
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
            let cache = match AudioCache::new(&app_handle, max_size) {
                Ok(c) => c,
                Err(_) => {
                    // Remove from in-progress on error
                    PREFETCH_IN_PROGRESS.lock().await.remove(&song_id);
                    return;
                }
            };

            // Skip if already cached
            if cache.is_cached(&song_id, &suffix) {
                PREFETCH_IN_PROGRESS.lock().await.remove(&song_id);
                return;
            }

            // Fetch and cache
            let _ = cache.get_or_fetch(&client, &song_id, &suffix).await;

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
