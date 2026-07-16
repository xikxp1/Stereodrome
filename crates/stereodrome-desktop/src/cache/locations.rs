use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::{DesktopPaths, JsonStore};
use log::warn;
use serde::Serialize;

pub const AUDIO_CACHE_DIR_NAME: &str = "audio_cache";
pub const COVER_CACHE_DIR_NAME: &str = "cover_cache";
pub const KEY_CACHE_ROOT: &str = "cache_root";

#[derive(Debug, Clone, Serialize)]
pub struct CacheLocationInfo {
    pub cache_root: String,
    pub default_cache_root: String,
    pub audio_cache_dir: String,
    pub cover_cache_dir: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CacheMoveSummary {
    pub moved_files: u64,
    pub skipped_files: u64,
    pub failed_files: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheRootUpdateResult {
    pub locations: CacheLocationInfo,
    pub audio: CacheMoveSummary,
    pub cover_art: CacheMoveSummary,
}

pub fn default_cache_root(paths: &DesktopPaths) -> PathBuf {
    paths.default_cache_root.clone()
}

pub fn current_cache_root(paths: &DesktopPaths, settings: &JsonStore) -> AppResult<PathBuf> {
    if let Some(path) = settings.get::<String>(KEY_CACHE_ROOT)? {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Ok(path);
        }
        warn!("Ignoring relative cache root from settings: {:?}", path);
    }

    Ok(default_cache_root(paths))
}

pub fn audio_cache_dir(paths: &DesktopPaths, settings: &JsonStore) -> AppResult<PathBuf> {
    let cache_dir = current_cache_root(paths, settings)?.join(AUDIO_CACHE_DIR_NAME);
    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

pub fn cover_cache_dir(paths: &DesktopPaths, settings: &JsonStore) -> AppResult<PathBuf> {
    let cache_dir = current_cache_root(paths, settings)?.join(COVER_CACHE_DIR_NAME);
    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

pub fn cache_location_info(
    paths: &DesktopPaths,
    settings: &JsonStore,
) -> AppResult<CacheLocationInfo> {
    let cache_root = current_cache_root(paths, settings)?;
    let default_cache_root = default_cache_root(paths);
    Ok(location_info_from_roots(cache_root, default_cache_root))
}

pub fn set_cache_root(
    paths: &DesktopPaths,
    settings: &JsonStore,
    cache_root: Option<String>,
) -> AppResult<CacheRootUpdateResult> {
    let previous_root = current_cache_root(paths, settings)?;
    let default_root = default_cache_root(paths);
    let next_root = normalize_requested_root(cache_root, &default_root)?;

    fs::create_dir_all(&next_root)?;
    let previous_audio_dir = previous_root.join(AUDIO_CACHE_DIR_NAME);
    let previous_cover_dir = previous_root.join(COVER_CACHE_DIR_NAME);
    let next_audio_dir = next_root.join(AUDIO_CACHE_DIR_NAME);
    let next_cover_dir = next_root.join(COVER_CACHE_DIR_NAME);
    fs::create_dir_all(&next_audio_dir)?;
    fs::create_dir_all(&next_cover_dir)?;

    let same_root = paths_refer_to_same_location(&previous_root, &next_root);
    let (audio, cover_art) = if same_root {
        (CacheMoveSummary::default(), CacheMoveSummary::default())
    } else {
        (
            move_cache_files(&previous_audio_dir, &next_audio_dir),
            move_cache_files(&previous_cover_dir, &next_cover_dir),
        )
    };

    write_cache_root(settings, &next_root, &default_root)?;

    Ok(CacheRootUpdateResult {
        locations: location_info_from_roots(next_root, default_root),
        audio,
        cover_art,
    })
}

fn normalize_requested_root(cache_root: Option<String>, default_root: &Path) -> AppResult<PathBuf> {
    let Some(cache_root) = cache_root.map(|value| value.trim().to_string()) else {
        return Ok(default_root.to_path_buf());
    };
    if cache_root.is_empty() {
        return Ok(default_root.to_path_buf());
    }

    let path = PathBuf::from(cache_root);
    if !path.is_absolute() {
        return Err(AppError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cache root must be an absolute path",
        )));
    }
    Ok(path)
}

fn write_cache_root(settings: &JsonStore, next_root: &Path, default_root: &Path) -> AppResult<()> {
    if paths_refer_to_same_location(next_root, default_root) {
        settings.remove(KEY_CACHE_ROOT)?;
    } else {
        settings.set(KEY_CACHE_ROOT, next_root.to_string_lossy().to_string())?;
    }
    Ok(())
}

fn location_info_from_roots(cache_root: PathBuf, default_cache_root: PathBuf) -> CacheLocationInfo {
    CacheLocationInfo {
        audio_cache_dir: cache_root
            .join(AUDIO_CACHE_DIR_NAME)
            .to_string_lossy()
            .to_string(),
        cover_cache_dir: cache_root
            .join(COVER_CACHE_DIR_NAME)
            .to_string_lossy()
            .to_string(),
        is_default: paths_refer_to_same_location(&cache_root, &default_cache_root),
        cache_root: cache_root.to_string_lossy().to_string(),
        default_cache_root: default_cache_root.to_string_lossy().to_string(),
    }
}

fn move_cache_files(source_dir: &Path, destination_dir: &Path) -> CacheMoveSummary {
    let mut summary = CacheMoveSummary::default();
    if paths_refer_to_same_location(source_dir, destination_dir) || !source_dir.exists() {
        return summary;
    }

    let entries = match fs::read_dir(source_dir) {
        Ok(entries) => entries,
        Err(error) => {
            warn!("Failed to read cache directory {:?}: {}", source_dir, error);
            summary.failed_files += 1;
            return summary;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn!(
                    "Failed to read a cache directory entry in {:?}: {}",
                    source_dir, error
                );
                summary.failed_files += 1;
                continue;
            }
        };
        let source_path = entry.path();
        if !source_path.is_file() {
            summary.skipped_files += 1;
            continue;
        }

        let destination_path = destination_dir.join(entry.file_name());
        if destination_path.exists() {
            summary.skipped_files += 1;
            continue;
        }

        match move_file_without_overwrite(&source_path, &destination_path) {
            Ok(()) => summary.moved_files += 1,
            Err(error) => {
                warn!(
                    "Failed to move cache file {:?} to {:?}: {}",
                    source_path, destination_path, error
                );
                summary.failed_files += 1;
            }
        }
    }

