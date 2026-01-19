use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub song_count: i32,
    pub duration: i32,
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
    pub position: i32,
    // Joined fields
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[tauri::command]
pub fn get_playlists(state: State<'_, AppState>) -> AppResult<Vec<Playlist>> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, name, song_count, duration, created_at, changed_at FROM playlists ORDER BY name",
    )?;

    let playlists: Vec<Playlist> = stmt
        .query_map([], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                song_count: row.get(2)?,
                duration: row.get(3)?,
                created_at: row.get(4)?,
                changed_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(playlists)
}

#[tauri::command]
pub fn get_playlist_songs(
    state: State<'_, AppState>,
    playlist_id: String,
) -> AppResult<Vec<PlaylistSong>> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        r#"
        SELECT 
            s.id, s.album_id, s.artist_id, s.title, s.track_number, s.disc_number,
            s.duration, s.bit_rate, s.size, s.suffix, s.content_type, s.path,
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
                position: row.get(12)?,
                artist: row.get(13)?,
                album: row.get(14)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(songs)
}

#[tauri::command]
pub fn create_playlist(
    state: State<'_, AppState>,
    name: String,
    song_ids: Option<Vec<String>>,
) -> AppResult<Playlist> {
    let now = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();

    let db = state.db.lock().unwrap();

    // Create the playlist
    db.execute(
        "INSERT INTO playlists (id, name, song_count, duration, created_at, changed_at, synced_at) VALUES (?1, ?2, 0, 0, ?3, ?3, ?3)",
        rusqlite::params![&id, &name, &now],
    )?;

    // Add songs if provided
    if let Some(songs) = song_ids {
        for (position, song_id) in songs.iter().enumerate() {
            db.execute(
                "INSERT INTO playlist_songs (playlist_id, song_id, position) VALUES (?1, ?2, ?3)",
                rusqlite::params![&id, song_id, position as i32],
            )?;
        }

        // Update playlist stats
        update_playlist_stats(&db, &id)?;
    }

    // Fetch and return the created playlist
    let playlist = db.query_row(
        "SELECT id, name, song_count, duration, created_at, changed_at FROM playlists WHERE id = ?1",
        [&id],
        |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                song_count: row.get(2)?,
                duration: row.get(3)?,
                created_at: row.get(4)?,
                changed_at: row.get(5)?,
            })
        },
    )?;

    Ok(playlist)
}

#[tauri::command]
pub fn update_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    name: String,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    db.execute(
        "UPDATE playlists SET name = ?1, changed_at = ?2 WHERE id = ?3",
        rusqlite::params![&name, &now, &playlist_id],
    )?;

    Ok(())
}

#[tauri::command]
pub fn delete_playlist(state: State<'_, AppState>, playlist_id: String) -> AppResult<()> {
    let db = state.db.lock().unwrap();

    // Delete playlist songs first
    db.execute(
        "DELETE FROM playlist_songs WHERE playlist_id = ?1",
        [&playlist_id],
    )?;

    // Delete playlist
    db.execute("DELETE FROM playlists WHERE id = ?1", [&playlist_id])?;

    Ok(())
}

#[tauri::command]
pub fn add_songs_to_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    song_ids: Vec<String>,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    // Get current max position
    let max_position: i32 = db
        .query_row(
            "SELECT COALESCE(MAX(position), -1) FROM playlist_songs WHERE playlist_id = ?1",
            [&playlist_id],
            |row| row.get(0),
        )
        .unwrap_or(-1);

    // Add songs
    for (i, song_id) in song_ids.iter().enumerate() {
        // Skip if song already in playlist
        let exists: bool = db
            .query_row(
                "SELECT 1 FROM playlist_songs WHERE playlist_id = ?1 AND song_id = ?2",
                rusqlite::params![&playlist_id, song_id],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if !exists {
            let position = max_position + 1 + i as i32;
            db.execute(
                "INSERT INTO playlist_songs (playlist_id, song_id, position) VALUES (?1, ?2, ?3)",
                rusqlite::params![&playlist_id, song_id, position],
            )?;
        }
    }

    // Update stats
    update_playlist_stats(&db, &playlist_id)?;

    // Update changed_at
    db.execute(
        "UPDATE playlists SET changed_at = ?1 WHERE id = ?2",
        rusqlite::params![&now, &playlist_id],
    )?;

    Ok(())
}

#[tauri::command]
pub fn remove_song_from_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    song_id: String,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    db.execute(
        "DELETE FROM playlist_songs WHERE playlist_id = ?1 AND song_id = ?2",
        rusqlite::params![&playlist_id, &song_id],
    )?;

    // Reorder remaining songs
    reorder_playlist_positions(&db, &playlist_id)?;

    // Update stats
    update_playlist_stats(&db, &playlist_id)?;

    // Update changed_at
    db.execute(
        "UPDATE playlists SET changed_at = ?1 WHERE id = ?2",
        rusqlite::params![&now, &playlist_id],
    )?;

    Ok(())
}

#[tauri::command]
pub fn reorder_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    song_ids: Vec<String>,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    // Update positions for each song
    for (position, song_id) in song_ids.iter().enumerate() {
        db.execute(
            "UPDATE playlist_songs SET position = ?1 WHERE playlist_id = ?2 AND song_id = ?3",
            rusqlite::params![position as i32, &playlist_id, song_id],
        )?;
    }

    // Update changed_at
    db.execute(
        "UPDATE playlists SET changed_at = ?1 WHERE id = ?2",
        rusqlite::params![&now, &playlist_id],
    )?;

    Ok(())
}

// Helper function to update playlist song count and duration
fn update_playlist_stats(
    db: &rusqlite::Connection,
    playlist_id: &str,
) -> Result<(), rusqlite::Error> {
    db.execute(
        r#"
        UPDATE playlists SET
            song_count = (SELECT COUNT(*) FROM playlist_songs WHERE playlist_id = ?1),
            duration = (
                SELECT COALESCE(SUM(s.duration), 0)
                FROM playlist_songs ps
                JOIN songs s ON s.id = ps.song_id
                WHERE ps.playlist_id = ?1
            )
        WHERE id = ?1
        "#,
        [playlist_id],
    )?;
    Ok(())
}

// Helper function to reorder positions after deletion
fn reorder_playlist_positions(
    db: &rusqlite::Connection,
    playlist_id: &str,
) -> Result<(), rusqlite::Error> {
    // Get all song_ids in order
    let mut stmt =
        db.prepare("SELECT song_id FROM playlist_songs WHERE playlist_id = ?1 ORDER BY position")?;
    let song_ids: Vec<String> = stmt
        .query_map([playlist_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    // Update positions
    for (position, song_id) in song_ids.iter().enumerate() {
        db.execute(
            "UPDATE playlist_songs SET position = ?1 WHERE playlist_id = ?2 AND song_id = ?3",
            rusqlite::params![position as i32, playlist_id, song_id],
        )?;
    }

    Ok(())
}
