use std::collections::{HashMap, HashSet};

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::cache::AudioCache;
use crate::commands::coverart::preserve_cover_art_for_offline;
use crate::error::{AppResult, MutexExt};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub song_count: i32,
    pub duration: i32,
    pub owner: Option<String>,
    pub cover_art_id: Option<String>,
    pub created_at: String,
    pub changed_at: String,
    pub saved_offline: bool,
    pub offline_saved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistSong {
    pub id: String,
    pub album_id: String,
    pub artist_id: String,
    pub title: String,
    pub track_number: Option<i32>,
    pub disc_number: i32,
    pub duration: Option<i32>,
    pub bit_rate: Option<i32>,
    pub size: Option<i64>,
    pub suffix: Option<String>,
    pub content_type: Option<String>,
    pub path: Option<String>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub synced_at: String,
    pub position: i32,
    // Joined fields
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPlaylistOfflineResult {
    pub playlist_id: String,
    pub saved_offline: bool,
    pub downloaded_count: i32,
    pub removed_count: i32,
    pub skipped_protected_count: i32,
}

/// Sync playlists from Subsonic server to local cache
#[tauri::command]
pub async fn sync_playlists(app_handle: AppHandle, state: State<'_, AppState>) -> AppResult<i32> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(crate::error::AppError::OfflineMode);
    }

    info!("Syncing playlists from server");

    // Fetch all playlists from server
    let playlists = state.client.get_playlists().await?;
    let playlist_count = playlists.len() as i32;

    // Fetch songs for each playlist
    let mut playlist_details = Vec::new();
    for playlist_info in &playlists {
        match state.client.get_playlist(&playlist_info.id).await {
            Ok(detail) => playlist_details.push(detail),
            Err(e) => {
                debug!(
                    "Failed to fetch playlist '{}': {}, skipping",
                    playlist_info.name, e
                );
            }
        }
    }

    let old_saved_song_ids = {
        // Write to local database in a transaction
        let db = state.db.lock_recover();
        let saved_playlists = saved_playlist_timestamps(&db)?;
        let old_saved_song_ids = saved_playlist_song_ids(&db)?;
        db.execute_batch("BEGIN TRANSACTION")?;

        // Clear existing playlist data
        db.execute("DELETE FROM playlist_songs", [])?;
        db.execute("DELETE FROM playlists", [])?;

        let now = chrono::Utc::now().to_rfc3339();

        // Insert playlists and their songs
        for detail in &playlist_details {
            let info = &detail.info;
            let offline_saved_at = saved_playlists.get(&info.id);
            db.execute(
                "INSERT INTO playlists
                 (id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, offline_saved_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    &info.id,
                    &info.name,
                    info.song_count,
                    info.duration,
                    &info.owner,
                    &info.cover_art,
                    &info.created,
                    &info.changed,
                    offline_saved_at,
                    &now,
                ],
            )?;

            // Insert playlist songs (only those that exist in the local songs table)
            for (position, entry) in detail.entries.iter().enumerate() {
                // Check if song exists in local library
                let song_exists: bool = db
                    .query_row("SELECT 1 FROM songs WHERE id = ?1", [&entry.id], |_| {
                        Ok(true)
                    })
                    .unwrap_or(false);

                if song_exists {
                    db.execute(
                        "INSERT INTO playlist_songs (playlist_id, song_id, position) VALUES (?1, ?2, ?3)",
                        rusqlite::params![&info.id, &entry.id, position as i32],
                    )?;
                }
            }
        }

        db.execute_batch("COMMIT")?;
        old_saved_song_ids
    };

    let _ = reconcile_saved_playlists_offline_inner(&app_handle, &state).await?;
    remove_unprotected_cached_songs(&app_handle, &state, old_saved_song_ids)?;

    info!("Synced {} playlists with songs", playlist_details.len());
    Ok(playlist_count)
}

