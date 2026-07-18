use std::process::Command;

use cargo_packager_updater::{Config, Update, check_update, semver::Version, url::Url};

const ENDPOINT: &str = "https://github.com/xikxp1/Stereodrome/releases/latest/download/latest.json";
const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IENENEVBNkEzOTBBRjBDNjMKUldSakRLK1FvNlpPeld1bWhTU2pjVlNoTDlpMDl4dW5ML25HRkRBUmg3Tkw5NVNqNTVuT3UxaEsK";

pub fn check() -> Result<Option<Update>, String> {
    let version = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|error| format!("Invalid application version: {error}"))?;
    check_update(version, config()).map_err(|error| format!("Update check failed: {error}"))
}

pub fn install(update: Update) -> Result<(), String> {
    update
        .download_and_install()
        .map_err(|error| format!("Update installation failed: {error}"))
}

fn config() -> Config {
    Config {
        endpoints: vec![Url::parse(ENDPOINT).expect("static updater endpoint must be valid")],
        pubkey: PUBLIC_KEY.to_string(),
        ..Config::default()
    }
}

pub fn relaunch() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    let executable = std::env::var_os("APPIMAGE")
        .map(Into::into)
        .unwrap_or(std::env::current_exe()?);
    #[cfg(not(target_os = "linux"))]
    let executable = std::env::current_exe()?;
    Command::new(executable).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_uses_signed_release_manifest() {
        let config = config();
        assert_eq!(config.endpoints[0].as_str(), ENDPOINT);
        assert!(PUBLIC_KEY.starts_with("dW50cnVzdGVkIGNvbW1lbnQ6"));
    }
}
