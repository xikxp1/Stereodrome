use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use log::{debug, info, warn};
use rusqlite::Connection;

use crate::audio::{AudioPlayer, PlayQueue};
use crate::client::SubsonicClientHandle;
use crate::db;
use crate::db::queue::{load_queue_items, load_queue_state};
use crate::error::AppResult;
use crate::events::DesktopEvents;
use crate::lastfm::LastfmPlaybackTracker;
use crate::operations::normalization::AnalysisProgress;
use crate::search::IndexManager;
use crate::{DesktopPaths, JsonStore};

// Re-export ServerConfig from client module for backward compatibility
pub use crate::client::ServerConfig;

pub struct DesktopState {
    pub paths: DesktopPaths,
    pub settings: JsonStore,
    pub ui_state: JsonStore,
    pub client: SubsonicClientHandle,
    pub db: Mutex<Connection>,
    pub audio_player: Mutex<AudioPlayer>,
    pub queue: Mutex<PlayQueue>,
    pub search_index: Mutex<Option<IndexManager>>,
    pub events: DesktopEvents,
    pub index_path: PathBuf,
    /// Prevents race conditions when rapidly clicking next/previous
    pub navigating: AtomicBool,
    pub lastfm_retry_running: AtomicBool,
    pub lastfm_tracker: Mutex<LastfmPlaybackTracker>,
    /// Current analysis progress (set by analyze_all_songs, cleared on completion)
    pub analysis_progress: Arc<Mutex<Option<AnalysisProgress>>>,
}

impl DesktopState {
    pub fn new(
        paths: DesktopPaths,
        settings: JsonStore,
        ui_state: JsonStore,
        client_handle: SubsonicClientHandle,
    ) -> AppResult<Self> {
        let conn = Connection::open(&paths.database)?;
        db::init_db(&conn)?;
        let index_path = paths.search_index.clone();
        let audio_player = AudioPlayer::new()?;
        let events = DesktopEvents::new(&audio_player);

        // Try to create search index, but don't fail if it errors
        let search_index = match IndexManager::new(&index_path) {
            Ok(manager) => {
                info!("Search index initialized at {:?}", index_path);
                Some(manager)
            }
            Err(e) => {
                warn!("Failed to initialize search index: {}", e);
                None
            }
        };

        // Load persisted queue
        let queue = match (load_queue_items(&conn), load_queue_state(&conn)) {
            (Ok(items), Ok((current_index, shuffle, repeat_mode))) => {
                debug!("Loaded queue with {} items from database", items.len());
                PlayQueue::load(items, current_index, shuffle, repeat_mode)
            }
            _ => {
                debug!("No persisted queue found, starting fresh");
                PlayQueue::new()
            }
        };

        Ok(Self {
            paths,
            settings,
            ui_state,
            client: client_handle,
            db: Mutex::new(conn),
            audio_player: Mutex::new(audio_player),
            queue: Mutex::new(queue),
            search_index: Mutex::new(search_index),
            events,
            index_path,
            navigating: AtomicBool::new(false),
            lastfm_retry_running: AtomicBool::new(false),
            lastfm_tracker: Mutex::new(LastfmPlaybackTracker::default()),
            analysis_progress: Arc::new(Mutex::new(None)),
        })
    }

    /// Check if connected (fast, no lock needed)
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }
}
