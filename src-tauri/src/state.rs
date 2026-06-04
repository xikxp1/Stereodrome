use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use log::{debug, info, warn};
use rusqlite::Connection;

use crate::audio::{AudioPlayer, PlayQueue};
use crate::client::SubsonicClientHandle;
use crate::commands::normalization::AnalysisProgress;
use crate::db::queue::{load_queue_items, load_queue_state};
use crate::error::AppResult;
use crate::lastfm::LastfmPlaybackTracker;
use crate::search::IndexManager;

// Re-export ServerConfig from client module for backward compatibility
pub use crate::client::ServerConfig;

pub struct AppState {
    pub client: SubsonicClientHandle,
    pub db: Mutex<Connection>,
    pub audio_player: Mutex<AudioPlayer>,
    pub queue: Mutex<PlayQueue>,
    pub search_index: Mutex<Option<IndexManager>>,
    pub index_path: PathBuf,
    pub emitter_running: Arc<AtomicBool>,
    /// Prevents race conditions when rapidly clicking next/previous
    pub navigating: AtomicBool,
    pub lastfm_retry_running: AtomicBool,
    pub lastfm_tracker: Mutex<LastfmPlaybackTracker>,
    /// Current analysis progress (set by analyze_all_songs, cleared on completion)
    pub analysis_progress: Arc<Mutex<Option<AnalysisProgress>>>,
}

impl AppState {
    pub fn new(
        db_path: &str,
        index_path: PathBuf,
        client_handle: SubsonicClientHandle,
    ) -> AppResult<Self> {
        let conn = Connection::open(db_path)?;
        let audio_player = AudioPlayer::new()?;

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
            client: client_handle,
            db: Mutex::new(conn),
            audio_player: Mutex::new(audio_player),
            queue: Mutex::new(queue),
            search_index: Mutex::new(search_index),
            index_path,
            emitter_running: Arc::new(AtomicBool::new(true)),
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
