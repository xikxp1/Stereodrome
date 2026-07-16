use std::sync::Arc;

use crate::cache::{
    AudioCache, CacheLocationInfo, CacheRootUpdateResult, CacheStats, KEY_MAX_CACHE_SIZE,
    MAX_CACHE_SIZE, MIN_CACHE_SIZE, cache_location_info, set_cache_root as persist_cache_root,
};
use crate::error::{AppResult, MutexExt};
use crate::state::DesktopState;

/// Get configured cache locations.
pub fn get_cache_locations(state: &DesktopState) -> AppResult<CacheLocationInfo> {
    cache_location_info(&state.paths, &state.settings)
}

/// Set the cache root and move existing cache files into the new root.
pub fn set_cache_root(
    state: &DesktopState,
    cache_root: Option<String>,
) -> AppResult<CacheRootUpdateResult> {
    let result = persist_cache_root(&state.paths, &state.settings, cache_root)?;
    state.events.audio_cache_changed("location_changed");
    Ok(result)
}

/// Get statistics about the audio cache
pub fn get_audio_cache_stats(state: Arc<DesktopState>) -> AppResult<CacheStats> {
    let cache = AudioCache::new(state)?;
    cache.get_stats()
}

/// Get locally cached song IDs that can be played without a server connection.
pub fn get_offline_song_ids(state: Arc<DesktopState>) -> AppResult<Vec<String>> {
    let cache = AudioCache::new(Arc::clone(&state))?;
    let db = state.db.lock_recover();
    let mut stmt = db.prepare("SELECT id, suffix FROM songs")?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        ))
    })?;

    let mut song_ids = Vec::new();
    for row in rows {
        let (song_id, suffix) = row?;
        if cache.is_cached(&song_id, &suffix) {
            song_ids.push(song_id);
        }
    }

    Ok(song_ids)
}

/// Clear all cached audio files
pub fn clear_audio_cache(state: Arc<DesktopState>) -> AppResult<()> {
    let cache = AudioCache::new(state)?;
    cache.clear()
}

/// Set the maximum cache size (in bytes)
pub fn set_max_cache_size(state: Arc<DesktopState>, size: u64) -> AppResult<CacheStats> {
    let size = size.clamp(MIN_CACHE_SIZE, MAX_CACHE_SIZE);

    state.settings.set(KEY_MAX_CACHE_SIZE, size)?;

    // Enforce new limit (AudioCache::new reads the size we just saved to the store)
    let cache = AudioCache::new(state)?;
    let evicted = cache.enforce_size_limit()?;
    if evicted {
        cache.emit_changed("evicted");
    }

    // Return updated stats
    cache.get_stats()
}
