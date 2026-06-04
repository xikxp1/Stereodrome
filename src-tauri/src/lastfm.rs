use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::Utc;
use log::{debug, warn};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

use crate::audio::SongMetadata;
use crate::credentials::{
    LastfmSession, delete_lastfm_session, load_lastfm_session, save_lastfm_session,
};
use crate::error::{AppError, AppResult, MutexExt};
use crate::state::AppState;

const API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTH_ROOT: &str = "https://www.last.fm/api/auth/";
const API_KEY: Option<&str> = option_env!("LASTFM_API_KEY");
const SHARED_SECRET: Option<&str> = option_env!("LASTFM_SHARED_SECRET");
const STORE_FILE: &str = "settings.json";
const KEY_LASTFM: &str = "lastfm";
const MAX_BATCH_SIZE: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastfmStatus {
    pub available: bool,
    pub authenticated: bool,
    pub enabled: bool,
    pub username: Option<String>,
    pub pending_auth: bool,
    pub queue_count: i64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastfmAuthStart {
    pub auth_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastfmQueueItem {
    pub id: i64,
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration: Option<i64>,
    pub played_at: i64,
    pub attempts: i64,
    pub next_retry_at: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LastfmSettings {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    pending_token: Option<String>,
}

impl Default for LastfmSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            pending_token: None,
        }
    }
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct LastfmQueuedScrobble {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration: Option<i64>,
    pub played_at: i64,
}

#[derive(Debug, Default)]
pub struct LastfmPlaybackTracker {
    current_song_id: Option<String>,
    started_at: i64,
    scrobbled_song_id: Option<String>,
    last_position: f64,
}

impl LastfmPlaybackTracker {
    pub fn update(
        &mut self,
        song: &SongMetadata,
        position: f64,
        duration: f64,
    ) -> Option<LastfmQueuedScrobble> {
        let song_changed = self.current_song_id.as_deref() != Some(song.id.as_str());
        let restarted = !song_changed && position < self.last_position && position < 5.0;

        if song_changed || restarted {
            self.current_song_id = Some(song.id.clone());
            self.started_at = Utc::now().timestamp();
            self.scrobbled_song_id = None;
        }

        self.last_position = position;

        if !should_scrobble(position, duration)
            || self.scrobbled_song_id.as_deref() == Some(&song.id)
        {
            return None;
        }

        self.scrobbled_song_id = Some(song.id.clone());
        Some(LastfmQueuedScrobble {
            song_id: song.id.clone(),
            title: song.title.clone(),
            artist: song.artist.clone(),
            album: non_empty(song.album.as_str()),
            duration: finite_i64(duration),
            played_at: self.started_at,
        })
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn finite_i64(value: f64) -> Option<i64> {
    if value.is_finite() && value > 0.0 {
        Some(value.round() as i64)
    } else {
        None
    }
}

pub fn should_scrobble(position: f64, duration: f64) -> bool {
    if !position.is_finite() || !duration.is_finite() || duration <= 30.0 {
        return false;
    }

    let threshold = (duration / 2.0).min(240.0);
    position >= threshold
}

fn credentials() -> AppResult<(String, String)> {
    match (API_KEY, SHARED_SECRET) {
        (Some(api_key), Some(secret))
            if !api_key.trim().is_empty() && !secret.trim().is_empty() =>
        {
            Ok((api_key.trim().to_string(), secret.trim().to_string()))
        }
        _ => Err(AppError::Lastfm(
            "Last.fm API credentials are not configured".to_string(),
        )),
    }
}

fn read_settings(app_handle: &AppHandle) -> LastfmSettings {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Some(value) = store.get(KEY_LASTFM)
        && let Ok(settings) = serde_json::from_value(value.clone())
    {
        return settings;
    }
    LastfmSettings::default()
}

fn write_settings(app_handle: &AppHandle, settings: &LastfmSettings) -> AppResult<()> {
    let store = app_handle
        .store(STORE_FILE)
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))?;
    let value = serde_json::to_value(settings)
        .map_err(|e| AppError::Lastfm(format!("failed to serialize Last.fm settings: {e}")))?;
    store.set(KEY_LASTFM, value);
    store
        .save()
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))?;
    Ok(())
}

fn sign_params(params: &[(String, String)], secret: &str) -> String {
    let mut filtered: Vec<(&str, &str)> = params
        .iter()
        .filter(|(key, _)| key != "format" && key != "callback")
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    filtered.sort_by(|left, right| left.0.cmp(right.0));

    let mut input = String::new();
    for (key, value) in filtered {
        input.push_str(key);
        input.push_str(value);
    }
    input.push_str(secret);
    format!("{:x}", md5::compute(input))
}

