use serde::{Deserialize, Serialize};
use stereodrome_core::{CoreCommand, Playlist, SavedPlaylistOfflineResult, Song};
use tauri::State;

use crate::error::AppResult;
use crate::runtime::{dispatch_async, dispatch_unit_async};
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
pub async fn get_playlists(state: State<'_, AppState>) -> AppResult<Vec<Playlist>> {
    dispatch_async(&state, CoreCommand::GetPlaylists).await
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

#[tauri::command]
pub async fn create_playlist(
    state: State<'_, AppState>,
    name: String,
    song_ids: Option<Vec<String>>,
) -> AppResult<Playlist> {
    dispatch_async(
        &state,
        CoreCommand::CreatePlaylist {
            name,
            song_ids: song_ids.unwrap_or_default(),
        },
    )
    .await
}

#[tauri::command]
pub async fn update_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    name: String,
) -> AppResult<()> {
    dispatch_unit_async(&state, CoreCommand::RenamePlaylist { playlist_id, name }).await
}

#[tauri::command]
pub async fn delete_playlist(state: State<'_, AppState>, playlist_id: String) -> AppResult<()> {
    dispatch_unit_async(&state, CoreCommand::DeletePlaylist { playlist_id }).await
}

#[tauri::command]
pub async fn add_songs_to_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    song_ids: Vec<String>,
) -> AppResult<()> {
    dispatch_unit_async(
        &state,
        CoreCommand::AddSongsToPlaylist {
            playlist_id,
            song_ids,
        },
    )
    .await
}

#[tauri::command]
pub async fn remove_song_from_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    position: i32,
) -> AppResult<()> {
    remove_songs_from_playlist(state, playlist_id, vec![position]).await
}

#[tauri::command]
pub async fn remove_songs_from_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    positions: Vec<i32>,
) -> AppResult<()> {
    dispatch_unit_async(
        &state,
        CoreCommand::RemoveSongsFromPlaylist {
            playlist_id,
            song_indexes: positions.into_iter().map(i64::from).collect(),
        },
    )
    .await
}

#[tauri::command]
pub async fn set_playlist_saved_offline(
    state: State<'_, AppState>,
    playlist_id: String,
    saved_offline: bool,
) -> AppResult<SavedPlaylistOfflineResult> {
    dispatch_async(
        &state,
        CoreCommand::SetPlaylistSavedOffline {
            playlist_id,
            saved_offline,
        },
    )
    .await
}

#[tauri::command]
pub async fn reconcile_saved_playlists_offline(
    state: State<'_, AppState>,
) -> AppResult<Vec<SavedPlaylistOfflineResult>> {
    dispatch_async(&state, CoreCommand::ReconcileSavedPlaylistsOffline).await
}
