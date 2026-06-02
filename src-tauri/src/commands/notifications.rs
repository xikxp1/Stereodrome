use tauri::AppHandle;

use crate::error::AppResult;

#[tauri::command]
pub async fn send_now_playing_notification(
    app_handle: AppHandle,
    title: String,
    body: String,
    cover_art_path: Option<String>,
) -> AppResult<bool> {
    #[cfg(target_os = "windows")]
    {
        return send_windows_now_playing_notification(
            &app_handle,
            &title,
            &body,
            cover_art_path.as_deref(),
        );
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, title, body, cover_art_path);
        Ok(false)
    }
}

#[cfg(target_os = "windows")]
fn send_windows_now_playing_notification(
    app_handle: &AppHandle,
    title: &str,
    body: &str,
    cover_art_path: Option<&str>,
) -> AppResult<bool> {
    use std::path::Path;

    use log::warn;
    use tauri_winrt_notification::{Duration, IconCrop, Toast};

    use crate::error::AppError;

    let app_id = if tauri::is_dev() {
        Toast::POWERSHELL_APP_ID
    } else {
        app_handle.config().identifier.as_str()
    };

    let mut toast = Toast::new(app_id)
        .title(title)
        .text1(body)
        .duration(Duration::Short);

    if let Some(path) = cover_art_path.filter(|path| !path.is_empty()) {
        let image_path = Path::new(path);
        if image_path.exists() {
            toast = toast.icon(image_path, IconCrop::Square, "Album artwork");
        } else {
            warn!("Skipping notification cover art because file does not exist: {path}");
        }
    }

    toast
        .show()
        .map_err(|e| AppError::Window(format!("Failed to send Windows notification: {e}")))?;

    Ok(true)
}