fn signed_params(mut params: Vec<(String, String)>, secret: &str) -> Vec<(String, String)> {
    let signature = sign_params(&params, secret);
    params.push(("api_sig".to_string(), signature));
    params.push(("format".to_string(), "json".to_string()));
    params
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn now_unix() -> i64 {
    Utc::now().timestamp()
}

fn retry_delay_secs(attempts: i64) -> i64 {
    let exponent = attempts.clamp(0, 6) as u32;
    (60_i64 * 2_i64.pow(exponent)).min(3600)
}

#[derive(Debug, Deserialize)]
struct LastfmTokenResponse {
    token: String,
}

#[derive(Debug, Deserialize)]
struct LastfmSessionEnvelope {
    session: LastfmSessionResponse,
}

#[derive(Debug, Deserialize)]
struct LastfmSessionResponse {
    name: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct LastfmErrorEnvelope {
    error: Option<i64>,
    message: Option<String>,
}

async fn post_lastfm(params: Vec<(String, String)>) -> AppResult<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Lastfm(format!("failed to create Last.fm client: {e}")))?;

    let response = client
        .post(API_ROOT)
        .form(&params)
        .send()
        .await
        .map_err(|e| AppError::Lastfm(e.to_string()))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| AppError::Lastfm(format!("failed to read Last.fm response: {e}")))?;

    let value = serde_json::from_str::<serde_json::Value>(&body).map_err(|e| {
        AppError::Lastfm(format!(
            "failed to decode Last.fm response: {e}; body: {}",
            summarize_response_body(&body)
        ))
    })?;

    if !status.is_success() {
        return Err(AppError::Lastfm(format!("Last.fm HTTP {status}: {value}")));
    }

    if let Ok(error) = serde_json::from_value::<LastfmErrorEnvelope>(value.clone())
        && let Some(code) = error.error
    {
        let message = error.message.unwrap_or_else(|| "unknown error".to_string());
        return Err(AppError::Lastfm(format!("{message} ({code})")));
    }

    Ok(value)
}

fn summarize_response_body(body: &str) -> String {
    if let Some(error) = extract_lastfm_xml_error(body) {
        return error;
    }

    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_LEN: usize = 300;
    if compact.len() > MAX_LEN {
        format!("{}...", &compact[..MAX_LEN])
    } else if compact.is_empty() {
        "<empty>".to_string()
    } else {
        compact
    }
}

fn extract_lastfm_xml_error(body: &str) -> Option<String> {
    let start_tag = body.find("<error")?;
    let after_start = body[start_tag..].find('>')? + start_tag + 1;
    let end = body[after_start..].find("</error>")? + after_start;
    let message = body[after_start..end].trim();

    if message.is_empty() {
        None
    } else {
        Some(message.to_string())
    }
}

async fn get_token(api_key: &str, secret: &str) -> AppResult<String> {
    let params = signed_params(
        vec![
            ("method".to_string(), "auth.getToken".to_string()),
            ("api_key".to_string(), api_key.to_string()),
        ],
        secret,
    );
    let value = post_lastfm(params).await?;
    let response: LastfmTokenResponse = serde_json::from_value(value)
        .map_err(|e| AppError::Lastfm(format!("invalid token response: {e}")))?;
    Ok(response.token)
}

async fn get_session(api_key: &str, secret: &str, token: &str) -> AppResult<LastfmSession> {
    let params = signed_params(
        vec![
            ("method".to_string(), "auth.getSession".to_string()),
            ("api_key".to_string(), api_key.to_string()),
            ("token".to_string(), token.to_string()),
        ],
        secret,
    );
    let value = post_lastfm(params).await?;
    let response: LastfmSessionEnvelope = serde_json::from_value(value)
        .map_err(|e| AppError::Lastfm(format!("invalid session response: {e}")))?;
    Ok(LastfmSession {
        username: response.session.name,
        session_key: response.session.key,
    })
}

