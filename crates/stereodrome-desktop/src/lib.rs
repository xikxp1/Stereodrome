//! Native desktop backend primitives shared by the Tauri and GPUI shells.

mod backend;
mod error;
mod paths;
mod store;

pub use backend::{DesktopBackend, WorkerHandle};
pub use error::DesktopError;
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
