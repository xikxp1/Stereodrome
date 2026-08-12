use stereodrome_core::CoreCommand;
use tauri::State;

use crate::error::AppResult;
use crate::runtime::dispatch_value_async;
use crate::state::AppState;

/// Single entry point for runtime commands that need no desktop-specific work.
///
/// The frontend resolves payload types through the generated `CoreCommandValue`,
/// so adding a runtime command no longer requires a desktop wrapper.
#[tauri::command]
pub async fn core_dispatch(
    state: State<'_, AppState>,
    command: CoreCommand,
) -> AppResult<serde_json::Value> {
    dispatch_value_async(&state, command).await
}
