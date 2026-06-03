use tauri::AppHandle;
#[cfg(target_os = "windows")]
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg(target_os = "windows")]
use crate::error::AppError;
use crate::error::AppResult;

#[cfg(target_os = "windows")]
const TASKBAR_WIDGET_LABEL: &str = "taskbar-widget";
#[cfg(target_os = "windows")]
const TASKBAR_WIDGET_TITLE: &str = "Stereodrome Taskbar Widget";
#[cfg(target_os = "windows")]
const TASKBAR_WIDGET_URL: &str = "/taskbar-widget";
#[cfg(target_os = "windows")]
const TASKBAR_WIDGET_WIDTH: f64 = 360.0;
#[cfg(target_os = "windows")]
const TASKBAR_WIDGET_HEIGHT: f64 = 40.0;

#[cfg(target_os = "windows")]
pub fn open(app_handle: &AppHandle) -> AppResult<()> {
    let window = if let Some(window) = app_handle.get_webview_window(TASKBAR_WIDGET_LABEL) {
        window
    } else {
        WebviewWindowBuilder::new(
            app_handle,
            TASKBAR_WIDGET_LABEL,
            WebviewUrl::App(TASKBAR_WIDGET_URL.into()),
        )
        .title(TASKBAR_WIDGET_TITLE)
        .inner_size(TASKBAR_WIDGET_WIDTH, TASKBAR_WIDGET_HEIGHT)
        .min_inner_size(TASKBAR_WIDGET_WIDTH, TASKBAR_WIDGET_HEIGHT)
        .max_inner_size(TASKBAR_WIDGET_WIDTH, TASKBAR_WIDGET_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(false)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()
        .map_err(|e| AppError::Window(format!("failed to create taskbar widget: {e}")))?
    };

    platform::attach_and_position(&window)?;
    window
        .show()
        .map_err(|e| AppError::Window(format!("failed to show taskbar widget: {e}")))?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn open(_app_handle: &AppHandle) -> AppResult<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn close(app_handle: &AppHandle) -> AppResult<()> {
    if let Some(window) = app_handle.get_webview_window(TASKBAR_WIDGET_LABEL) {
        window
            .close()
            .map_err(|e| AppError::Window(format!("failed to close taskbar widget: {e}")))?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn close(_app_handle: &AppHandle) -> AppResult<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn reposition(app_handle: &AppHandle) -> AppResult<()> {
    if let Some(window) = app_handle.get_webview_window(TASKBAR_WIDGET_LABEL) {
        platform::attach_and_position(&window)?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn reposition(_app_handle: &AppHandle) -> AppResult<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
mod platform {
    use tauri::WebviewWindow;
    use windows::{
        Win32::{
            Foundation::{HWND, POINT, RECT},
            Graphics::Gdi::{CreateRectRgn, DeleteObject, HGDIOBJ, ScreenToClient, SetWindowRgn},
            UI::WindowsAndMessaging::{
                FindWindowW, GWL_STYLE, GetWindowLongPtrW, GetWindowRect, SWP_ASYNCWINDOWPOS,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetParent,
                SetWindowLongPtrW, SetWindowPos, WS_CHILD, WS_POPUP,
            },
        },
        core::w,
    };

    use crate::error::{AppError, AppResult};

    const TASKBAR_MARGIN: i32 = 20;
    const WIDGET_WIDTH: i32 = super::TASKBAR_WIDGET_WIDTH as i32;
    const WIDGET_HEIGHT: i32 = super::TASKBAR_WIDGET_HEIGHT as i32;

    pub fn attach_and_position(window: &WebviewWindow) -> AppResult<()> {
        let taskbar_hwnd = find_primary_taskbar()?;
        let widget_hwnd = widget_hwnd(window)?;
        attach_to_taskbar(widget_hwnd, taskbar_hwnd);
        position_in_taskbar(widget_hwnd, taskbar_hwnd)
    }

    fn widget_hwnd(window: &WebviewWindow) -> AppResult<HWND> {
        let hwnd = window
            .hwnd()
            .map_err(|e| AppError::Window(format!("failed to get taskbar widget HWND: {e}")))?;
        Ok(HWND(hwnd.0 as _))
    }

    fn find_primary_taskbar() -> AppResult<HWND> {
        unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }
            .map_err(|e| AppError::Window(format!("failed to find primary taskbar: {e}")))
    }

    fn attach_to_taskbar(widget_hwnd: HWND, taskbar_hwnd: HWND) {
        unsafe {
            let style = GetWindowLongPtrW(widget_hwnd, GWL_STYLE);
            let next_style = (style & !(WS_POPUP.0 as isize)) | WS_CHILD.0 as isize;
            if next_style != style {
                SetWindowLongPtrW(widget_hwnd, GWL_STYLE, next_style);
            }
            let _ = SetParent(widget_hwnd, Some(taskbar_hwnd));
        }
    }

    fn position_in_taskbar(widget_hwnd: HWND, taskbar_hwnd: HWND) -> AppResult<()> {
        let mut taskbar_rect = RECT::default();
        unsafe { GetWindowRect(taskbar_hwnd, &mut taskbar_rect) }
            .map_err(|e| AppError::Window(format!("failed to read taskbar bounds: {e}")))?;

        let taskbar_width = taskbar_rect.right - taskbar_rect.left;
        let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;
        if taskbar_width <= 0 || taskbar_height <= 0 {
            return Err(AppError::Window(
                "taskbar bounds are empty; cannot position widget".to_string(),
            ));
        }

        let width = WIDGET_WIDTH.min((taskbar_width - TASKBAR_MARGIN * 2).max(WIDGET_HEIGHT));
        let height = WIDGET_HEIGHT.min(taskbar_height.max(WIDGET_HEIGHT));
        let mut origin = POINT {
            x: taskbar_rect.left,
            y: taskbar_rect.top,
        };
        unsafe {
            if !ScreenToClient(taskbar_hwnd, &mut origin).as_bool() {
                origin = POINT { x: 0, y: 0 };
            }
        }

        let x = origin.x + TASKBAR_MARGIN;
        let y = origin.y + ((taskbar_height - height) / 2).max(0);

        unsafe {
            SetWindowPos(
                widget_hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOZORDER
                    | SWP_NOACTIVATE
                    | SWP_ASYNCWINDOWPOS
                    | SWP_SHOWWINDOW
                    | SWP_FRAMECHANGED,
            )
            .map_err(|e| AppError::Window(format!("failed to position taskbar widget: {e}")))?;

            let region = CreateRectRgn(0, 0, width, height);
            if region.is_invalid() {
                return Err(AppError::Window(
                    "failed to create taskbar widget window region".to_string(),
                ));
            }
            if SetWindowRgn(widget_hwnd, Some(region), true) == 0 {
                let _ = DeleteObject(HGDIOBJ(region.0));
                return Err(AppError::Window(
                    "failed to apply taskbar widget window region".to_string(),
                ));
            }
        }

        Ok(())
    }
}
