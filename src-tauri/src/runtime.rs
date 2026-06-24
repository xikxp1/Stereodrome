#[cfg(feature = "cef")]
pub type Runtime = tauri::Cef;

#[cfg(all(not(feature = "cef"), feature = "wry"))]
pub type Runtime = tauri::Wry;

#[cfg(not(any(feature = "cef", feature = "wry")))]
compile_error!("enable either the `cef` or `wry` feature for the desktop Tauri runtime");

pub type App = tauri::App<Runtime>;
pub type AppHandle = tauri::AppHandle<Runtime>;
pub type WebviewWindow = tauri::WebviewWindow<Runtime>;
