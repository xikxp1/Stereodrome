//! Deterministic boundary doubles for runtime characterization tests.
//!
//! These types intentionally model effects rather than Stereodrome policy. The
//! runtime introduced in later refactor phases can put its transition logic
//! under test without real clocks, audio devices, servers, files, or callbacks.

#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<DateTime<Utc>>,
}

impl ManualClock {
    #[must_use]
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    #[must_use]
    pub fn now(&self) -> DateTime<Utc> {
        *self.now.lock().expect("manual clock lock")
    }

    pub fn advance(&self, duration: Duration) {
        let mut now = self.now.lock().expect("manual clock lock");
        *now += duration;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AudioCall {
    Play { song_id: String },
    Pause,
    Resume,
    Stop,
    Seek { position_seconds: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FakeAudioState {
    pub song_id: Option<String>,
    pub is_playing: bool,
    pub position_seconds: f64,
}

#[derive(Debug)]
pub struct FakeAudio {
    state: Mutex<FakeAudioState>,
    calls: Mutex<Vec<AudioCall>>,
    failures: Mutex<VecDeque<String>>,
}

impl Default for FakeAudio {
    fn default() -> Self {
        Self {
            state: Mutex::new(FakeAudioState {
                song_id: None,
                is_playing: false,
                position_seconds: 0.0,
            }),
            calls: Mutex::new(Vec::new()),
            failures: Mutex::new(VecDeque::new()),
        }
    }
}

impl FakeAudio {
    pub fn fail_next(&self, message: impl Into<String>) {
        self.failures
            .lock()
            .expect("fake audio failure lock")
            .push_back(message.into());
    }

    pub fn play(&self, song_id: impl Into<String>) -> Result<(), String> {
        let song_id = song_id.into();
        self.record(AudioCall::Play {
            song_id: song_id.clone(),
        })?;
        let mut state = self.state.lock().expect("fake audio state lock");
        state.song_id = Some(song_id);
        state.is_playing = true;
        state.position_seconds = 0.0;
        Ok(())
    }

    pub fn pause(&self) -> Result<(), String> {
        self.record(AudioCall::Pause)?;
        self.state.lock().expect("fake audio state lock").is_playing = false;
        Ok(())
    }

    pub fn resume(&self) -> Result<(), String> {
        self.record(AudioCall::Resume)?;
        let mut state = self.state.lock().expect("fake audio state lock");
        if state.song_id.is_none() {
            return Err("no song is loaded".to_string());
        }
        state.is_playing = true;
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        self.record(AudioCall::Stop)?;
        let mut state = self.state.lock().expect("fake audio state lock");
        state.song_id = None;
        state.is_playing = false;
        state.position_seconds = 0.0;
        Ok(())
    }

    pub fn seek(&self, position_seconds: f64) -> Result<(), String> {
        self.record(AudioCall::Seek { position_seconds })?;
        self.state
            .lock()
            .expect("fake audio state lock")
            .position_seconds = position_seconds;
        Ok(())
    }

    #[must_use]
    pub fn state(&self) -> FakeAudioState {
        self.state.lock().expect("fake audio state lock").clone()
    }

    #[must_use]
    pub fn calls(&self) -> Vec<AudioCall> {
        self.calls.lock().expect("fake audio call lock").clone()
    }

    fn record(&self, call: AudioCall) -> Result<(), String> {
        self.calls.lock().expect("fake audio call lock").push(call);
        if let Some(message) = self
            .failures
            .lock()
            .expect("fake audio failure lock")
            .pop_front()
        {
            return Err(message);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCall {
    pub operation: String,
    pub payload: Value,
}

#[derive(Debug, Default)]
pub struct FakeServer {
    calls: Mutex<Vec<ServerCall>>,
    responses: Mutex<VecDeque<Result<Value, String>>>,
}

impl FakeServer {
    pub fn respond_with(&self, response: Result<Value, String>) {
        self.responses
            .lock()
            .expect("fake server response lock")
            .push_back(response);
    }

    pub fn call(&self, operation: impl Into<String>, payload: Value) -> Result<Value, String> {
        self.calls
            .lock()
            .expect("fake server call lock")
            .push(ServerCall {
                operation: operation.into(),
                payload,
            });
        self.responses
            .lock()
            .expect("fake server response lock")
            .pop_front()
            .unwrap_or_else(|| Err("no fake server response was queued".to_string()))
    }

    #[must_use]
    pub fn calls(&self) -> Vec<ServerCall> {
        self.calls.lock().expect("fake server call lock").clone()
    }
}

#[derive(Debug, Default)]
pub struct MemoryRepository {
    values: Mutex<BTreeMap<String, Value>>,
    next_write_failure: Mutex<Option<String>>,
}

impl MemoryRepository {
    pub fn fail_next_write(&self, message: impl Into<String>) {
        *self
            .next_write_failure
            .lock()
            .expect("memory repository failure lock") = Some(message.into());
    }

    pub fn put(&self, key: impl Into<String>, value: Value) -> Result<(), String> {
        if let Some(message) = self
            .next_write_failure
            .lock()
            .expect("memory repository failure lock")
            .take()
        {
            return Err(message);
        }
        self.values
            .lock()
            .expect("memory repository value lock")
            .insert(key.into(), value);
        Ok(())
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<Value> {
        self.values
            .lock()
            .expect("memory repository value lock")
            .get(key)
            .cloned()
    }
}

#[derive(Debug, Default)]
pub struct RecordingEventSink {
    events: Mutex<Vec<Value>>,
}

impl RecordingEventSink {
    pub fn emit(&self, event: Value) {
        self.events
            .lock()
            .expect("recording event sink lock")
            .push(event);
    }

    #[must_use]
    pub fn events(&self) -> Vec<Value> {
        self.events
            .lock()
            .expect("recording event sink lock")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    #[test]
    fn deterministic_boundaries_record_order_failures_and_time() {
        let clock = ManualClock::new(
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
                .single()
                .expect("valid test date"),
        );
        clock.advance(Duration::seconds(30));
        assert_eq!(clock.now().timestamp(), 1_767_225_630);

        let audio = FakeAudio::default();
        audio.play("song-a").expect("fake play succeeds");
        audio.seek(12.5).expect("fake seek succeeds");
        audio.fail_next("output lost");
        assert_eq!(audio.pause(), Err("output lost".to_string()));
        assert!((audio.state().position_seconds - 12.5).abs() < f64::EPSILON);
        assert_eq!(audio.calls().len(), 3);

        let server = FakeServer::default();
        server.respond_with(Ok(json!({ "connected": true })));
        assert_eq!(
            server.call("ping", Value::Null).expect("fake ping"),
            json!({ "connected": true })
        );

        let repository = MemoryRepository::default();
        repository
            .put("queue", json!(["song-a"]))
            .expect("memory write");
        repository.fail_next_write("database busy");
        assert_eq!(
            repository.put("queue", json!([])),
            Err("database busy".to_string())
        );
        assert_eq!(repository.get("queue"), Some(json!(["song-a"])));

        let events = RecordingEventSink::default();
        events.emit(json!({ "type": "playback", "seq": 1 }));
        assert_eq!(events.events()[0]["seq"], 1);
    }
}
