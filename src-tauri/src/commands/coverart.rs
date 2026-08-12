use base64::{Engine, engine::general_purpose::STANDARD};
use stereodrome_core::CoreCommand;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::runtime::{dispatch_async, file_uri_path};
use crate::state::AppState;

#[tauri::command]
pub async fn get_cover_art(
    state: State<'_, AppState>,
    cover_art_id: String,
    size: Option<i32>,
) -> AppResult<String> {
    let uri: String = dispatch_async(
        &state,
        CoreCommand::GetCoverArtUri {
            id: cover_art_id,
            size,
        },
    )
    .await?;
    let bytes = std::fs::read(runtime_file_path(&uri)?)?;
    let mime = guess_mime_type(&bytes);
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

#[tauri::command]
pub async fn get_cover_art_path(
    state: State<'_, AppState>,
    cover_art_id: String,
    size: Option<i32>,
) -> AppResult<String> {
    let uri: String = dispatch_async(
        &state,
        CoreCommand::GetCoverArtUri {
            id: cover_art_id,
            size,
        },
    )
    .await?;
    Ok(runtime_file_path(&uri)?.to_string_lossy().into_owned())
}

fn runtime_file_path(uri: &str) -> AppResult<std::path::PathBuf> {
    file_uri_path(uri)
        .ok_or_else(|| AppError::Runtime("runtime returned a non-file cover URI".to_string()))
}

fn guess_mime_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.starts_with(b"GIF") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "image/jpeg"
    }
}
