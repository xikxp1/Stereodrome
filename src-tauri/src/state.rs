use rusqlite::Connection;
use std::sync::Mutex;
use submarine::Client;

use crate::audio::{AudioPlayer, PlayQueue};
use crate::error::AppResult;

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
}

impl AppState {
    pub fn new(db_path: &str) -> AppResult<Self> {
        let conn = Connection::open(db_path)?;
        let audio_player = AudioPlayer::new()?;

        Ok(Self {
            client: Mutex::new(None),
            server_config: Mutex::new(None),
            db: Mutex::new(conn),
            audio_player: Mutex::new(audio_player),
            queue: Mutex::new(PlayQueue::new()),
        })
    }

    pub fn is_connected(&self) -> bool {
        self.client.lock().unwrap().is_some()
    }
}
