use std::collections::HashSet;

use log::{debug, info};
use serde::{Deserialize, Serialize};
use tauri::State;

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

/// Sync playlists from Subsonic server to local cache
#[tauri::command]
pub async fn sync_playlists(state: State<'_, AppState>) -> AppResult<i32> {
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

    // Write to local database in a transaction
    let db = state.db.lock_recover();
    db.execute_batch("BEGIN TRANSACTION")?;

    // Clear existing playlist data
    db.execute("DELETE FROM playlist_songs", [])?;
    db.execute("DELETE FROM playlists", [])?;

    let now = chrono::Utc::now().to_rfc3339();

    // Insert playlists and their songs
    for detail in &playlist_details {
        let info = &detail.info;
        db.execute(
            "INSERT INTO playlists (id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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

    info!("Synced {} playlists with songs", playlist_details.len());
    Ok(playlist_count)
}

/// Get all playlists from local cache
#[tauri::command]
pub fn get_playlists(state: State<'_, AppState>) -> AppResult<Vec<Playlist>> {
    let db = state.db.lock_recover();
    let mut stmt = db.prepare(
        "SELECT id, name, song_count, duration, owner, cover_art_id, created_at, changed_at FROM playlists ORDER BY name",
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
    state: State<'_, AppState>,
    name: String,
    song_ids: Option<Vec<String>>,
) -> AppResult<Playlist> {
    let ids = playlist_song_ids_to_add(song_ids.unwrap_or_default(), &HashSet::new());
    let detail = state.client.create_playlist(&name, ids).await?;
    let info = &detail.info;

    let now = chrono::Utc::now().to_rfc3339();

    // Cache locally
    let db = state.db.lock_recover();
    db.execute(
        "INSERT OR REPLACE INTO playlists (id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
    })
}

/// Rename a playlist on server and update local cache
#[tauri::command]
pub async fn update_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    name: String,
) -> AppResult<()> {
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
pub async fn delete_playlist(state: State<'_, AppState>, playlist_id: String) -> AppResult<()> {
    // Delete from server
    state.client.delete_playlist(&playlist_id).await?;

    // Delete from local cache
    let db = state.db.lock_recover();
    db.execute(
        "DELETE FROM playlist_songs WHERE playlist_id = ?1",
        [&playlist_id],
    )?;
    db.execute("DELETE FROM playlists WHERE id = ?1", [&playlist_id])?;

    Ok(())
}

/// Add songs to a playlist on server and refresh local cache
#[tauri::command]
pub async fn add_songs_to_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    song_ids: Vec<String>,
) -> AppResult<()> {
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
    state: State<'_, AppState>,
    playlist_id: String,
    position: i32,
) -> AppResult<()> {
    remove_playlist_positions(&state, &playlist_id, vec![position as i64]).await
}

/// Remove multiple songs from a playlist by position on server and refresh local cache
#[tauri::command]
pub async fn remove_songs_from_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    positions: Vec<i32>,
) -> AppResult<()> {
    let mut positions_to_remove: Vec<i64> = positions
        .into_iter()
        .filter(|position| *position >= 0)
        .map(i64::from)
        .collect();
    positions_to_remove.sort_unstable();
    positions_to_remove.dedup();

    remove_playlist_positions(&state, &playlist_id, positions_to_remove).await
}

async fn remove_playlist_positions(
    state: &State<'_, AppState>,
    playlist_id: &str,
    positions: Vec<i64>,
) -> AppResult<()> {
    if positions.is_empty() {
        return Ok(());
    }

    // Remove by index on server
    state
        .client
        .update_playlist(playlist_id, None, vec![], positions)
        .await?;

    // Re-fetch playlist from server and update local cache
    refresh_playlist_cache(state, playlist_id).await?;

    Ok(())
}

/// Re-fetch a single playlist from server and update local cache
async fn refresh_playlist_cache(state: &State<'_, AppState>, playlist_id: &str) -> AppResult<()> {
    let detail = state.client.get_playlist(playlist_id).await?;
    let info = &detail.info;
    let now = chrono::Utc::now().to_rfc3339();

    let db = state.db.lock_recover();

    // Update playlist metadata
    db.execute(
        "INSERT OR REPLACE INTO playlists (id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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

#[cfg(test)]
mod tests {
    use super::playlist_song_ids_to_add;
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
}
