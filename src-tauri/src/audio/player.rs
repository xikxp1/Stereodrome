use rodio::{Decoder, OutputStreamBuilder, Sink};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackStatus {
    pub is_playing: bool,
    pub current_song_id: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
}

/// Commands sent to the audio thread
enum AudioCommand {
    Play {
        audio_data: Vec<u8>,
        song_id: String,
        duration_secs: f64,
    },
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    Shutdown,
}

/// State shared between the main thread and audio thread
struct SharedState {
    is_playing: AtomicBool,
    current_song_id: Mutex<Option<String>>,
    volume: Mutex<f32>,
    playback_start: Mutex<Option<Instant>>,
    paused_position: Mutex<f64>,
    duration: Mutex<f64>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            is_playing: AtomicBool::new(false),
            current_song_id: Mutex::new(None),
            volume: Mutex::new(0.8),
            playback_start: Mutex::new(None),
            paused_position: Mutex::new(0.0),
            duration: Mutex::new(0.0),
        }
    }

    fn get_position(&self) -> f64 {
        if !self.is_playing.load(Ordering::SeqCst) {
            return *self.paused_position.lock().unwrap();
        }

        let start = self.playback_start.lock().unwrap();
        let paused = *self.paused_position.lock().unwrap();
        let dur = *self.duration.lock().unwrap();

        if let Some(instant) = *start {
            (instant.elapsed().as_secs_f64() + paused).min(dur)
        } else {
            paused
        }
    }

    fn get_status(&self) -> PlaybackStatus {
        PlaybackStatus {
            is_playing: self.is_playing.load(Ordering::SeqCst),
            current_song_id: self.current_song_id.lock().unwrap().clone(),
            position: self.get_position(),
            duration: *self.duration.lock().unwrap(),
            volume: *self.volume.lock().unwrap(),
        }
    }
}

pub struct AudioPlayer {
    command_tx: Sender<AudioCommand>,
    shared_state: Arc<SharedState>,
    _audio_thread: JoinHandle<()>,
}

// Implement Send + Sync manually since we use channels for thread communication
unsafe impl Send for AudioPlayer {}
unsafe impl Sync for AudioPlayer {}

impl AudioPlayer {
    pub fn new() -> AppResult<Self> {
        let (command_tx, command_rx) = mpsc::channel::<AudioCommand>();
        let shared_state = Arc::new(SharedState::new());
        let state_clone = Arc::clone(&shared_state);

        let audio_thread = thread::spawn(move || {
            run_audio_thread(command_rx, state_clone);
        });

        Ok(Self {
            command_tx,
            shared_state,
            _audio_thread: audio_thread,
        })
    }

    pub fn play(&self, audio_data: Vec<u8>, song_id: String, duration_secs: f64) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Play {
                audio_data,
                song_id,
                duration_secs,
            })
            .map_err(|e| AppError::Audio(format!("Failed to send play command: {}", e)))
    }

    pub fn pause(&self) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Pause)
            .map_err(|e| AppError::Audio(format!("Failed to send pause command: {}", e)))
    }

    pub fn resume(&self) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Resume)
            .map_err(|e| AppError::Audio(format!("Failed to send resume command: {}", e)))
    }

    pub fn stop(&self) -> AppResult<()> {
        self.command_tx
            .send(AudioCommand::Stop)
            .map_err(|e| AppError::Audio(format!("Failed to send stop command: {}", e)))
    }

    pub fn set_volume(&self, volume: f32) -> AppResult<()> {
        let clamped = volume.clamp(0.0, 1.0);
        *self.shared_state.volume.lock().unwrap() = clamped;
        self.command_tx
            .send(AudioCommand::SetVolume(clamped))
            .map_err(|e| AppError::Audio(format!("Failed to send volume command: {}", e)))
    }

    #[allow(dead_code)]
    pub fn get_volume(&self) -> f32 {
        *self.shared_state.volume.lock().unwrap()
    }

    #[allow(dead_code)]
    pub fn get_position(&self) -> f64 {
        self.shared_state.get_position()
    }

    #[allow(dead_code)]
    pub fn get_duration(&self) -> f64 {
        *self.shared_state.duration.lock().unwrap()
    }

    pub fn get_status(&self) -> PlaybackStatus {
        self.shared_state.get_status()
    }

    #[allow(dead_code)]
    pub fn current_song_id(&self) -> Option<String> {
        self.shared_state.current_song_id.lock().unwrap().clone()
    }

    #[allow(dead_code)]
    pub fn is_playing(&self) -> bool {
        self.shared_state.is_playing.load(Ordering::SeqCst)
    }

    /// Start a background thread that emits position updates
    pub fn start_position_emitter(&self, app_handle: AppHandle) {
        let shared_state = Arc::clone(&self.shared_state);

        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(100)); // 10Hz updates

                if !shared_state.is_playing.load(Ordering::SeqCst) {
                    continue;
                }

                let position = shared_state.get_position();
                let duration = *shared_state.duration.lock().unwrap();
                let song_id = shared_state.current_song_id.lock().unwrap().clone();

                // Check if playback ended naturally
                if position >= duration && duration > 0.0 {
                    shared_state.is_playing.store(false, Ordering::SeqCst);
                    let _ = app_handle.emit("playback-ended", ());
                    continue;
                }

                let _ = app_handle.emit(
                    "playback-position",
                    serde_json::json!({
                        "position": position,
                        "song_id": song_id
                    }),
                );
            }
        });
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.command_tx.send(AudioCommand::Shutdown);
    }
}

