use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultSong {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultAlbum {
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<i32>,
    pub song_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultArtist {
    pub id: String,
    pub name: String,
    pub album_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub songs: Vec<SearchResultSong>,
    pub albums: Vec<SearchResultAlbum>,
    pub artists: Vec<SearchResultArtist>,
}

#[tauri::command]
pub fn search_library(
    state: State<'_, AppState>,
    query: String,
    limit: Option<i32>,
) -> AppResult<SearchResults> {
    let db = state.db.lock().unwrap();
    let limit = limit.unwrap_or(20);
    let search_pattern = format!("%{}%", query);

    // Search songs
    let mut song_stmt = db.prepare(
        r#"
        SELECT 
            s.id, s.title, a.name as artist, al.name as album, s.duration
        FROM songs s
        LEFT JOIN artists a ON a.id = s.artist_id
        LEFT JOIN albums al ON al.id = s.album_id
        WHERE s.title LIKE ?1
           OR a.name LIKE ?1
           OR al.name LIKE ?1
        ORDER BY 
            CASE 
                WHEN s.title LIKE ?1 THEN 0
                ELSE 1
            END,
            s.title
        LIMIT ?2
        "#,
    )?;

    let songs: Vec<SearchResultSong> = song_stmt
        .query_map(rusqlite::params![&search_pattern, limit], |row| {
            Ok(SearchResultSong {
                id: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                duration: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Search albums
    let mut album_stmt = db.prepare(
        r#"
        SELECT 
            al.id, al.name, a.name as artist, al.year, al.song_count
        FROM albums al
        LEFT JOIN artists a ON a.id = al.artist_id
        WHERE al.name LIKE ?1
           OR a.name LIKE ?1
        ORDER BY 
            CASE 
                WHEN al.name LIKE ?1 THEN 0
                ELSE 1
            END,
            al.name
        LIMIT ?2
        "#,
    )?;

    let albums: Vec<SearchResultAlbum> = album_stmt
        .query_map(rusqlite::params![&search_pattern, limit], |row| {
            Ok(SearchResultAlbum {
                id: row.get(0)?,
                name: row.get(1)?,
                artist: row.get(2)?,
                year: row.get(3)?,
                song_count: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Search artists
    let mut artist_stmt = db.prepare(
        r#"
        SELECT id, name, album_count
        FROM artists
        WHERE name LIKE ?1
        ORDER BY name
        LIMIT ?2
        "#,
    )?;

    let artists: Vec<SearchResultArtist> = artist_stmt
        .query_map(rusqlite::params![&search_pattern, limit], |row| {
            Ok(SearchResultArtist {
                id: row.get(0)?,
                name: row.get(1)?,
                album_count: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SearchResults {
        songs,
        albums,
        artists,
    })
}