pub async fn report_now_playing(
    app_handle: AppHandle,
    song: SongMetadata,
    duration: f64,
) -> AppResult<()> {
    let (api_key, secret) = credentials()?;
    let settings = read_settings(&app_handle);
    if !settings.enabled {
        return Ok(());
    }

    let Some(session) = load_lastfm_session()? else {
        return Ok(());
    };

    if song.title.trim().is_empty() || song.artist.trim().is_empty() {
        return Ok(());
    }

    let mut params = vec![
        ("method".to_string(), "track.updateNowPlaying".to_string()),
        ("api_key".to_string(), api_key.to_string()),
        ("sk".to_string(), session.session_key),
        ("artist".to_string(), song.artist),
        ("track".to_string(), song.title),
    ];
    if let Some(album) = non_empty(&song.album) {
        params.push(("album".to_string(), album));
    }
    if let Some(duration) = finite_i64(duration) {
        params.push(("duration".to_string(), duration.to_string()));
    }

    let params = signed_params(params, &secret);
    post_lastfm(params).await?;
    Ok(())
}

pub fn handle_playback_progress(
    app_handle: &AppHandle,
    state: &AppState,
    song: &SongMetadata,
    position: f64,
    duration: f64,
) {
    let queued = {
        let mut tracker = state.lastfm_tracker.lock_recover();
        tracker.update(song, position, duration)
    };

    let Some(scrobble) = queued else {
        return;
    };

    if scrobble.title.trim().is_empty() || scrobble.artist.trim().is_empty() {
        return;
    }

    let inserted = {
        let conn = state.db.lock_recover();
        match enqueue_scrobble(&conn, &scrobble) {
            Ok(inserted) => inserted,
            Err(e) => {
                warn!("Failed to queue Last.fm scrobble: {e}");
                false
            }
        }
    };

    if inserted {
        let app = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let _ = retry_lastfm_queue_inner(&app, false).await;
        });
    }
}

fn enqueue_scrobble(conn: &Connection, scrobble: &LastfmQueuedScrobble) -> AppResult<bool> {
    let now = now_rfc3339();
    let changed = conn.execute(
        "INSERT OR IGNORE INTO lastfm_scrobble_queue
         (song_id, title, artist, album, duration, played_at, attempts, next_retry_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, ?7, ?7)",
        params![
            scrobble.song_id,
            scrobble.title,
            scrobble.artist,
            scrobble.album,
            scrobble.duration,
            scrobble.played_at,
            now,
        ],
    )?;
    Ok(changed > 0)
}

fn queue_count(conn: &Connection) -> AppResult<i64> {
    Ok(
        conn.query_row("SELECT COUNT(*) FROM lastfm_scrobble_queue", [], |row| {
            row.get(0)
        })?,
    )
}

fn latest_queue_error(conn: &Connection) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT last_error FROM lastfm_scrobble_queue
             WHERE last_error IS NOT NULL
             ORDER BY updated_at DESC, id DESC
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

fn list_queue(conn: &Connection) -> AppResult<Vec<LastfmQueueItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, song_id, title, artist, album, duration, played_at, attempts,
                next_retry_at, last_error, created_at, updated_at
         FROM lastfm_scrobble_queue
         ORDER BY played_at ASC, id ASC
         LIMIT 100",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(LastfmQueueItem {
            id: row.get(0)?,
            song_id: row.get(1)?,
            title: row.get(2)?,
            artist: row.get(3)?,
            album: row.get(4)?,
            duration: row.get(5)?,
            played_at: row.get(6)?,
            attempts: row.get(7)?,
            next_retry_at: row.get(8)?,
            last_error: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

fn due_queue(conn: &Connection, include_not_due: bool) -> AppResult<Vec<LastfmQueueItem>> {
    let now = now_unix();
    let sql = if include_not_due {
        "SELECT id, song_id, title, artist, album, duration, played_at, attempts,
                next_retry_at, last_error, created_at, updated_at
         FROM lastfm_scrobble_queue
         ORDER BY played_at ASC, id ASC
         LIMIT ?1"
    } else {
        "SELECT id, song_id, title, artist, album, duration, played_at, attempts,
                next_retry_at, last_error, created_at, updated_at
         FROM lastfm_scrobble_queue
         WHERE next_retry_at <= ?2
         ORDER BY played_at ASC, id ASC
         LIMIT ?1"
    };

    let mut stmt = conn.prepare(sql)?;
    let map_row = |row: &rusqlite::Row<'_>| {
        Ok(LastfmQueueItem {
            id: row.get(0)?,
            song_id: row.get(1)?,
            title: row.get(2)?,
            artist: row.get(3)?,
            album: row.get(4)?,
            duration: row.get(5)?,
            played_at: row.get(6)?,
            attempts: row.get(7)?,
            next_retry_at: row.get(8)?,
            last_error: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        })
    };

    let rows = if include_not_due {
        stmt.query_map([MAX_BATCH_SIZE as i64], map_row)?
    } else {
        stmt.query_map(params![MAX_BATCH_SIZE as i64, now], map_row)?
    };
    Ok(rows.filter_map(Result::ok).collect())
}

