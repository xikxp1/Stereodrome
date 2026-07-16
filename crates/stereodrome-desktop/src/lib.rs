//! Native desktop backend primitives shared by the Tauri and GPUI shells.

pub mod audio;
mod backend;
pub mod cache;
pub mod client;
pub mod credentials;
pub mod db;
mod error;
pub mod events;
pub mod lastfm;
pub mod operations;
mod paths;
pub mod search;
pub mod state;
mod store;

pub use backend::{DesktopBackend, WorkerHandle};
pub use error::{AppError, AppResult, DesktopError, MutexExt};
pub use events::{DesktopEvent, DesktopEvents};
pub use paths::DesktopPaths;
pub use store::JsonStore;

/// Stable desktop application identifier shared with the shipping Tauri app.
pub const APPLICATION_ID: &str = "dev.xikxp1.stereodrome";

#[cfg(test)]
mod tests {
    use super::APPLICATION_ID;

    #[test]
    fn preserves_application_identity() {
        assert_eq!(APPLICATION_ID, "dev.xikxp1.stereodrome");
    }
}
