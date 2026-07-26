use serde::{Deserialize, Serialize};
use stereodrome_core::{CoreCommand, Playlist, Song};
use tauri::State;

use crate::error::AppResult;
use crate::runtime::dispatch_async;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistSong {
    #[serde(flatten)]
    pub song: Song,
    pub position: i32,
}

#[tauri::command]
pub async fn sync_playlists(state: State<'_, AppState>) -> AppResult<i32> {
    let playlists: Vec<Playlist> = dispatch_async(&state, CoreCommand::GetPlaylists).await?;
    Ok(i32::try_from(playlists.len()).unwrap_or(i32::MAX))
}

#[tauri::command]
pub async fn get_playlist_songs(
    state: State<'_, AppState>,
    playlist_id: String,
) -> AppResult<Vec<PlaylistSong>> {
    let songs: Vec<Song> =
        dispatch_async(&state, CoreCommand::GetPlaylistSongs { playlist_id }).await?;
    Ok(songs
        .into_iter()
        .enumerate()
        .map(|(position, song)| PlaylistSong {
            song,
            position: i32::try_from(position).unwrap_or(i32::MAX),
        })
        .collect())
}