fn mark_batch_success(conn: &Connection, items: &[LastfmQueueItem]) -> AppResult<()> {
    for item in items {
        conn.execute("DELETE FROM lastfm_scrobble_queue WHERE id = ?1", [item.id])?;
    }
    Ok(())
}

fn mark_batch_failure(conn: &Connection, items: &[LastfmQueueItem], error: &str) -> AppResult<()> {
    let now_ts = now_unix();
    let now = now_rfc3339();
    for item in items {
        let attempts = item.attempts + 1;
        let next_retry_at = now_ts + retry_delay_secs(attempts);
        conn.execute(
            "UPDATE lastfm_scrobble_queue
             SET attempts = ?1, next_retry_at = ?2, last_error = ?3, updated_at = ?4
             WHERE id = ?5",
            params![attempts, next_retry_at, error, now, item.id],
        )?;
    }
    Ok(())
}

fn scrobble_batch_params(
    items: &[LastfmQueueItem],
    api_key: &str,
    session_key: &str,
) -> Vec<(String, String)> {
    let mut params = vec![
        ("method".to_string(), "track.scrobble".to_string()),
        ("api_key".to_string(), api_key.to_string()),
        ("sk".to_string(), session_key.to_string()),
    ];

    for (index, item) in items.iter().enumerate() {
        params.push((format!("artist[{index}]"), item.artist.clone()));
        params.push((format!("track[{index}]"), item.title.clone()));
        params.push((format!("timestamp[{index}]"), item.played_at.to_string()));
        if let Some(album) = &item.album
            && !album.trim().is_empty()
        {
            params.push((format!("album[{index}]"), album.clone()));
        }
        if let Some(duration) = item.duration {
            params.push((format!("duration[{index}]"), duration.to_string()));
        }
    }

    params
}

async fn submit_scrobble_batch(
    items: &[LastfmQueueItem],
    session: &LastfmSession,
) -> AppResult<()> {
    let (api_key, secret) = credentials()?;
    if items.is_empty() {
        return Ok(());
    }

    let params = scrobble_batch_params(items, &api_key, &session.session_key);
    let params = signed_params(params, &secret);
    post_lastfm(params).await?;
    Ok(())
}

pub async fn retry_lastfm_queue_inner(
    app_handle: &AppHandle,
    include_not_due: bool,
) -> AppResult<usize> {
    let state: tauri::State<'_, AppState> = app_handle.state();
    if state
        .lastfm_retry_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(0);
    }

    let result = async {
        let settings = read_settings(app_handle);
        if !settings.enabled {
            return Ok(0);
        }

        let Some(session) = load_lastfm_session()? else {
            return Ok(0);
        };

        let items = {
            let conn = state.db.lock_recover();
            due_queue(&conn, include_not_due)?
        };
        if items.is_empty() {
            return Ok(0);
        }

        let count = items.len();
        match submit_scrobble_batch(&items, &session).await {
            Ok(()) => {
                let conn = state.db.lock_recover();
                mark_batch_success(&conn, &items)?;
                debug!("Submitted {count} queued Last.fm scrobbles");
                Ok(count)
            }
            Err(e) => {
                let conn = state.db.lock_recover();
                let message = e.to_string();
                mark_batch_failure(&conn, &items, &message)?;
                Err(e)
            }
        }
    }
    .await;

    state.lastfm_retry_running.store(false, Ordering::SeqCst);
    result
}

pub fn start_lastfm_retry_scheduler(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            if let Err(e) = retry_lastfm_queue_inner(&app_handle, false).await {
                warn!("Last.fm queue retry failed: {e}");
            }
        }
    });
}

pub fn lastfm_status(app_handle: &AppHandle, state: &AppState) -> LastfmStatus {
    let available = credentials().is_ok();
    let settings = read_settings(app_handle);
    let session = load_lastfm_session().ok().flatten();
    let (queue_count, queue_error) = {
        let conn = state.db.lock_recover();
        (
            queue_count(&conn).unwrap_or_default(),
            latest_queue_error(&conn).unwrap_or(None),
        )
    };

    LastfmStatus {
        available,
        authenticated: session.is_some(),
        enabled: settings.enabled,
        username: session.map(|session| session.username),
        pending_auth: settings.pending_token.is_some(),
        queue_count,
        last_error: if available {
            queue_error
        } else {
            Some("Last.fm API credentials are not configured".to_string())
        },
    }
}

