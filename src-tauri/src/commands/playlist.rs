use stereodrome_desktop::DesktopBackend;
use stereodrome_desktop::operations::playlist::{
    Playlist, PlaylistSong, SavedPlaylistOfflineResult,
};
use tauri::State;

use crate::error::AppResult;

#[tauri::command]
pub async fn sync_playlists(backend: State<'_, DesktopBackend>) -> AppResult<i32> {
    stereodrome_desktop::operations::playlist::sync_playlists(backend.state()).await
}

#[tauri::command]
pub fn get_playlists(backend: State<'_, DesktopBackend>) -> AppResult<Vec<Playlist>> {
    stereodrome_desktop::operations::playlist::get_playlists(&backend.state())
}

#[tauri::command]
pub fn get_playlist_songs(
    backend: State<'_, DesktopBackend>,
    playlist_id: String,
) -> AppResult<Vec<PlaylistSong>> {
    stereodrome_desktop::operations::playlist::get_playlist_songs(&backend.state(), playlist_id)
}

#[tauri::command]
pub async fn create_playlist(
    backend: State<'_, DesktopBackend>,
    name: String,
    song_ids: Option<Vec<String>>,
) -> AppResult<Playlist> {
    stereodrome_desktop::operations::playlist::create_playlist(backend.state(), name, song_ids)
        .await
}

#[tauri::command]
pub async fn update_playlist(
    backend: State<'_, DesktopBackend>,
    playlist_id: String,
    name: String,
) -> AppResult<()> {
    stereodrome_desktop::operations::playlist::update_playlist(backend.state(), playlist_id, name)
        .await
}

#[tauri::command]
pub async fn delete_playlist(
    backend: State<'_, DesktopBackend>,
    playlist_id: String,
) -> AppResult<()> {
    stereodrome_desktop::operations::playlist::delete_playlist(backend.state(), playlist_id).await
}

#[tauri::command]
pub async fn add_songs_to_playlist(
    backend: State<'_, DesktopBackend>,
    playlist_id: String,
    song_ids: Vec<String>,
) -> AppResult<()> {
    stereodrome_desktop::operations::playlist::add_songs_to_playlist(
        backend.state(),
        playlist_id,
        song_ids,
    )
    .await
}

#[tauri::command]
pub async fn remove_song_from_playlist(
    backend: State<'_, DesktopBackend>,
    playlist_id: String,
    position: i32,
) -> AppResult<()> {
    stereodrome_desktop::operations::playlist::remove_song_from_playlist(
        backend.state(),
        playlist_id,
        position,
    )
    .await
}

#[tauri::command]
pub async fn remove_songs_from_playlist(
    backend: State<'_, DesktopBackend>,
    playlist_id: String,
    positions: Vec<i32>,
) -> AppResult<()> {
    stereodrome_desktop::operations::playlist::remove_songs_from_playlist(
        backend.state(),
        playlist_id,
        positions,
    )
    .await
}

#[tauri::command]
pub async fn set_playlist_saved_offline(
    backend: State<'_, DesktopBackend>,
    playlist_id: String,
    saved_offline: bool,
) -> AppResult<SavedPlaylistOfflineResult> {
    stereodrome_desktop::operations::playlist::set_playlist_saved_offline(
        backend.state(),
        playlist_id,
        saved_offline,
    )
    .await
}

#[tauri::command]
pub async fn reconcile_saved_playlists_offline(
    backend: State<'_, DesktopBackend>,
) -> AppResult<Vec<SavedPlaylistOfflineResult>> {
    stereodrome_desktop::operations::playlist::reconcile_saved_playlists_offline(backend.state())
        .await
}
