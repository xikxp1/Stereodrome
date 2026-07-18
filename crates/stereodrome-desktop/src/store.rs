use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::DesktopError;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct JsonStore {
    path: PathBuf,
    values: Mutex<Map<String, Value>>,
}

impl JsonStore {
    pub fn open(path: PathBuf) -> Result<Self, DesktopError> {
        let values = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes)
                .map_err(|source| DesktopError::json(&path, source))?
            {
                Value::Object(values) => values,
                _ => return Err(DesktopError::JsonRootNotObject { path }),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(source) => return Err(DesktopError::io(&path, source)),
        };
        Ok(Self {
            path,
            values: Mutex::new(values),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, DesktopError> {
        let values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        values
            .get(key)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|source| DesktopError::json(&self.path, source))
    }

    pub fn set<T: Serialize>(&self, key: &str, value: T) -> Result<(), DesktopError> {
        let value =
            serde_json::to_value(value).map_err(|source| DesktopError::json(&self.path, source))?;
        self.update(key, Some(value))
    }

    pub fn remove(&self, key: &str) -> Result<(), DesktopError> {
        self.update(key, None)
    }

    pub fn flush(&self) -> Result<(), DesktopError> {
        match OpenOptions::new().read(true).write(true).open(&self.path) {
            Ok(file) => file
                .sync_all()
                .map_err(|source| DesktopError::io(&self.path, source)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(DesktopError::io(&self.path, source)),
        }
    }

    fn update(&self, key: &str, value: Option<Value>) -> Result<(), DesktopError> {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous = values.get(key).cloned();
        match value {
            Some(value) => {
                values.insert(key.to_string(), value);
            }
            None => {
                if values.remove(key).is_none() {
                    return Ok(());
                }
            }
        }

        match persist(&self.path, &values) {
            Ok(()) => Ok(()),
            Err(SaveError::Uncommitted(error)) => {
                match previous {
                    Some(previous) => {
                        values.insert(key.to_string(), previous);
                    }
                    None => {
                        values.remove(key);
                    }
                }
                Err(error)
            }
            #[cfg(unix)]
            Err(SaveError::Committed(error)) => Err(error),
        }
    }
}

enum SaveError {
    Uncommitted(DesktopError),
    #[cfg(unix)]
    Committed(DesktopError),
}

fn persist(path: &Path, values: &Map<String, Value>) -> Result<(), SaveError> {
    let bytes = serde_json::to_vec(&Value::Object(values.clone()))
        .map_err(|source| SaveError::Uncommitted(DesktopError::json(path, source)))?;
    let (temporary_path, mut temporary) = create_temporary(path).map_err(SaveError::Uncommitted)?;

    let write_result = temporary
        .write_all(&bytes)
        .and_then(|()| temporary.sync_all());
    drop(temporary);
    if let Err(source) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(SaveError::Uncommitted(DesktopError::io(
            &temporary_path,
            source,
        )));
    }

    if let Err(error) = atomic_replace(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(SaveError::Uncommitted(error));
    }

    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Err(source) = File::open(parent).and_then(|directory| directory.sync_all())
    {
        return Err(SaveError::Committed(DesktopError::io(parent, source)));
    }

    Ok(())
}

fn create_temporary(path: &Path) -> Result<(PathBuf, File), DesktopError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("store.json");

    loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary_path = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(DesktopError::io(temporary_path, source)),
        }
    }
}

#[cfg(unix)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), DesktopError> {
    fs::rename(source, destination).map_err(|error| DesktopError::io(destination, error))
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), DesktopError> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let error_path = destination.to_path_buf();
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(DesktopError::io(
            error_path,
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "stereodrome-{name}-{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_store_is_empty_and_unknown_keys_survive_writes() {
        let directory = TestDir::new("unknown-key");
        let path = directory.0.join("settings.json");
        let store = JsonStore::open(path.clone()).unwrap();
        assert_eq!(store.get::<Value>("missing").unwrap(), None);

        store
            .set("future_key", serde_json::json!({"kept": true}))
            .unwrap();
        store.set("volume", 0.4_f32).unwrap();
        store.set("volume", 0.8_f32).unwrap();

        let reopened = JsonStore::open(path).unwrap();
        assert_eq!(
            reopened.get::<Value>("future_key").unwrap(),
            Some(serde_json::json!({"kept": true}))
        );
        assert_eq!(reopened.get::<f32>("volume").unwrap(), Some(0.8));
    }

    #[test]
    fn invalid_or_non_object_json_is_never_overwritten() {
        for content in [b"not json".as_slice(), b"[]".as_slice()] {
            let directory = TestDir::new("invalid");
            let path = directory.0.join("settings.json");
            fs::write(&path, content).unwrap();
            assert!(JsonStore::open(path.clone()).is_err());
            assert_eq!(fs::read(path).unwrap(), content);
        }
    }

    #[test]
    fn abandoned_temporary_file_does_not_corrupt_target() {
        let directory = TestDir::new("interrupted");
        let path = directory.0.join("state.json");
        let store = JsonStore::open(path.clone()).unwrap();
        store.set("volume", 0.8_f32).unwrap();
        fs::write(directory.0.join(".state.json.interrupted.tmp"), b"{").unwrap();

        let bytes = fs::read(path).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["volume"], serde_json::json!(0.8_f32));
    }
}
