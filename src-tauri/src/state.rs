use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use submarine::Client;

use crate::audio::{AudioPlayer, PlayQueue};
use crate::db::queue::{load_queue_items, load_queue_state};
use crate::error::{AppResult, MutexExt};
use crate::search::IndexManager;

#[derive(Clone)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

pub struct AppState {
    pub client: Mutex<Option<Client>>,
    pub server_config: Mutex<Option<ServerConfig>>,
    pub db: Mutex<Connection>,
    pub audio_player: Mutex<AudioPlayer>,
    pub queue: Mutex<PlayQueue>,
    pub search_index: Mutex<Option<IndexManager>>,
    pub index_path: PathBuf,
    pub emitter_running: Arc<AtomicBool>,
    /// Prevents race conditions when rapidly clicking next/previous
    pub navigating: AtomicBool,
}

impl AppState {
    pub fn new(db_path: &str, index_path: PathBuf) -> AppResult<Self> {
        let conn = Connection::open(db_path)?;
        let audio_player = AudioPlayer::new()?;

        // Try to create search index, but don't fail if it errors
        let search_index = match IndexManager::new(&index_path) {
            Ok(manager) => {
                eprintln!("Search index initialized at {:?}", index_path);
                Some(manager)
            }
            Err(e) => {
                eprintln!("Failed to initialize search index: {}", e);
                None
            }
        };

        // Load persisted queue
        let queue = match (load_queue_items(&conn), load_queue_state(&conn)) {
            (Ok(items), Ok((current_index, shuffle, repeat_mode))) => {
                eprintln!("Loaded queue with {} items from database", items.len());
                PlayQueue::load(items, current_index, shuffle, repeat_mode)
            }
            _ => {
                eprintln!("No persisted queue found, starting fresh");
                PlayQueue::new()
            }
        };

        Ok(Self {
            client: Mutex::new(None),
            server_config: Mutex::new(None),
            db: Mutex::new(conn),
            audio_player: Mutex::new(audio_player),
            queue: Mutex::new(queue),
            search_index: Mutex::new(search_index),
            index_path,
            emitter_running: Arc::new(AtomicBool::new(true)),
            navigating: AtomicBool::new(false),
        })
    }

    pub fn get_client(&self) -> Option<Client> {
        self.client.lock_recover().clone()
    }

    pub fn is_connected(&self) -> bool {
        self.client.lock_recover().is_some()
    }
}
