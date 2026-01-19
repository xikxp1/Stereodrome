use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
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
    let limit = limit.unwrap_or(20) as usize;

    let search_index_guard = state
        .search_index
        .lock()
        .map_err(|e| AppError::Search(format!("Failed to lock search index: {}", e)))?;

    let index_manager = search_index_guard.as_ref().ok_or_else(|| {
        AppError::Search("Search index not initialized. Please sync library first.".to_string())
    })?;

    // Multiply limit to get enough results for each category
    let hits = index_manager.search(&query, limit * 3)?;

    eprintln!(
        "search_library: query='{}', limit={}, hits={}",
        query,
        limit,
        hits.len()
    );

    let mut songs = Vec::new();
    let mut albums = Vec::new();
    let mut artists = Vec::new();

    for hit in hits {
        match hit.entity_type.as_str() {
            "song" => {
                if songs.len() < limit {
                    songs.push(SearchResultSong {
                        id: hit.id,
                        title: hit.name,
                        artist: hit.artist_name,
                        album: hit.album_name,
                        duration: hit.duration.map(|d| d as i32),
                    });
                }
            }
            "album" => {
                if albums.len() < limit {
                    albums.push(SearchResultAlbum {
                        id: hit.id,
                        name: hit.name,
                        artist: hit.artist_name,
                        year: hit.year.map(|y| y as i32),
                        song_count: hit.song_count.unwrap_or(0) as i32,
                    });
                }
            }
            "artist" => {
                if artists.len() < limit {
                    artists.push(SearchResultArtist {
                        id: hit.id,
                        name: hit.name,
                        album_count: hit.album_count.unwrap_or(0) as i32,
                    });
                }
            }
            _ => {}
        }
    }

    eprintln!(
        "search_library: returning {} songs, {} albums, {} artists",
        songs.len(),
        albums.len(),
        artists.len()
    );

    Ok(SearchResults {
        songs,
        albums,
        artists,
    })
}
