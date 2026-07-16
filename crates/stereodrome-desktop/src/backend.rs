use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use log::warn;

use crate::{DesktopError, DesktopPaths, JsonStore};

pub enum WorkerHandle {
    Thread {
        name: String,
        handle: JoinHandle<()>,
    },
}

impl WorkerHandle {
    fn wake(&self) {
        match self {
            Self::Thread { handle, .. } => handle.thread().unpark(),
        }
    }

    fn join(self) -> Result<(), String> {
        match self {
            Self::Thread { name, handle } => handle
                .join()
                .map_err(|_| format!("worker {name} panicked during shutdown")),
        }
    }
}

pub struct DesktopBackend {
    paths: DesktopPaths,
    settings: JsonStore,
    ui_state: JsonStore,
    workers: Mutex<Vec<WorkerHandle>>,
    worker_running: Arc<AtomicBool>,
    shutdown: AtomicBool,
}

impl DesktopBackend {
    pub fn open(mut paths: DesktopPaths) -> Result<Self, DesktopError> {
        std::fs::create_dir_all(&paths.data_dir)
            .map_err(|error| DesktopError::io(&paths.data_dir, error))?;

        let settings = JsonStore::open(paths.settings.clone())?;
        let ui_state = JsonStore::open(paths.state.clone())?;
        if let Some(cache_root) = settings.get::<String>("cache_root")? {
            let cache_root = Path::new(&cache_root);
            if cache_root.is_absolute() {
                paths.use_cache_root(cache_root);
            } else {
                warn!("Ignoring relative cache root from settings: {cache_root:?}");
            }
        }
        std::fs::create_dir_all(&paths.audio_cache)
            .map_err(|error| DesktopError::io(&paths.audio_cache, error))?;
        std::fs::create_dir_all(&paths.cover_cache)
            .map_err(|error| DesktopError::io(&paths.cover_cache, error))?;

        Ok(Self {
            paths,
            settings,
            ui_state,
            workers: Mutex::new(Vec::new()),
            worker_running: Arc::new(AtomicBool::new(true)),
            shutdown: AtomicBool::new(false),
        })
    }

    pub fn paths(&self) -> &DesktopPaths {
        &self.paths
    }

    pub fn settings(&self) -> &JsonStore {
        &self.settings
    }

    pub fn ui_state(&self) -> &JsonStore {
        &self.ui_state
    }

    pub fn worker_running(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.worker_running)
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    pub fn register_thread(
        &self,
        name: impl Into<String>,
        handle: JoinHandle<()>,
    ) -> Result<(), DesktopError> {
        let mut workers = self
            .workers
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.shutdown.load(Ordering::Acquire) {
            drop(workers);
            handle.thread().unpark();
            let _ = handle.join();
            return Err(DesktopError::ShuttingDown);
        }
        workers.push(WorkerHandle::Thread {
            name: name.into(),
            handle,
        });
        Ok(())
    }

    pub fn shutdown(
        &self,
        request_component_shutdown: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), DesktopError> {
        if self.shutdown.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        self.worker_running.store(false, Ordering::Release);
        let mut errors = Vec::new();
        if let Err(error) = request_component_shutdown() {
            errors.push(error);
        }

        let workers = {
            let mut workers = self
                .workers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::take(&mut *workers)
        };
        for worker in &workers {
            worker.wake();
        }
        for worker in workers {
            if let Err(error) = worker.join() {
                errors.push(error);
            }
        }
        if let Err(error) = self.settings.flush() {
            errors.push(error.to_string());
        }
        if let Err(error) = self.ui_state.flush() {
            errors.push(error.to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(DesktopError::Shutdown(errors.join("; ")))
        }
    }
}

impl Drop for DesktopBackend {
    fn drop(&mut self) {
        if !self.shutdown.load(Ordering::Acquire) {
            self.worker_running.store(false, Ordering::Release);
            let workers = self
                .workers
                .get_mut()
                .unwrap_or_else(|error| error.into_inner());
            for worker in workers.iter() {
                worker.wake();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn shutdown_cancels_joins_and_is_idempotent() {
        let data_dir = std::env::temp_dir().join(format!(
            "stereodrome-backend-shutdown-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&data_dir);
        let backend = DesktopBackend::open(DesktopPaths::from_data_dir(data_dir.clone())).unwrap();
        assert_eq!(
            backend.paths().database.file_name().unwrap(),
            "stereodrome.db"
        );
        assert!(backend.paths().audio_cache.is_dir());
        assert!(backend.paths().cover_cache.is_dir());
        assert_eq!(
            backend
                .settings()
                .get::<serde_json::Value>("playback")
                .unwrap(),
            None
        );
        assert_eq!(backend.ui_state().get::<f32>("volume").unwrap(), None);
        let running = backend.worker_running();
        let stopped = Arc::new(AtomicUsize::new(0));
        let worker_stopped = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                thread::park_timeout(Duration::from_secs(60));
            }
            worker_stopped.fetch_add(1, Ordering::Release);
        });
        backend.register_thread("test", worker).unwrap();

        backend.shutdown(|| Ok(())).unwrap();
        backend
            .shutdown(|| Err("must not run twice".to_string()))
            .unwrap();
        assert_eq!(stopped.load(Ordering::Acquire), 1);
        assert!(backend.is_shutting_down());

        drop(backend);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn copied_profile_keeps_identity_data_and_custom_cache_root() {
        let data_dir = std::env::temp_dir().join(format!(
            "stereodrome-backend-profile-{}",
            std::process::id()
        ));
        let cache_root = data_dir.with_extension("cache");
        let _ = std::fs::remove_dir_all(&data_dir);
        let _ = std::fs::remove_dir_all(&cache_root);
        std::fs::create_dir_all(data_dir.join("search_index")).unwrap();
        std::fs::write(data_dir.join("stereodrome.db"), b"copied database").unwrap();
        std::fs::write(data_dir.join("search_index/marker"), b"copied index").unwrap();
        std::fs::write(
            data_dir.join("settings.json"),
            serde_json::to_vec(&serde_json::json!({
                "cache_root": cache_root,
                "future_key": {"kept": true}
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(data_dir.join("state.json"), br#"{"volume":0.4}"#).unwrap();

        let paths = DesktopPaths::from_data_dir(data_dir.clone());
        paths.verify_installed_profile(&data_dir).unwrap();
        let backend = DesktopBackend::open(paths).unwrap();
        assert_eq!(
            backend
                .settings()
                .get::<serde_json::Value>("future_key")
                .unwrap(),
            Some(serde_json::json!({"kept": true}))
        );
        assert_eq!(backend.ui_state().get::<f32>("volume").unwrap(), Some(0.4));
        assert_eq!(backend.paths().audio_cache, cache_root.join("audio_cache"));
        assert_eq!(backend.paths().cover_cache, cache_root.join("cover_cache"));
        assert_eq!(
            std::fs::read(&backend.paths().database).unwrap(),
            b"copied database"
        );
        assert_eq!(
            std::fs::read(backend.paths().search_index.join("marker")).unwrap(),
            b"copied index"
        );
        backend.shutdown(|| Ok(())).unwrap();
        drop(backend);

        std::fs::remove_dir_all(data_dir).unwrap();
        std::fs::remove_dir_all(cache_root).unwrap();
    }
}
