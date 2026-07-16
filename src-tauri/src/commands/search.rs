use stereodrome_desktop::DesktopBackend;
use tauri::State;

use crate::error::AppResult;

use stereodrome_desktop::operations::search::SearchResults;

#[tauri::command]
pub fn search_library(
    backend: State<'_, DesktopBackend>,
    query: String,
    limit: Option<i32>,
) -> AppResult<SearchResults> {
    stereodrome_desktop::operations::search::search_library(&backend.state(), query, limit)
}
