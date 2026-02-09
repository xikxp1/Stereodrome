use log::{debug, warn};
use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, WebviewUrl};
#[cfg(target_os = "macos")]
use tauri::{Emitter, LogicalPosition, LogicalSize, Position, Size};

use crate::error::{AppError, AppResult};

const MAIN_WINDOW_LABEL: &str = "main";
const MINI_PLAYER_LABEL: &str = "mini-player";
const MINI_PLAYER_TITLE: &str = "Stereodrome Mini Player";
const MINI_PLAYER_URL: &str = "/mini-player";
const MINI_PLAYER_WIDTH: f64 = 320.0;
const MINI_PLAYER_HEIGHT: f64 = 72.0;
#[cfg(target_os = "macos")]
const MINI_PLAYER_HOVER_EVENT: &str = "mini-player-hover-state";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MiniPlayerPosition {
    pub x: f64,
    pub y: f64,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, Serialize)]
struct MiniPlayerHoverState {
    hovered: bool,
}

fn minimize_main_window(app_handle: &AppHandle) -> AppResult<()> {
    let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) else {
        warn!("Main window not found while minimizing");
        return Ok(());
    };

    window
        .minimize()
        .map_err(|e| AppError::Window(format!("failed to minimize main window: {e}")))
}

fn restore_main_window_impl(app_handle: &AppHandle) -> AppResult<()> {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;
        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }
    }

    let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) else {
        warn!("Main window not found while restoring");
        return Ok(());
    };

    let is_minimized = window
        .is_minimized()
        .map_err(|e| AppError::Window(format!("failed to read main window state: {e}")))?;
    if is_minimized {
        window
            .unminimize()
            .map_err(|e| AppError::Window(format!("failed to unminimize main window: {e}")))?;
    }

    window
        .show()
        .map_err(|e| AppError::Window(format!("failed to show main window: {e}")))?;
    window
        .set_focus()
        .map_err(|e| AppError::Window(format!("failed to focus main window: {e}")))?;

    Ok(())
}

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(MiniPlayerPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            becomes_key_only_if_needed: true,
            is_floating_panel: true,
            hides_on_deactivate: false
        }
        with: {
            tracking_area: {
                options: tauri_nspanel::TrackingAreaOptions::new()
                    .active_always()
                    .mouse_entered_and_exited()
                    .mouse_moved(),
                auto_resize: true
            }
        }
    })

    panel_event!(MiniPlayerPanelEventHandler {})
}

#[cfg(target_os = "macos")]
fn create_mini_player_window(
    app_handle: &AppHandle,
    position: MiniPlayerPosition,
) -> AppResult<()> {
    use tauri_nspanel::{CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

    let panel = PanelBuilder::<_, MiniPlayerPanel>::new(app_handle, MINI_PLAYER_LABEL)
        .url(WebviewUrl::App(MINI_PLAYER_URL.into()))
        .title(MINI_PLAYER_TITLE)
        .position(Position::Logical(LogicalPosition::new(
            position.x, position.y,
        )))
        .size(Size::Logical(LogicalSize::new(
            MINI_PLAYER_WIDTH,
            MINI_PLAYER_HEIGHT,
        )))
        .level(PanelLevel::Floating)
        .floating(true)
        .becomes_key_only_if_needed(true)
        .accepts_mouse_moved_events(true)
        .hides_on_deactivate(false)
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .collection_behavior(
            CollectionBehavior::new()
                .full_screen_auxiliary()
                .can_join_all_spaces(),
        )
        .with_window(|window| {
            window
                .inner_size(MINI_PLAYER_WIDTH, MINI_PLAYER_HEIGHT)
                .min_inner_size(MINI_PLAYER_WIDTH, MINI_PLAYER_HEIGHT)
                .max_inner_size(MINI_PLAYER_WIDTH, MINI_PLAYER_HEIGHT)
                .resizable(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .accept_first_mouse(true)
        })
        .build()
        .map_err(|e| AppError::Window(format!("failed to create mini player panel: {e}")))?;

    let handler = MiniPlayerPanelEventHandler::new();

    let entered_handle = app_handle.clone();
    handler.on_mouse_entered(move |_event| {
        let _ = entered_handle.emit(
            MINI_PLAYER_HOVER_EVENT,
            MiniPlayerHoverState { hovered: true },
        );
    });

    let exited_handle = app_handle.clone();
    handler.on_mouse_exited(move |_event| {
        let _ = exited_handle.emit(
            MINI_PLAYER_HOVER_EVENT,
            MiniPlayerHoverState { hovered: false },
        );
    });

    panel.set_event_handler(Some(handler.as_ref()));
    panel.show();

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn create_mini_player_window(
    app_handle: &AppHandle,
    position: MiniPlayerPosition,
) -> AppResult<()> {
    WebviewWindowBuilder::new(
        app_handle,
        MINI_PLAYER_LABEL,
        WebviewUrl::App(MINI_PLAYER_URL.into()),
    )
    .title(MINI_PLAYER_TITLE)
    .inner_size(MINI_PLAYER_WIDTH, MINI_PLAYER_HEIGHT)
    .min_inner_size(MINI_PLAYER_WIDTH, MINI_PLAYER_HEIGHT)
    .max_inner_size(MINI_PLAYER_WIDTH, MINI_PLAYER_HEIGHT)
    .position(position.x, position.y)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .accept_first_mouse(true)
    .build()
    .map_err(|e| AppError::Window(format!("failed to create mini player window: {e}")))?;

    Ok(())
}

#[tauri::command]
pub fn open_mini_player(app_handle: AppHandle, position: MiniPlayerPosition) -> AppResult<()> {
    if let Some(window) = app_handle.get_webview_window(MINI_PLAYER_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        #[cfg(target_os = "macos")]
        {
            use tauri_nspanel::ManagerExt;
            if let Ok(panel) = app_handle.get_webview_panel(MINI_PLAYER_LABEL) {
                panel.show();
                panel.order_front_regardless();
            } else {
                let _ = window.show();
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = window.set_focus();
        }
        debug!("Mini player already exists; focusing existing window");
        minimize_main_window(&app_handle)?;
        return Ok(());
    }

    create_mini_player_window(&app_handle, position)?;
    minimize_main_window(&app_handle)?;
    debug!("Mini player opened");

    Ok(())
}

#[tauri::command]
pub fn close_mini_player(app_handle: AppHandle) -> AppResult<()> {
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::ManagerExt;

        if let Ok(panel) = app_handle.get_webview_panel(MINI_PLAYER_LABEL) {
            panel.hide();
        } else if let Some(window) = app_handle.get_webview_window(MINI_PLAYER_LABEL) {
            window
                .hide()
                .map_err(|e| AppError::Window(format!("failed to hide mini player: {e}")))?;
        } else {
            warn!("Mini player close requested but window not found");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(window) = app_handle.get_webview_window(MINI_PLAYER_LABEL) {
            window
                .close()
                .map_err(|e| AppError::Window(format!("failed to close mini player: {e}")))?;
        } else {
            warn!("Mini player close requested but window not found");
        }
    }

    restore_main_window_impl(&app_handle)?;
    Ok(())
}

#[tauri::command]
pub fn restore_main_window(app_handle: AppHandle) -> AppResult<()> {
    restore_main_window_impl(&app_handle)
}
