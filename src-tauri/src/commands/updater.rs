use crate::runtime::AppHandle;
use tauri::Manager;

use crate::tray::TrayManager;

/// Update the tray icon to show update availability
#[tauri::command]
pub async fn set_tray_update_available(app_handle: AppHandle, version: Option<String>) {
    if let Some(tray_manager) = app_handle.try_state::<TrayManager>() {
        tray_manager.update_update_available(version.as_deref());
    }
}