    summary
}

fn move_file_without_overwrite(source_path: &Path, destination_path: &Path) -> io::Result<()> {
    let mut source = fs::File::open(source_path)?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination_path)?;
    if let Err(error) = io::copy(&mut source, &mut destination) {
        drop(destination);
        let _ = fs::remove_file(destination_path);
        return Err(error);
    }
    fs::remove_file(source_path)
}

fn paths_refer_to_same_location(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AUDIO_CACHE_DIR_NAME, COVER_CACHE_DIR_NAME, move_cache_files, normalize_requested_root,
    };
    use std::fs;
    use std::path::Path;

    fn temp_dir(test_name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "stereodrome-cache-location-test-{test_name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn normalize_requested_root_uses_default_for_none_or_blank() {
        let default_root = Path::new("/default/cache");
        assert_eq!(
            normalize_requested_root(None, default_root).unwrap(),
            default_root
        );
        assert_eq!(
            normalize_requested_root(Some("  ".to_string()), default_root).unwrap(),
            default_root
        );
    }

    #[test]
    fn normalize_requested_root_rejects_relative_paths() {
        let default_root = Path::new("/default/cache");
        assert!(normalize_requested_root(Some("relative".to_string()), default_root).is_err());
    }

    #[test]
    fn move_cache_files_moves_files_and_skips_conflicts() {
        let root = temp_dir("move-cache-files");
        let source = root.join(AUDIO_CACHE_DIR_NAME);
        let destination = root.join(COVER_CACHE_DIR_NAME);
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("move.mp3"), "audio").unwrap();
        fs::write(source.join("conflict.mp3"), "old").unwrap();
        fs::write(destination.join("conflict.mp3"), "new").unwrap();

        let summary = move_cache_files(&source, &destination);

        assert_eq!(summary.moved_files, 1);
        assert_eq!(summary.skipped_files, 1);
        assert_eq!(summary.failed_files, 0);
        assert!(!source.join("move.mp3").exists());
        assert_eq!(
            fs::read_to_string(destination.join("move.mp3")).unwrap(),
            "audio"
        );
        assert_eq!(
            fs::read_to_string(destination.join("conflict.mp3")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(source.join("conflict.mp3")).unwrap(),
            "old"
        );

        let _ = fs::remove_dir_all(root);
    }
}