/// Get all playlists from local cache
#[tauri::command]
pub fn get_playlists(state: State<'_, AppState>) -> AppResult<Vec<Playlist>> {
    let db = state.db.lock_recover();
    let mut stmt = db.prepare(
        "SELECT id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, offline_saved_at FROM playlists ORDER BY name",
    )?;

    let playlists: Vec<Playlist> = stmt
        .query_map([], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                song_count: row.get(2)?,
                duration: row.get(3)?,
                owner: row.get(4)?,
                cover_art_id: row.get(5)?,
                created_at: row.get(6)?,
                changed_at: row.get(7)?,
                saved_offline: row.get::<_, Option<String>>(8)?.is_some(),
                offline_saved_at: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(playlists)
}

/// Get songs for a playlist from local cache
#[tauri::command]
pub fn get_playlist_songs(
    state: State<'_, AppState>,
    playlist_id: String,
) -> AppResult<Vec<PlaylistSong>> {
    let db = state.db.lock_recover();
    let mut stmt = db.prepare(
        r#"
        SELECT
            s.id, s.album_id, s.artist_id, s.title, s.track_number, s.disc_number,
            s.duration, s.bit_rate, s.size, s.suffix, s.content_type, s.path,
            s.year, s.genre, s.synced_at,
            ps.position,
            a.name as artist_name,
            al.name as album_name
        FROM playlist_songs ps
        JOIN songs s ON s.id = ps.song_id
        LEFT JOIN artists a ON a.id = s.artist_id
        LEFT JOIN albums al ON al.id = s.album_id
        WHERE ps.playlist_id = ?1
        ORDER BY ps.position
        "#,
    )?;

    let songs: Vec<PlaylistSong> = stmt
        .query_map([&playlist_id], |row| {
            Ok(PlaylistSong {
                id: row.get(0)?,
                album_id: row.get(1)?,
                artist_id: row.get(2)?,
                title: row.get(3)?,
                track_number: row.get(4)?,
                disc_number: row.get(5)?,
                duration: row.get(6)?,
                bit_rate: row.get(7)?,
                size: row.get(8)?,
                suffix: row.get(9)?,
                content_type: row.get(10)?,
                path: row.get(11)?,
                year: row.get(12)?,
                genre: row.get(13)?,
                synced_at: row.get(14)?,
                position: row.get(15)?,
                artist: row.get(16)?,
                album: row.get(17)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(songs)
}

/// Create a new playlist on server and cache locally
#[tauri::command]
pub async fn create_playlist(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    name: String,
    song_ids: Option<Vec<String>>,
) -> AppResult<Playlist> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(crate::error::AppError::OfflineMode);
    }

    let ids = playlist_song_ids_to_add(song_ids.unwrap_or_default(), &HashSet::new());
    let detail = state.client.create_playlist(&name, ids).await?;
    let info = &detail.info;

    let now = chrono::Utc::now().to_rfc3339();

    // Cache locally
    let db = state.db.lock_recover();
    db.execute(
        "INSERT INTO playlists
         (id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, offline_saved_at, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            song_count = excluded.song_count,
            duration = excluded.duration,
            owner = excluded.owner,
            cover_art_id = excluded.cover_art_id,
            created_at = excluded.created_at,
            changed_at = excluded.changed_at,
            synced_at = excluded.synced_at",
        rusqlite::params![
            &info.id,
            &info.name,
            info.song_count,
            info.duration,
            &info.owner,
            &info.cover_art,
            &info.created,
            &info.changed,
            &now,
        ],
    )?;

    // Insert playlist songs
    for (position, entry) in detail.entries.iter().enumerate() {
        let song_exists: bool = db
            .query_row("SELECT 1 FROM songs WHERE id = ?1", [&entry.id], |_| {
                Ok(true)
            })
            .unwrap_or(false);

        if song_exists {
            db.execute(
                "INSERT INTO playlist_songs (playlist_id, song_id, position) VALUES (?1, ?2, ?3)",
                rusqlite::params![&info.id, &entry.id, position as i32],
            )?;
        }
    }

    Ok(Playlist {
        id: info.id.clone(),
        name: info.name.clone(),
        song_count: info.song_count,
        duration: info.duration,
        owner: info.owner.clone(),
        cover_art_id: info.cover_art.clone(),
        created_at: info.created.clone(),
        changed_at: info.changed.clone(),
        saved_offline: false,
        offline_saved_at: None,
    })
}

/// Rename a playlist on server and update local cache
#[tauri::command]
pub async fn update_playlist(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    name: String,
) -> AppResult<()> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(crate::error::AppError::OfflineMode);
    }

    // Update on server
    state
        .client
        .update_playlist(&playlist_id, Some(name.clone()), vec![], vec![])
        .await?;

    // Update local cache
    let now = chrono::Utc::now().to_rfc3339();
    let db = state.db.lock_recover();
    db.execute(
        "UPDATE playlists SET name = ?1, changed_at = ?2 WHERE id = ?3",
        rusqlite::params![&name, &now, &playlist_id],
    )?;

    Ok(())
}

/// Delete a playlist from server and local cache
#[tauri::command]
pub async fn delete_playlist(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
) -> AppResult<()> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(crate::error::AppError::OfflineMode);
    }

    let removed_song_ids = playlist_song_ids(&state, &playlist_id)?;

    // Delete from server
    state.client.delete_playlist(&playlist_id).await?;

    // Delete from local cache
    let db = state.db.lock_recover();
    db.execute(
        "DELETE FROM playlist_songs WHERE playlist_id = ?1",
        [&playlist_id],
    )?;
    db.execute("DELETE FROM playlists WHERE id = ?1", [&playlist_id])?;
    drop(db);

    remove_unprotected_cached_songs(&app_handle, &state, removed_song_ids)?;

    Ok(())
}

