use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use log::warn;
use serde::{Deserialize, Serialize};
use stereodrome_core::runtime::StereodromeAudioPort;
use stereodrome_core::{StereodromeCore, StereodromeRuntimeHandle};

use crate::commands::normalization::AnalysisProgress;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

pub struct AppState {
    /// Authoritative owner of desktop operational state and transitions.
    pub runtime: StereodromeRuntimeHandle,
    /// Concrete runtime audio boundary retained only for desktop visualization.
    pub runtime_audio: Arc<StereodromeAudioPort>,
    /// Shared database location for desktop-only analysis/read-model adapters.
    pub db_path: PathBuf,
    pub emitter_running: Arc<AtomicBool>,
    /// Current analysis progress (set by `analyze_all_songs`, cleared on completion)
    pub analysis_progress: Arc<Mutex<Option<AnalysisProgress>>>,
}

impl AppState {
    pub fn new(data_dir: &std::path::Path) -> AppResult<Self> {
        let db_path = data_dir.join("stereodrome.db");
        let core = Arc::new(
            StereodromeCore::new(data_dir).map_err(|error| AppError::Runtime(error.to_string()))?,
        );
        match crate::credentials::load_legacy_lastfm_session() {
            Ok(Some(session)) => {
                match core.import_lastfm_session_if_missing(session.username, session.session_key) {
                    Ok(()) => {
                        if let Err(error) = crate::credentials::delete_legacy_lastfm_session() {
                            warn!("Failed to remove migrated Last.fm credential: {error}");
                        }
                    }
                    Err(error) => warn!("Failed to migrate Last.fm credential: {error}"),
                }
            }
            Ok(None) => {}
            Err(error) => warn!("Failed to inspect legacy Last.fm credential: {error}"),
        }
        let runtime_audio = Arc::new(
            StereodromeAudioPort::new_with_spectrum(true)
                .map_err(|error| AppError::Runtime(error.to_string()))?,
        );
        let runtime_audio_port: Arc<dyn stereodrome_core::runtime::AudioPort> =
            runtime_audio.clone();
        let runtime =
            StereodromeRuntimeHandle::start_with_core_and_audio(data_dir, core, runtime_audio_port)
                .map_err(|error| AppError::Runtime(error.to_string()))?;

        Ok(Self {
            runtime,
            runtime_audio,
            db_path,
            emitter_running: Arc::new(AtomicBool::new(true)),
            analysis_progress: Arc::new(Mutex::new(None)),
        })
    }
}
