use std::path::{Path, PathBuf};

use crate::{APPLICATION_ID, DesktopError};

const DATABASE_FILE: &str = "stereodrome.db";
const SEARCH_INDEX_DIR: &str = "search_index";
const SETTINGS_FILE: &str = "settings.json";
const STATE_FILE: &str = "state.json";
const AUDIO_CACHE_DIR: &str = "audio_cache";
const COVER_CACHE_DIR: &str = "cover_cache";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPaths {
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub search_index: PathBuf,
    pub settings: PathBuf,
    pub state: PathBuf,
    pub default_cache_root: PathBuf,
    pub audio_cache: PathBuf,
    pub cover_cache: PathBuf,
}

impl DesktopPaths {
    pub fn detect() -> Result<Self, DesktopError> {
        let data_dir = directories::BaseDirs::new()
            .ok_or(DesktopError::NoDataDirectory)?
            .data_dir()
            .join(APPLICATION_ID);
        Ok(Self::from_data_dir(data_dir))
    }

    pub fn from_data_dir(data_dir: PathBuf) -> Self {
        Self {
            database: data_dir.join(DATABASE_FILE),
            search_index: data_dir.join(SEARCH_INDEX_DIR),
            settings: data_dir.join(SETTINGS_FILE),
            state: data_dir.join(STATE_FILE),
            default_cache_root: data_dir.clone(),
            audio_cache: data_dir.join(AUDIO_CACHE_DIR),
            cover_cache: data_dir.join(COVER_CACHE_DIR),
            data_dir,
        }
    }

    pub fn verify_installed_profile(&self, installed_data_dir: &Path) -> Result<(), DesktopError> {
        if self.data_dir == installed_data_dir {
            Ok(())
        } else {
            Err(DesktopError::ProfileMismatch {
                candidate: self.data_dir.clone(),
                installed: installed_data_dir.to_path_buf(),
            })
        }
    }

    pub(crate) fn use_cache_root(&mut self, cache_root: &Path) {
        self.audio_cache = cache_root.join(AUDIO_CACHE_DIR);
        self.cover_cache = cache_root.join(COVER_CACHE_DIR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_legacy_leaf_names() {
        let paths = DesktopPaths::from_data_dir(PathBuf::from("profile"));
        assert_eq!(paths.database, PathBuf::from("profile/stereodrome.db"));
        assert_eq!(paths.search_index, PathBuf::from("profile/search_index"));
        assert_eq!(paths.settings, PathBuf::from("profile/settings.json"));
        assert_eq!(paths.state, PathBuf::from("profile/state.json"));
        assert_eq!(paths.audio_cache, PathBuf::from("profile/audio_cache"));
        assert_eq!(paths.cover_cache, PathBuf::from("profile/cover_cache"));
    }

    #[test]
    fn detects_the_shipping_platform_profile_directory() {
        let detected = DesktopPaths::detect().unwrap().data_dir;
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let base_dirs = directories::BaseDirs::new().unwrap();
        #[cfg(target_os = "macos")]
        let expected = base_dirs
            .home_dir()
            .join("Library/Application Support")
            .join(APPLICATION_ID);
        #[cfg(target_os = "windows")]
        let expected = PathBuf::from(std::env::var_os("APPDATA").unwrap()).join(APPLICATION_ID);
        #[cfg(target_os = "linux")]
        let expected = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| base_dirs.home_dir().join(".local/share"))
            .join(APPLICATION_ID);

        assert_eq!(detected, expected);
    }

    #[test]
    fn refuses_a_second_profile() {
        let paths = DesktopPaths::from_data_dir(PathBuf::from("candidate"));
        assert!(paths.verify_installed_profile(Path::new("legacy")).is_err());
    }
}