/// Add songs to a playlist on server and refresh local cache
#[tauri::command]
pub async fn add_songs_to_playlist(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    song_ids: Vec<String>,
) -> AppResult<()> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(crate::error::AppError::OfflineMode);
    }

    let playlist = state.client.get_playlist(&playlist_id).await?;
    let existing_song_ids: HashSet<String> = playlist
        .entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect();
    let new_song_ids = playlist_song_ids_to_add(song_ids, &existing_song_ids);

    if new_song_ids.is_empty() {
        return Ok(());
    }

    // Add songs on server
    state
        .client
        .update_playlist(&playlist_id, None, new_song_ids, vec![])
        .await?;

    // Re-fetch playlist from server and update local cache
    refresh_playlist_cache(&state, &playlist_id).await?;
    if playlist_saved_offline(&state, &playlist_id)? {
        let _ = download_playlist_to_cache(&app_handle, &state, &playlist_id).await?;
    }

    Ok(())
}

fn playlist_song_ids_to_add(
    song_ids: Vec<String>,
    existing_song_ids: &HashSet<String>,
) -> Vec<String> {
    let mut seen_song_ids = existing_song_ids.clone();

    song_ids
        .into_iter()
        .fold(Vec::new(), |mut output, song_id| {
            if !song_id.trim().is_empty() && seen_song_ids.insert(song_id.clone()) {
                output.push(song_id);
            }
            output
        })
}

/// Remove a song from a playlist by position on server and refresh local cache
#[tauri::command]
pub async fn remove_song_from_playlist(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    position: i32,
) -> AppResult<()> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(crate::error::AppError::OfflineMode);
    }

    remove_playlist_positions(&app_handle, &state, &playlist_id, vec![position as i64]).await
}

/// Remove multiple songs from a playlist by position on server and refresh local cache
#[tauri::command]
pub async fn remove_songs_from_playlist(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    positions: Vec<i32>,
) -> AppResult<()> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(crate::error::AppError::OfflineMode);
    }

    let mut positions_to_remove: Vec<i64> = positions
        .into_iter()
        .filter(|position| *position >= 0)
        .map(i64::from)
        .collect();
    positions_to_remove.sort_unstable();
    positions_to_remove.dedup();

    remove_playlist_positions(&app_handle, &state, &playlist_id, positions_to_remove).await
}

