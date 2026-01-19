use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub album_count: i32,
    pub cover_art_id: Option<String>,
    pub synced_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub artist_id: String,
    pub name: String,
    pub year: Option<i32>,
    pub song_count: i32,
    pub duration: Option<i32>,
    pub cover_art_id: Option<String>,
    pub synced_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Song {
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
    // Joined fields
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub artists: usize,
    pub albums: usize,
    pub songs: usize,
}

// Internal structs for collecting data before writing to DB
struct ArtistData {
    id: String,
    name: String,
    album_count: i32,
    cover_art: Option<String>,
}

struct AlbumData {
    id: String,
    artist_id: String,
    name: String,
    year: Option<i32>,
    song_count: i32,
    duration: Option<i32>,
    cover_art: Option<String>,
}

struct SongData {
    id: String,
    album_id: String,
    artist_id: String,
    title: String,
    track: Option<i32>,
    disc_number: i32,
    duration: Option<i32>,
    bit_rate: Option<i32>,
    size: Option<i64>,
    suffix: Option<String>,
    content_type: Option<String>,
    path: Option<String>,
    year: Option<i32>,
    genre: Option<String>,
}

#[tauri::command]
pub async fn sync_library(state: State<'_, AppState>) -> AppResult<SyncResult> {
    let client = {
        let lock = state.client.lock().unwrap();
        lock.clone().ok_or(AppError::NotConnected)?
    };

    // Fetch all data from server first (no DB lock held)
    let indexes = client
        .get_artists(None)
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    let mut artists_data: Vec<ArtistData> = Vec::new();
    let mut albums_data: Vec<AlbumData> = Vec::new();
    let mut songs_data: Vec<SongData> = Vec::new();

    eprintln!("Fetched {} indexes", indexes.len());

    for index in indexes {
        eprintln!("Index has {} artists", index.artist.len());
        for artist in index.artist {
            artists_data.push(ArtistData {
                id: artist.id.clone(),
                name: artist.name.clone(),
                album_count: artist.album_count,
                cover_art: artist.cover_art.clone(),
            });

            // Fetch albums for this artist
            match client.get_artist(&artist.id).await {
                Ok(artist_detail) => {
                    for album in artist_detail.album {
                        albums_data.push(AlbumData {
                            id: album.id.clone(),
                            artist_id: artist.id.clone(),
                            name: album.name.clone(),
                            year: album.year,
                            song_count: album.song_count,
                            duration: Some(album.duration),
                            cover_art: album.cover_art.clone(),
                        });

                        // Fetch songs for this album
                        match client.get_album(&album.id).await {
                            Ok(album_detail) => {
                                for song in album_detail.song {
                                    songs_data.push(SongData {
                                        id: song.id.clone(),
                                        album_id: album.id.clone(),
                                        artist_id: artist.id.clone(),
                                        title: song.title.clone(),
                                        track: song.track,
                                        disc_number: song.disc_number.unwrap_or(1),
                                        duration: song.duration,
                                        bit_rate: song.bit_rate,
                                        size: song.size,
                                        suffix: song.suffix.clone(),
                                        content_type: song.content_type.clone(),
                                        path: song.path.clone(),
                                        year: song.year.or(album.year),
                                        genre: song.genre.clone(),
                                    });
                                }
                            }
                            Err(e) => eprintln!("Error fetching album {}: {}", album.id, e),
                        }
                    }
                }
                Err(e) => eprintln!("Error fetching artist {}: {}", artist.id, e),
            }
        }
    }

    // Now write all data to DB (short lock duration, no await)
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    for artist in &artists_data {
        db.execute(
            "INSERT OR REPLACE INTO artists (id, name, album_count, cover_art_id, synced_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![artist.id, artist.name, artist.album_count, artist.cover_art, &now],
        )?;
    }

    for album in &albums_data {
        db.execute(
            "INSERT OR REPLACE INTO albums (id, artist_id, name, year, song_count, duration, cover_art_id, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![album.id, album.artist_id, album.name, album.year, album.song_count, album.duration, album.cover_art, &now],
        )?;
    }

    for song in &songs_data {
        db.execute(
            "INSERT OR REPLACE INTO songs (id, album_id, artist_id, title, track_number, disc_number, duration, bit_rate, size, suffix, content_type, path, year, genre, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![song.id, song.album_id, song.artist_id, song.title, song.track, song.disc_number, song.duration, song.bit_rate, song.size, song.suffix, song.content_type, song.path, song.year, song.genre, &now],
        )?;
    }

    eprintln!(
        "Sync complete: {} artists, {} albums, {} songs",
        artists_data.len(),
        albums_data.len(),
        songs_data.len()
    );

    Ok(SyncResult {
        artists: artists_data.len(),
        albums: albums_data.len(),
        songs: songs_data.len(),
    })
}

