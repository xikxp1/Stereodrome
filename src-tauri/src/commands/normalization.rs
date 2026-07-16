use stereodrome_desktop::DesktopBackend;
use tauri::State;

use crate::error::AppResult;

pub use stereodrome_desktop::operations::normalization::{AnalysisProgress, NormalizationStats};

#[tauri::command]
pub fn get_analysis_progress(backend: State<'_, DesktopBackend>) -> Option<AnalysisProgress> {
    stereodrome_desktop::operations::normalization::get_analysis_progress(&backend.state())
}

#[tauri::command]
pub fn get_normalization_stats(
    backend: State<'_, DesktopBackend>,
) -> AppResult<NormalizationStats> {
    stereodrome_desktop::operations::normalization::get_normalization_stats(&backend.state())
}

#[tauri::command]
pub async fn analyze_all_songs(backend: State<'_, DesktopBackend>) -> AppResult<()> {
    stereodrome_desktop::operations::normalization::analyze_all_songs(
        &backend.runtime_handle(),
        backend.state(),
    )
}

#[tauri::command]
pub fn clear_normalization_data(backend: State<'_, DesktopBackend>) -> AppResult<()> {
    stereodrome_desktop::operations::normalization::clear_normalization_data(&backend.state())
}
