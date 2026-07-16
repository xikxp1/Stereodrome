use stereodrome_desktop::DesktopBackend;
use tauri::State;

use crate::error::AppResult;

#[tauri::command]
pub async fn get_cover_art(
    backend: State<'_, DesktopBackend>,
    cover_art_id: String,
    size: Option<i32>,
) -> AppResult<String> {
    stereodrome_desktop::operations::cover_art::get_cover_art(&backend.state(), cover_art_id, size)
        .await
}

#[tauri::command]
pub async fn get_song_cover_art(
    backend: State<'_, DesktopBackend>,
    song_id: String,
    size: Option<i32>,
) -> AppResult<Option<String>> {
    stereodrome_desktop::operations::cover_art::get_song_cover_art(&backend.state(), song_id, size)
        .await
}

#[tauri::command]
pub async fn get_cover_art_path(
    backend: State<'_, DesktopBackend>,
    cover_art_id: String,
    size: Option<i32>,
) -> AppResult<String> {
    stereodrome_desktop::operations::cover_art::get_cover_art_path(
        &backend.state(),
        cover_art_id,
        size,
    )
    .await
}