async fn remove_playlist_positions(
    app_handle: &AppHandle,
    state: &State<'_, AppState>,
    playlist_id: &str,
    positions: Vec<i64>,
) -> AppResult<()> {
    if positions.is_empty() {
        return Ok(());
    }
    let before_song_ids = playlist_song_ids(state, playlist_id)?;

    // Remove by index on server
    state
        .client
        .update_playlist(playlist_id, None, vec![], positions)
        .await?;

    // Re-fetch playlist from server and update local cache
    refresh_playlist_cache(state, playlist_id).await?;
    if playlist_saved_offline(state, playlist_id)? {
        let after_song_ids = playlist_song_ids(state, playlist_id)?;
        let removed_song_ids = before_song_ids
            .difference(&after_song_ids)
            .cloned()
            .collect::<HashSet<_>>();
        remove_unprotected_cached_songs(app_handle, state, removed_song_ids)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn set_playlist_saved_offline(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    saved_offline: bool,
) -> AppResult<SavedPlaylistOfflineResult> {
    if saved_offline {
        if crate::commands::settings::manual_offline_enabled(&app_handle) {
            return Err(crate::error::AppError::OfflineMode);
        }

        let previous_saved_at = playlist_offline_saved_at(&state, &playlist_id)?;
        set_playlist_offline_saved_at(
            &state,
            &playlist_id,
            Some(
                previous_saved_at
                    .clone()
                    .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
            ),
        )?;
        match download_playlist_to_cache(&app_handle, &state, &playlist_id).await {
            Ok(downloaded_count) => Ok(SavedPlaylistOfflineResult {
                playlist_id,
                saved_offline: true,
                downloaded_count,
                removed_count: 0,
                skipped_protected_count: 0,
            }),
            Err(error) => {
                set_playlist_offline_saved_at(&state, &playlist_id, previous_saved_at)?;
                Err(error)
            }
        }
    } else {
        let song_ids = playlist_song_ids(&state, &playlist_id)?;
        set_playlist_offline_saved_at(&state, &playlist_id, None)?;
        let (removed_count, skipped_protected_count) =
            remove_unprotected_cached_songs(&app_handle, &state, song_ids)?;
        Ok(SavedPlaylistOfflineResult {
            playlist_id,
            saved_offline: false,
            downloaded_count: 0,
            removed_count,
            skipped_protected_count,
        })
    }
}

#[tauri::command]
pub async fn reconcile_saved_playlists_offline(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<SavedPlaylistOfflineResult>> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Ok(Vec::new());
    }

    reconcile_saved_playlists_offline_inner(&app_handle, &state).await
}

/// Re-fetch a single playlist from server and update local cache
async fn refresh_playlist_cache(state: &State<'_, AppState>, playlist_id: &str) -> AppResult<()> {
    let detail = state.client.get_playlist(playlist_id).await?;
    let info = &detail.info;
    let now = chrono::Utc::now().to_rfc3339();

    let db = state.db.lock_recover();

    // Update playlist metadata
    db.execute(
        "INSERT INTO playlists
         (id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, offline_saved_at, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, (SELECT offline_saved_at FROM playlists WHERE id = ?1), ?9)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            song_count = excluded.song_count,
            duration = excluded.duration,
            owner = excluded.owner,
            cover_art_id = excluded.cover_art_id,
            created_at = excluded.created_at,
            changed_at = excluded.changed_at,
            synced_at = excluded.synced_at",
        rusqlite::params![
            &info.id,
            &info.name,
            info.song_count,
            info.duration,
            &info.owner,
            &info.cover_art,
            &info.created,
            &info.changed,
            &now,
        ],
    )?;

    // Replace playlist songs
    db.execute(
        "DELETE FROM playlist_songs WHERE playlist_id = ?1",
        [playlist_id],
    )?;

    for (position, entry) in detail.entries.iter().enumerate() {
        let song_exists: bool = db
            .query_row("SELECT 1 FROM songs WHERE id = ?1", [&entry.id], |_| {
                Ok(true)
            })
            .unwrap_or(false);

        if song_exists {
            db.execute(
                "INSERT INTO playlist_songs (playlist_id, song_id, position) VALUES (?1, ?2, ?3)",
                rusqlite::params![playlist_id, &entry.id, position as i32],
            )?;
        }
    }

    Ok(())
}

async fn reconcile_saved_playlists_offline_inner(
    app_handle: &AppHandle,
    state: &State<'_, AppState>,
) -> AppResult<Vec<SavedPlaylistOfflineResult>> {
    let playlist_ids = {
        let db = state.db.lock_recover();
        let mut stmt = db
            .prepare("SELECT id FROM playlists WHERE offline_saved_at IS NOT NULL ORDER BY name")?;
        stmt.query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut results = Vec::with_capacity(playlist_ids.len());
    for playlist_id in playlist_ids {
        let downloaded_count =
            match download_playlist_to_cache(app_handle, state, &playlist_id).await {
                Ok(count) => count,
                Err(error) => {
                    warn!(
                        "Failed to reconcile saved playlist {}: {}",
                        playlist_id, error
                    );
                    0
                }
            };
        results.push(SavedPlaylistOfflineResult {
            playlist_id,
            saved_offline: true,
            downloaded_count,
            removed_count: 0,
            skipped_protected_count: 0,
        });
    }

    Ok(results)
}

fn saved_playlist_timestamps(db: &rusqlite::Connection) -> AppResult<HashMap<String, String>> {
    let mut stmt = db
        .prepare("SELECT id, offline_saved_at FROM playlists WHERE offline_saved_at IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

fn saved_playlist_song_ids(db: &rusqlite::Connection) -> AppResult<HashSet<String>> {
    let mut stmt = db.prepare(
        "SELECT DISTINCT ps.song_id
         FROM playlist_songs ps
         JOIN playlists p ON p.id = ps.playlist_id
         WHERE p.offline_saved_at IS NOT NULL",
    )?;
    Ok(stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?)
}

fn playlist_song_ids(state: &State<'_, AppState>, playlist_id: &str) -> AppResult<HashSet<String>> {
    let db = state.db.lock_recover();
    let mut stmt = db.prepare("SELECT song_id FROM playlist_songs WHERE playlist_id = ?1")?;
    Ok(stmt
        .query_map([playlist_id], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?)
}

fn playlist_saved_offline(state: &State<'_, AppState>, playlist_id: &str) -> AppResult<bool> {
    Ok(playlist_offline_saved_at(state, playlist_id)?.is_some())
}

fn playlist_offline_saved_at(
    state: &State<'_, AppState>,
    playlist_id: &str,
) -> AppResult<Option<String>> {
    let db = state.db.lock_recover();
    Ok(db
        .query_row(
            "SELECT offline_saved_at FROM playlists WHERE id = ?1",
            [playlist_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .unwrap_or(None))
}

fn set_playlist_offline_saved_at(
    state: &State<'_, AppState>,
    playlist_id: &str,
    offline_saved_at: Option<String>,
) -> AppResult<()> {
    let db = state.db.lock_recover();
    db.execute(
        "UPDATE playlists SET offline_saved_at = ?1 WHERE id = ?2",
        rusqlite::params![offline_saved_at, playlist_id],
    )?;
    Ok(())
}

async fn download_playlist_to_cache(
    app_handle: &AppHandle,
    state: &State<'_, AppState>,
    playlist_id: &str,
) -> AppResult<i32> {
    let (songs, cover_art_ids) = {
        let db = state.db.lock_recover();
        let songs = {
            let mut stmt = db.prepare(
                "SELECT s.id, COALESCE(s.suffix, '')
                 FROM playlist_songs ps
                 JOIN songs s ON s.id = ps.song_id
                 WHERE ps.playlist_id = ?1
                 ORDER BY ps.position",
            )?;
            stmt.query_map([playlist_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
        };
        let cover_art_ids = playlist_offline_cover_art_ids(&db, playlist_id)?;
        (songs, cover_art_ids)
    };

    let cache = AudioCache::new(app_handle)?;
    let mut downloaded_count = 0;
    for (song_id, suffix) in songs {
        cache.get_or_fetch(&state.client, &song_id, &suffix).await?;
        downloaded_count += 1;
    }

    for cover_art_id in cover_art_ids {
        if let Err(error) =
            preserve_cover_art_for_offline(app_handle, &state.client, &cover_art_id).await
        {
            warn!(
                "Failed to preserve cover art {} for offline playlist {}: {}",
                cover_art_id, playlist_id, error
            );
        }
    }

    cache.emit_changed("saved-playlist");
    Ok(downloaded_count)
}

fn playlist_offline_cover_art_ids(
    db: &rusqlite::Connection,
    playlist_id: &str,
) -> AppResult<Vec<String>> {
    let mut stmt = db.prepare(
        "SELECT cover_art_id FROM (
            SELECT 0 AS sort_order, 0 AS position, p.cover_art_id
            FROM playlists p
            WHERE p.id = ?1
            UNION ALL
            SELECT 1 AS sort_order, ps.position, al.cover_art_id
            FROM playlist_songs ps
            JOIN songs s ON s.id = ps.song_id
            LEFT JOIN albums al ON al.id = s.album_id
            WHERE ps.playlist_id = ?1
            UNION ALL
            SELECT 2 AS sort_order, ps.position, ar.cover_art_id
            FROM playlist_songs ps
            JOIN songs s ON s.id = ps.song_id
            LEFT JOIN artists ar ON ar.id = s.artist_id
            WHERE ps.playlist_id = ?1
        )
        ORDER BY sort_order, position",
    )?;
    let ids = stmt
        .query_map([playlist_id], |row| row.get::<_, Option<String>>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(distinct_nonempty_cover_art_ids(ids))
}

fn distinct_nonempty_cover_art_ids(ids: Vec<Option<String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();

    for id in ids {
        let Some(id) = id else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        output.push(id.to_string());
    }

    output
}

fn remove_unprotected_cached_songs(
    app_handle: &AppHandle,
    state: &State<'_, AppState>,
    song_ids: HashSet<String>,
) -> AppResult<(i32, i32)> {
    let cache = AudioCache::new(app_handle)?;
    let mut removed_count = 0;
    let mut skipped_protected_count = 0;

    for song_id in song_ids {
        let (suffix, protected): (String, bool) = {
            let db = state.db.lock_recover();
            let suffix = db
                .query_row(
                    "SELECT COALESCE(suffix, '') FROM songs WHERE id = ?1",
                    [&song_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap_or_default();
            let protected = db
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1
                        FROM playlist_songs ps
                        JOIN playlists p ON p.id = ps.playlist_id
                        WHERE ps.song_id = ?1 AND p.offline_saved_at IS NOT NULL
                     )",
                    [&song_id],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap_or(false);
            (suffix, protected)
        };

        if protected {
            skipped_protected_count += 1;
            continue;
        }
        if cache.remove_if_unprotected(&song_id, &suffix)? {
            removed_count += 1;
        }
    }

    Ok((removed_count, skipped_protected_count))
}

#[cfg(test)]
mod tests {
    use super::{
        distinct_nonempty_cover_art_ids, playlist_offline_cover_art_ids, playlist_song_ids_to_add,
    };
    use rusqlite::Connection;
    use std::collections::HashSet;

    #[test]
    fn playlist_song_ids_to_add_skips_duplicate_entries() {
        let song_ids = playlist_song_ids_to_add(
            vec![
                "song-a".to_string(),
                "song-b".to_string(),
                "song-a".to_string(),
            ],
            &HashSet::new(),
        );

        assert_eq!(song_ids, ["song-a", "song-b"]);
    }

    #[test]
    fn playlist_song_ids_to_add_ignores_empty_entries() {
        let song_ids = playlist_song_ids_to_add(
            vec![
                "song-a".to_string(),
                "".to_string(),
                "  ".to_string(),
                "song-b".to_string(),
            ],
            &HashSet::new(),
        );

        assert_eq!(song_ids, ["song-a", "song-b"]);
    }

    #[test]
    fn playlist_song_ids_to_add_skips_existing_playlist_entries() {
        let song_ids = playlist_song_ids_to_add(
            vec![
                "song-a".to_string(),
                "song-b".to_string(),
                "song-c".to_string(),
            ],
            &HashSet::from(["song-a".to_string(), "song-c".to_string()]),
        );

        assert_eq!(song_ids, ["song-b"]);
    }

    #[test]
    fn distinct_nonempty_cover_art_ids_preserves_first_occurrence_order() {
        let cover_art_ids = distinct_nonempty_cover_art_ids(vec![
            Some(" playlist-cover ".to_string()),
            Some("album-cover".to_string()),
            None,
            Some("".to_string()),
            Some("album-cover".to_string()),
            Some("artist-cover".to_string()),
            Some("playlist-cover".to_string()),
        ]);

        assert_eq!(
            cover_art_ids,
            ["playlist-cover", "album-cover", "artist-cover"]
        );
    }

    #[test]
    fn playlist_offline_cover_art_ids_includes_playlist_album_and_artist_art() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "
            CREATE TABLE playlists (
                id TEXT PRIMARY KEY,
                cover_art_id TEXT
            );
            CREATE TABLE artists (
                id TEXT PRIMARY KEY,
                cover_art_id TEXT
            );
            CREATE TABLE albums (
                id TEXT PRIMARY KEY,
                cover_art_id TEXT
            );
            CREATE TABLE songs (
                id TEXT PRIMARY KEY,
                album_id TEXT NOT NULL,
                artist_id TEXT NOT NULL
            );
            CREATE TABLE playlist_songs (
                playlist_id TEXT NOT NULL,
                song_id TEXT NOT NULL,
                position INTEGER NOT NULL
            );
            INSERT INTO playlists (id, cover_art_id) VALUES ('playlist-1', 'playlist-cover');
            INSERT INTO artists (id, cover_art_id) VALUES
                ('artist-1', 'artist-cover'),
                ('artist-2', 'artist-cover-2');
            INSERT INTO albums (id, cover_art_id) VALUES
                ('album-1', 'album-cover'),
                ('album-2', 'album-cover');
            INSERT INTO songs (id, album_id, artist_id) VALUES
                ('song-1', 'album-1', 'artist-1'),
                ('song-2', 'album-2', 'artist-2');
            INSERT INTO playlist_songs (playlist_id, song_id, position) VALUES
                ('playlist-1', 'song-1', 0),
                ('playlist-1', 'song-2', 1);
            ",
        )
        .unwrap();

        let cover_art_ids = playlist_offline_cover_art_ids(&db, "playlist-1").unwrap();

        assert_eq!(
            cover_art_ids,
            [
                "playlist-cover",
                "album-cover",
                "artist-cover",
                "artist-cover-2"
            ]
        );
    }
}
