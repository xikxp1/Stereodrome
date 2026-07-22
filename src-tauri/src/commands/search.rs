use stereodrome_core::{CoreCommand, SearchResults};
use tauri::State;

use crate::error::AppResult;
use crate::runtime::dispatch;
use crate::state::AppState;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn search_library(
    state: State<'_, AppState>,
    query: String,
    limit: Option<i32>,
) -> AppResult<SearchResults> {
    let limit = limit.map(|value| usize::try_from(value.max(0)).unwrap_or(usize::MAX));
    dispatch(&state, CoreCommand::SearchLibrary { query, limit })
}