/// Main audio thread function
fn run_audio_thread(command_rx: Receiver<AudioCommand>, shared_state: Arc<SharedState>) {
    // Open the default audio output stream
    let stream = match OutputStreamBuilder::open_default_stream() {
        Ok(mut s) => {
            s.log_on_drop(false);
            s
        }
        Err(e) => {
            eprintln!("Failed to open audio stream: {:?}", e);
            return;
        }
    };

    let mut current_sink: Option<Sink> = None;

    loop {
        // Use recv_timeout to allow periodic checks
        match command_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(command) => match command {
                AudioCommand::Play {
                    audio_data,
                    song_id,
                    duration_secs,
                } => {
                    // Stop any existing playback
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }

                    // Decode and play
                    let cursor = Cursor::new(audio_data);
                    match Decoder::new(cursor) {
                        Ok(source) => {
                            let sink = Sink::connect_new(stream.mixer());
                            let volume = *shared_state.volume.lock().unwrap();
                            sink.set_volume(volume);
                            sink.append(source);

                            // Update shared state
                            *shared_state.current_song_id.lock().unwrap() = Some(song_id);
                            *shared_state.playback_start.lock().unwrap() = Some(Instant::now());
                            *shared_state.paused_position.lock().unwrap() = 0.0;
                            *shared_state.duration.lock().unwrap() = duration_secs;
                            shared_state.is_playing.store(true, Ordering::SeqCst);

                            current_sink = Some(sink);
                        }
                        Err(e) => {
                            eprintln!("Failed to decode audio: {:?}", e);
                        }
                    }
                }
                AudioCommand::Pause => {
                    if let Some(ref sink) = current_sink {
                        if !sink.is_paused() {
                            // Save current position
                            let start = shared_state.playback_start.lock().unwrap();
                            let paused = *shared_state.paused_position.lock().unwrap();
                            if let Some(instant) = *start {
                                let elapsed = instant.elapsed().as_secs_f64() + paused;
                                *shared_state.paused_position.lock().unwrap() = elapsed;
                            }
                            drop(start);

                            sink.pause();
                            shared_state.is_playing.store(false, Ordering::SeqCst);
                        }
                    }
                }
                AudioCommand::Resume => {
                    if let Some(ref sink) = current_sink {
                        if sink.is_paused() {
                            *shared_state.playback_start.lock().unwrap() = Some(Instant::now());
                            sink.play();
                            shared_state.is_playing.store(true, Ordering::SeqCst);
                        }
                    }
                }
                AudioCommand::Stop => {
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }
                    shared_state.is_playing.store(false, Ordering::SeqCst);
                    *shared_state.current_song_id.lock().unwrap() = None;
                    *shared_state.playback_start.lock().unwrap() = None;
                    *shared_state.paused_position.lock().unwrap() = 0.0;
                    *shared_state.duration.lock().unwrap() = 0.0;
                }
                AudioCommand::SetVolume(volume) => {
                    if let Some(ref sink) = current_sink {
                        sink.set_volume(volume);
                    }
                }
                AudioCommand::Shutdown => {
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }
                    break;
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Check if current playback has ended
                if let Some(ref sink) = current_sink {
                    if sink.empty() && shared_state.is_playing.load(Ordering::SeqCst) {
                        shared_state.is_playing.store(false, Ordering::SeqCst);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }
}
