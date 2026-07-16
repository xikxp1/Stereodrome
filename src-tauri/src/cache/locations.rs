use std::path::PathBuf;

use stereodrome_desktop::DesktopBackend;
use tauri::{AppHandle, Manager};

use crate::error::AppResult;

pub fn cover_cache_dir(app_handle: &AppHandle) -> AppResult<PathBuf> {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::cache::cover_cache_dir(backend.paths(), backend.settings())
}
