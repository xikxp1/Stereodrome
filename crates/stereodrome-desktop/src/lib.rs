//! Desktop backend boundary. Product code moves here after Phase 0.

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