#[tauri::command]
pub async fn get_artists(state: State<'_, AppState>) -> AppResult<Vec<Artist>> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, name, album_count, cover_art_id, synced_at FROM artists ORDER BY name",
    )?;

    let artists: Vec<Artist> = stmt
        .query_map([], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                album_count: row.get(2)?,
                cover_art_id: row.get(3)?,
                synced_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(artists)
}

#[tauri::command]
pub async fn get_albums(
    state: State<'_, AppState>,
    artist_id: Option<String>,
) -> AppResult<Vec<Album>> {
    let db = state.db.lock().unwrap();

    let albums: Vec<Album> = if let Some(aid) = artist_id {
        let mut stmt = db.prepare(
            "SELECT id, artist_id, name, year, song_count, duration, cover_art_id, synced_at FROM albums WHERE artist_id = ?1 ORDER BY year, name",
        )?;
        let result: Vec<Album> = stmt
            .query_map([aid], |row| {
                Ok(Album {
                    id: row.get(0)?,
                    artist_id: row.get(1)?,
                    name: row.get(2)?,
                    year: row.get(3)?,
                    song_count: row.get(4)?,
                    duration: row.get(5)?,
                    cover_art_id: row.get(6)?,
                    synced_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        result
    } else {
        let mut stmt = db.prepare(
            "SELECT id, artist_id, name, year, song_count, duration, cover_art_id, synced_at FROM albums ORDER BY name",
        )?;
        let result: Vec<Album> = stmt
            .query_map([], |row| {
                Ok(Album {
                    id: row.get(0)?,
                    artist_id: row.get(1)?,
                    name: row.get(2)?,
                    year: row.get(3)?,
                    song_count: row.get(4)?,
                    duration: row.get(5)?,
                    cover_art_id: row.get(6)?,
                    synced_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        result
    };

    Ok(albums)
}

#[tauri::command]
pub async fn get_songs(
    state: State<'_, AppState>,
    album_id: Option<String>,
) -> AppResult<Vec<Song>> {
    let db = state.db.lock().unwrap();

    let base_query = "SELECT s.id, s.album_id, s.artist_id, s.title, s.track_number, s.disc_number,
                      s.duration, s.bit_rate, s.size, s.suffix, s.content_type, s.path,
                      s.year, s.genre, s.synced_at, ar.name as artist_name, al.name as album_name
                      FROM songs s
                      LEFT JOIN artists ar ON s.artist_id = ar.id
                      LEFT JOIN albums al ON s.album_id = al.id";

    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<Song> {
        Ok(Song {
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
            artist: row.get(15)?,
            album: row.get(16)?,
        })
    };

    let songs: Vec<Song> = if let Some(aid) = album_id {
        let query = format!(
            "{} WHERE s.album_id = ?1 ORDER BY s.disc_number, s.track_number",
            base_query
        );
        let mut stmt = db.prepare(&query)?;
        let result: Vec<Song> = stmt
            .query_map([aid], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        result
    } else {
        let query = format!(
            "{} ORDER BY ar.name COLLATE NOCASE, al.name COLLATE NOCASE, s.disc_number, s.track_number",
            base_query
        );
        let mut stmt = db.prepare(&query)?;
        let result: Vec<Song> = stmt
            .query_map([], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        result
    };

    Ok(songs)
}
