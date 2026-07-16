use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use log::warn;

use crate::error::MutexExt;
use crate::operations::settings::read_persisted_volume;
use crate::state::DesktopState;
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
    state: Arc<DesktopState>,
    runtime: tokio::runtime::Runtime,
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

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| DesktopError::io(&paths.data_dir, error))?;
        let (client, client_thread) = crate::client::spawn();
        let state = match DesktopState::new(paths, settings, ui_state, client.clone()) {
            Ok(state) => Arc::new(state),
            Err(error) => {
                client.shutdown();
                client_thread.thread().unpark();
                let _ = client_thread.join();
                return Err(error);
            }
        };
        let volume = read_persisted_volume(&state.ui_state);
        if let Err(error) = state.audio_player.lock_recover().set_volume(volume) {
            client.shutdown();
            client_thread.thread().unpark();
            let _ = client_thread.join();
            return Err(error);
        }

        let worker_running = Arc::new(AtomicBool::new(true));
        let library_sync = crate::operations::library::start_library_sync_scheduler(
            Arc::clone(&state),
            Arc::clone(&worker_running),
        );
        let lastfm_retry = crate::lastfm::start_lastfm_retry_scheduler(
            Arc::clone(&state),
            Arc::clone(&worker_running),
        );
        let playback = crate::operations::playback::start_playback_publisher(
            runtime.handle().clone(),
            Arc::clone(&state),
            Arc::clone(&worker_running),
        );
        let spectrum = crate::operations::playback::start_spectrum_publisher(
            Arc::clone(&state),
            Arc::clone(&worker_running),
        );

        Ok(Self {
            state,
            runtime,
            workers: Mutex::new(vec![
                WorkerHandle::Thread {
                    name: "subsonic-client".to_string(),
                    handle: client_thread,
                },
                WorkerHandle::Thread {
                    name: "library-sync".to_string(),
                    handle: library_sync,
                },
                WorkerHandle::Thread {
                    name: "lastfm-retry".to_string(),
                    handle: lastfm_retry,
                },
                WorkerHandle::Thread {
                    name: "playback".to_string(),
                    handle: playback,
                },
                WorkerHandle::Thread {
                    name: "spectrum".to_string(),
                    handle: spectrum,
                },
            ]),
            worker_running,
            shutdown: AtomicBool::new(false),
        })
    }

    pub fn state(&self) -> Arc<DesktopState> {
        Arc::clone(&self.state)
    }

    pub fn paths(&self) -> &DesktopPaths {
        &self.state.paths
    }

    pub fn settings(&self) -> &JsonStore {
        &self.state.settings
    }

    pub fn ui_state(&self) -> &JsonStore {
        &self.state.ui_state
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    pub fn subscribe_playback(
        &self,
    ) -> tokio::sync::watch::Receiver<stereodrome_audio::PlaybackState> {
        self.state.events.subscribe_playback()
    }

    pub fn subscribe_spectrum(
        &self,
    ) -> tokio::sync::watch::Receiver<stereodrome_audio::spectrum::SpectrumData> {
        self.state.events.subscribe_spectrum()
    }

    pub fn take_event_receiver(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<crate::DesktopEvent>> {
        self.state.events.take_durable_receiver()
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

    pub fn shutdown(&self) -> Result<(), DesktopError> {
        if self.shutdown.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        self.worker_running.store(false, Ordering::Release);
        self.state.navigating.store(true, Ordering::Release);
        let mut errors = Vec::new();
        if let Err(error) = self.state.audio_player.lock_recover().stop() {
            errors.push(error.to_string());
        }
        self.state.client.shutdown();

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
        if let Err(error) = self.state.settings.flush() {
            errors.push(error.to_string());
        }
        if let Err(error) = self.state.ui_state.flush() {
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
    static BACKEND_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn shutdown_cancels_joins_and_is_idempotent() {
        let _guard = BACKEND_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
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

        backend.shutdown().unwrap();
        backend.shutdown().unwrap();
        assert_eq!(stopped.load(Ordering::Acquire), 1);
        assert!(backend.is_shutting_down());

        drop(backend);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn copied_profile_keeps_identity_data_and_custom_cache_root() {
        let _guard = BACKEND_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let data_dir = std::env::temp_dir().join(format!(
            "stereodrome-backend-profile-{}",
            std::process::id()
        ));
        let cache_root = data_dir.with_extension("cache");
        let _ = std::fs::remove_dir_all(&data_dir);
        let _ = std::fs::remove_dir_all(&cache_root);
        {
            let index = crate::search::IndexManager::new(&data_dir.join("search_index")).unwrap();
            drop(index);
        }
        {
            let database = rusqlite::Connection::open(data_dir.join("stereodrome.db")).unwrap();
            database
                .execute_batch(
                    "CREATE TABLE copied_marker(value TEXT NOT NULL);
                     INSERT INTO copied_marker(value) VALUES ('preserved');",
                )
                .unwrap();
        }
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
        let preserved: String = backend
            .state()
            .db
            .lock_recover()
            .query_row("SELECT value FROM copied_marker", [], |row| row.get(0))
            .unwrap();
        assert_eq!(preserved, "preserved");
        assert!(backend.state().search_index.lock_recover().is_some());
        backend.shutdown().unwrap();
        drop(backend);

        std::fs::remove_dir_all(data_dir).unwrap();
        std::fs::remove_dir_all(cache_root).unwrap();
    }

    #[test]
    fn settings_mutations_publish_ordered_typed_events() {
        let _guard = BACKEND_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let data_dir =
            std::env::temp_dir().join(format!("stereodrome-backend-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data_dir);
        let backend = DesktopBackend::open(DesktopPaths::from_data_dir(data_dir.clone())).unwrap();
        let mut events = backend.take_event_receiver().unwrap();

        let playback = crate::operations::settings::PlaybackSettings {
            crossfade_duration_ms: 1,
            ..Default::default()
        };
        crate::operations::settings::set_playback_settings(&backend.state(), playback).unwrap();
        crate::operations::settings::set_connectivity_settings(
            &backend.state(),
            crate::operations::settings::ConnectivitySettings {
                manual_offline_enabled: true,
            },
        )
        .unwrap();
        let sync = crate::operations::settings::SyncSettings {
            incremental_interval_minutes: 1,
            ..Default::default()
        };
        crate::operations::settings::set_sync_settings(&backend.state(), sync).unwrap();

        match events.try_recv().unwrap() {
            crate::DesktopEvent::PlaybackSettingsChanged(settings) => {
                assert_eq!(settings.crossfade_duration_ms, 1_000);
            }
            event => panic!("unexpected first event: {event:?}"),
        }
        match events.try_recv().unwrap() {
            crate::DesktopEvent::ConnectivitySettingsChanged(settings) => {
                assert!(settings.manual_offline_enabled);
            }
            event => panic!("unexpected second event: {event:?}"),
        }
        match events.try_recv().unwrap() {
            crate::DesktopEvent::SyncSettingsChanged(settings) => {
                assert_eq!(settings.incremental_interval_minutes, 5);
            }
            event => panic!("unexpected third event: {event:?}"),
        }
        assert!(events.try_recv().is_err());

        backend.shutdown().unwrap();
        drop(backend);
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}
