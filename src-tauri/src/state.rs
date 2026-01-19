use rusqlite::Connection;
use std::sync::Mutex;
use submarine::Client;

use crate::error::AppResult;

pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

pub struct AppState {
    pub client: Mutex<Option<Client>>,
    pub server_config: Mutex<Option<ServerConfig>>,
    pub db: Mutex<Connection>,
}

impl AppState {
    pub fn new(db_path: &str) -> AppResult<Self> {
        let conn = Connection::open(db_path)?;
        Ok(Self {
            client: Mutex::new(None),
            server_config: Mutex::new(None),
            db: Mutex::new(conn),
        })
    }

    pub fn is_connected(&self) -> bool {
        self.client.lock().unwrap().is_some()
    }
}