pub async fn begin_auth(app_handle: &AppHandle) -> AppResult<LastfmAuthStart> {
    let (api_key, secret) = credentials()?;
    let token = get_token(&api_key, &secret).await?;
    let mut settings = read_settings(app_handle);
    settings.enabled = true;
    settings.pending_token = Some(token.clone());
    write_settings(app_handle, &settings)?;

    Ok(LastfmAuthStart {
        auth_url: format!("{AUTH_ROOT}?api_key={api_key}&token={token}"),
    })
}

pub async fn complete_auth(app_handle: &AppHandle, state: &AppState) -> AppResult<LastfmStatus> {
    let (api_key, secret) = credentials()?;
    let mut settings = read_settings(app_handle);
    let token = settings
        .pending_token
        .clone()
        .ok_or_else(|| AppError::Lastfm("No Last.fm authorization is pending".to_string()))?;
    let session = get_session(&api_key, &secret, &token).await?;
    save_lastfm_session(&session)?;
    settings.enabled = true;
    settings.pending_token = None;
    write_settings(app_handle, &settings)?;

    let _ = retry_lastfm_queue_inner(app_handle, true).await;

    Ok(lastfm_status(app_handle, state))
}

pub fn disconnect(app_handle: &AppHandle, state: &AppState) -> AppResult<LastfmStatus> {
    delete_lastfm_session()?;
    let mut settings = read_settings(app_handle);
    settings.pending_token = None;
    write_settings(app_handle, &settings)?;
    Ok(lastfm_status(app_handle, state))
}

pub fn queue(state: &AppState) -> AppResult<Vec<LastfmQueueItem>> {
    let conn = state.db.lock_recover();
    list_queue(&conn)
}

pub async fn retry_queue(app_handle: &AppHandle) -> AppResult<usize> {
    retry_lastfm_queue_inner(app_handle, true).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_item(id: i64) -> LastfmQueueItem {
        LastfmQueueItem {
            id,
            song_id: format!("song-{id}"),
            title: "Track".to_string(),
            artist: "Artist".to_string(),
            album: Some("Album".to_string()),
            duration: Some(180),
            played_at: 1_700_000_000 + id,
            attempts: 0,
            next_retry_at: 0,
            last_error: None,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn signs_params_in_lastfm_order_and_ignores_format() {
        let params = vec![
            ("b".to_string(), "2".to_string()),
            ("format".to_string(), "json".to_string()),
            ("a".to_string(), "1".to_string()),
        ];
        assert_eq!(
            sign_params(&params, "secret"),
            "670699129dd49818b5abd9e7c2fd6569"
        );
    }

    #[test]
    fn applies_lastfm_scrobble_threshold() {
        assert!(!should_scrobble(15.0, 29.0));
        assert!(!should_scrobble(89.0, 180.0));
        assert!(should_scrobble(90.0, 180.0));
        assert!(!should_scrobble(239.0, 600.0));
        assert!(should_scrobble(240.0, 600.0));
    }

    #[test]
    fn tracker_scrobbles_once_per_playback() {
        let song = SongMetadata {
            id: "1".to_string(),
            title: "Track".to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            cover_art_id: None,
        };
        let mut tracker = LastfmPlaybackTracker::default();
        assert!(tracker.update(&song, 10.0, 120.0).is_none());
        assert!(tracker.update(&song, 60.0, 120.0).is_some());
        assert!(tracker.update(&song, 80.0, 120.0).is_none());
        assert!(tracker.update(&song, 1.0, 120.0).is_none());
        assert!(tracker.update(&song, 60.0, 120.0).is_some());
    }

    #[test]
    fn builds_batched_scrobble_payload() {
        let items = vec![queue_item(1), queue_item(2)];
        let params = scrobble_batch_params(&items, "key", "session");
        assert!(params.contains(&("method".to_string(), "track.scrobble".to_string())));
        assert!(params.contains(&("artist[0]".to_string(), "Artist".to_string())));
        assert!(params.contains(&("timestamp[1]".to_string(), "1700000002".to_string())));
        assert!(params.contains(&("duration[0]".to_string(), "180".to_string())));
    }

    #[test]
    fn retry_backoff_is_capped() {
        assert_eq!(retry_delay_secs(1), 120);
        assert_eq!(retry_delay_secs(99), 3600);
    }

    #[test]
    fn extracts_lastfm_xml_errors() {
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<lfm status="failed">
<error code="11">Access Denied - You cannot access this service</error>
</lfm>"#;
        assert_eq!(
            summarize_response_body(body),
            "Access Denied - You cannot access this service"
        );
    }
}
