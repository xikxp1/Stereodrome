use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult};

const API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTH_ROOT: &str = "https://www.last.fm/api/auth/";
const API_KEY: Option<&str> = option_env!("LASTFM_API_KEY");
const SHARED_SECRET: Option<&str> = option_env!("LASTFM_SHARED_SECRET");
const KEY_ENABLED: &str = "lastfm_enabled";
const KEY_PENDING_TOKEN: &str = "lastfm_pending_token";
const KEY_SESSION: &str = "lastfm_session";
const KEY_PLAYBACK_SONG_ID: &str = "lastfm_playback_song_id";
const KEY_PLAYBACK_STARTED_AT: &str = "lastfm_playback_started_at";
const KEY_PLAYBACK_LAST_POSITION: &str = "lastfm_playback_last_position";
const KEY_PLAYBACK_SCROBBLED_SONG_ID: &str = "lastfm_playback_scrobbled_song_id";
const MAX_BATCH_SIZE: i64 = 50;

// These flags are independent wire-format fields consumed by the mobile UI.
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastfmAuthStart {
    pub auth_url: String,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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

#[derive(Debug, Clone)]
pub struct LastfmTrack {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct LastfmQueuedScrobble {
    pub track: LastfmTrack,
    pub played_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LastfmSession {
    username: String,
    session_key: String,
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

pub fn should_scrobble(position: f64, duration: f64) -> bool {
    if !position.is_finite() || !duration.is_finite() || duration <= 30.0 {
        return false;
    }

    position >= (duration / 2.0).min(240.0)
}

pub fn track_for_song(db_path: &Path, song_id: &str) -> CoreResult<Option<LastfmTrack>> {
    let conn = Connection::open(db_path)?;
    let track = conn
        .query_row(
            "SELECT s.title, ar.name, al.name, s.duration
             FROM songs s
             LEFT JOIN artists ar ON s.artist_id = ar.id
             LEFT JOIN albums al ON s.album_id = al.id
             WHERE s.id = ?1",
            [song_id],
            |row| {
                Ok(LastfmTrack {
                    song_id: song_id.to_string(),
                    title: row.get::<_, String>(0)?,
                    artist: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    album: row
                        .get::<_, Option<String>>(2)?
                        .filter(|value| !value.is_empty()),
                    duration: row.get::<_, Option<i64>>(3)?,
                })
            },
        )
        .optional()?;
    Ok(track)
}

pub fn enqueue_scrobble(db_path: &Path, scrobble: &LastfmQueuedScrobble) -> CoreResult<bool> {
    if scrobble.track.title.trim().is_empty() || scrobble.track.artist.trim().is_empty() {
        return Ok(false);
    }

    let conn = Connection::open(db_path)?;
    let now = Utc::now().to_rfc3339();
    let changed = conn.execute(
        "INSERT OR IGNORE INTO lastfm_scrobble_queue
         (song_id, title, artist, album, duration, played_at, attempts, next_retry_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, ?7, ?7)",
        params![
            scrobble.track.song_id,
            scrobble.track.title,
            scrobble.track.artist,
            scrobble.track.album,
            scrobble.track.duration,
            scrobble.played_at,
            now,
        ],
    )?;
    Ok(changed > 0)
}

pub fn maybe_enqueue_from_progress(
    db_path: &Path,
    track: LastfmTrack,
    position_seconds: f64,
    duration_seconds: f64,
) -> CoreResult<bool> {
    let current_song_id = sync_value(db_path, KEY_PLAYBACK_SONG_ID)?;
    let last_position = sync_value(db_path, KEY_PLAYBACK_LAST_POSITION)?
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    let song_changed = current_song_id.as_deref() != Some(track.song_id.as_str());
    let restarted = !song_changed && position_seconds < last_position && position_seconds < 5.0;

    if song_changed || restarted {
        write_sync_value(db_path, KEY_PLAYBACK_SONG_ID, &track.song_id)?;
        write_sync_value(
            db_path,
            KEY_PLAYBACK_STARTED_AT,
            &Utc::now().timestamp().to_string(),
        )?;
        write_sync_value(db_path, KEY_PLAYBACK_SCROBBLED_SONG_ID, "")?;
    }

    write_sync_value(
        db_path,
        KEY_PLAYBACK_LAST_POSITION,
        &position_seconds.to_string(),
    )?;

    let already_scrobbled = sync_value(db_path, KEY_PLAYBACK_SCROBBLED_SONG_ID)?.as_deref()
        == Some(track.song_id.as_str());
    if already_scrobbled || !should_scrobble(position_seconds, duration_seconds) {
        return Ok(false);
    }

    let played_at = sync_value(db_path, KEY_PLAYBACK_STARTED_AT)?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_else(|| Utc::now().timestamp());
    let inserted = enqueue_scrobble(db_path, &LastfmQueuedScrobble { track, played_at })?;
    write_sync_value(
        db_path,
        KEY_PLAYBACK_SCROBBLED_SONG_ID,
        sync_value(db_path, KEY_PLAYBACK_SONG_ID)?
            .as_deref()
            .unwrap_or_default(),
    )?;
    Ok(inserted)
}

pub async fn report_now_playing(db_path: &Path, track: &LastfmTrack) -> CoreResult<()> {
    let (api_key, secret) = credentials()?;
    if !enabled(db_path)? || track.title.trim().is_empty() || track.artist.trim().is_empty() {
        return Ok(());
    }

    let Some(session) = load_session(db_path)? else {
        return Ok(());
    };

    let mut params = vec![
        ("method".to_string(), "track.updateNowPlaying".to_string()),
        ("api_key".to_string(), api_key),
        ("sk".to_string(), session.session_key),
        ("artist".to_string(), track.artist.clone()),
        ("track".to_string(), track.title.clone()),
    ];
    if let Some(album) = &track.album {
        params.push(("album".to_string(), album.clone()));
    }
    if let Some(duration) = track.duration {
        params.push(("duration".to_string(), duration.to_string()));
    }

    post_lastfm(signed_params(params, &secret)).await?;
    Ok(())
}

pub async fn retry_queue(db_path: &Path, include_not_due: bool) -> CoreResult<usize> {
    if !enabled(db_path)? {
        return Ok(0);
    }

    let Some(session) = load_session(db_path)? else {
        return Ok(0);
    };

    let items = due_queue(db_path, include_not_due)?;
    if items.is_empty() {
        return Ok(0);
    }

    let count = items.len();
    match submit_scrobble_batch(&items, &session).await {
        Ok(()) => {
            mark_batch_success(db_path, &items)?;
            Ok(count)
        }
        Err(error) => {
            mark_batch_failure(db_path, &items, &error.to_string())?;
            Err(error)
        }
    }
}

pub fn status(db_path: &Path) -> LastfmStatus {
    let available = credentials().is_ok();
    let session = load_session(db_path).ok().flatten();
    LastfmStatus {
        available,
        authenticated: session.is_some(),
        enabled: enabled(db_path).unwrap_or(true),
        username: session.map(|session| session.username),
        pending_auth: sync_value(db_path, KEY_PENDING_TOKEN)
            .ok()
            .flatten()
            .is_some(),
        queue_count: queue_count(db_path).unwrap_or_default(),
        last_error: if available {
            latest_queue_error(db_path).unwrap_or(None)
        } else {
            Some("Last.fm API credentials are not configured".to_string())
        },
    }
}

pub async fn begin_auth(db_path: &Path) -> CoreResult<LastfmAuthStart> {
    let (api_key, secret) = credentials()?;
    let token = get_token(&api_key, &secret).await?;
    write_sync_value(db_path, KEY_ENABLED, "true")?;
    write_sync_value(db_path, KEY_PENDING_TOKEN, &token)?;
    Ok(LastfmAuthStart {
        auth_url: format!("{AUTH_ROOT}?api_key={api_key}&token={token}"),
    })
}

pub async fn complete_auth(db_path: &Path) -> CoreResult<LastfmStatus> {
    let (api_key, secret) = credentials()?;
    let token = sync_value(db_path, KEY_PENDING_TOKEN)?
        .ok_or_else(|| CoreError::Lastfm("No Last.fm authorization is pending".to_string()))?;
    let session = get_session(&api_key, &secret, &token).await?;
    write_sync_value(db_path, KEY_SESSION, &serde_json::to_string(&session)?)?;
    write_sync_value(db_path, KEY_PENDING_TOKEN, "")?;
    write_sync_value(db_path, KEY_ENABLED, "true")?;
    Ok(status(db_path))
}

pub fn disconnect(db_path: &Path) -> CoreResult<LastfmStatus> {
    write_sync_value(db_path, KEY_SESSION, "")?;
    write_sync_value(db_path, KEY_PENDING_TOKEN, "")?;
    Ok(status(db_path))
}

pub fn list_queue(db_path: &Path) -> CoreResult<Vec<LastfmQueueItem>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, song_id, title, artist, album, duration, played_at, attempts,
                next_retry_at, last_error, created_at, updated_at
         FROM lastfm_scrobble_queue
         ORDER BY played_at ASC, id ASC
         LIMIT 100",
    )?;
    let rows = stmt.query_map([], queue_item_from_row)?;
    Ok(rows.filter_map(Result::ok).collect())
}

fn credentials() -> CoreResult<(String, String)> {
    match (API_KEY, SHARED_SECRET) {
        (Some(api_key), Some(secret))
            if !api_key.trim().is_empty() && !secret.trim().is_empty() =>
        {
            Ok((api_key.trim().to_string(), secret.trim().to_string()))
        }
        _ => Err(CoreError::Lastfm(
            "Last.fm API credentials are not configured".to_string(),
        )),
    }
}

fn enabled(db_path: &Path) -> CoreResult<bool> {
    Ok(sync_value(db_path, KEY_ENABLED)?.as_deref() != Some("false"))
}

fn load_session(db_path: &Path) -> CoreResult<Option<LastfmSession>> {
    let Some(json) = sync_value(db_path, KEY_SESSION)?.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_str(&json)?))
}

pub(crate) fn import_session_if_missing(
    db_path: &Path,
    username: String,
    session_key: String,
) -> CoreResult<()> {
    if load_session(db_path)?.is_some() {
        return Ok(());
    }
    let session = LastfmSession {
        username,
        session_key,
    };
    write_sync_value(db_path, KEY_SESSION, &serde_json::to_string(&session)?)?;
    write_sync_value(db_path, KEY_ENABLED, "true")
}

fn sync_value(db_path: &Path, key: &str) -> CoreResult<Option<String>> {
    let conn = Connection::open(db_path)?;
    let value = conn
        .query_row(
            "SELECT value FROM sync_state WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.filter(|value: &String| !value.is_empty()))
}

fn write_sync_value(db_path: &Path, key: &str, value: &str) -> CoreResult<()> {
    let conn = Connection::open(db_path)?;
    conn.execute(
        "INSERT OR REPLACE INTO sync_state (key, value, updated_at) VALUES (?1, ?2, ?3)",
        params![key, value, Utc::now().to_rfc3339()],
    )?;
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

async fn post_lastfm(params: Vec<(String, String)>) -> CoreResult<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| CoreError::Lastfm(format!("failed to create Last.fm client: {e}")))?;

    let response = client
        .post(API_ROOT)
        .form(&params)
        .send()
        .await
        .map_err(|e| CoreError::Lastfm(e.to_string()))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| CoreError::Lastfm(format!("failed to read Last.fm response: {e}")))?;
    let value = serde_json::from_str::<serde_json::Value>(&body).map_err(|e| {
        CoreError::Lastfm(format!(
            "failed to decode Last.fm response: {e}; body: {}",
            summarize_response_body(&body)
        ))
    })?;

    if !status.is_success() {
        return Err(CoreError::Lastfm(format!("Last.fm HTTP {status}: {value}")));
    }

    if let Ok(error) = serde_json::from_value::<LastfmErrorEnvelope>(value.clone())
        && let Some(code) = error.error
    {
        let message = error.message.unwrap_or_else(|| "unknown error".to_string());
        return Err(CoreError::Lastfm(format!("{message} ({code})")));
    }

    Ok(value)
}

fn summarize_response_body(body: &str) -> String {
    const MAX_LEN: usize = 300;

    if let Some(error) = extract_lastfm_xml_error(body) {
        return error;
    }

    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
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
    (!message.is_empty()).then(|| message.to_string())
}

async fn get_token(api_key: &str, secret: &str) -> CoreResult<String> {
    let value = post_lastfm(signed_params(
        vec![
            ("method".to_string(), "auth.getToken".to_string()),
            ("api_key".to_string(), api_key.to_string()),
        ],
        secret,
    ))
    .await?;
    let response: LastfmTokenResponse = serde_json::from_value(value)
        .map_err(|e| CoreError::Lastfm(format!("invalid token response: {e}")))?;
    Ok(response.token)
}

async fn get_session(api_key: &str, secret: &str, token: &str) -> CoreResult<LastfmSession> {
    let value = post_lastfm(signed_params(
        vec![
            ("method".to_string(), "auth.getSession".to_string()),
            ("api_key".to_string(), api_key.to_string()),
            ("token".to_string(), token.to_string()),
        ],
        secret,
    ))
    .await?;
    let response: LastfmSessionEnvelope = serde_json::from_value(value)
        .map_err(|e| CoreError::Lastfm(format!("invalid session response: {e}")))?;
    Ok(LastfmSession {
        username: response.session.name,
        session_key: response.session.key,
    })
}

fn due_queue(db_path: &Path, include_not_due: bool) -> CoreResult<Vec<LastfmQueueItem>> {
    let conn = Connection::open(db_path)?;
    let now = Utc::now().timestamp();
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
    let rows = if include_not_due {
        stmt.query_map([MAX_BATCH_SIZE], queue_item_from_row)?
    } else {
        stmt.query_map(params![MAX_BATCH_SIZE, now], queue_item_from_row)?
    };
    Ok(rows.filter_map(Result::ok).collect())
}

fn queue_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LastfmQueueItem> {
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
}

fn submit_params(
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
) -> CoreResult<()> {
    let (api_key, secret) = credentials()?;
    post_lastfm(signed_params(
        submit_params(items, &api_key, &session.session_key),
        &secret,
    ))
    .await?;
    Ok(())
}

fn mark_batch_success(db_path: &Path, items: &[LastfmQueueItem]) -> CoreResult<()> {
    let conn = Connection::open(db_path)?;
    for item in items {
        conn.execute("DELETE FROM lastfm_scrobble_queue WHERE id = ?1", [item.id])?;
    }
    Ok(())
}

fn mark_batch_failure(db_path: &Path, items: &[LastfmQueueItem], error: &str) -> CoreResult<()> {
    let conn = Connection::open(db_path)?;
    let now_ts = Utc::now().timestamp();
    let now = Utc::now().to_rfc3339();
    for item in items {
        let attempts = item.attempts + 1;
        let delay = retry_delay_secs(attempts);
        conn.execute(
            "UPDATE lastfm_scrobble_queue
             SET attempts = ?1, next_retry_at = ?2, last_error = ?3, updated_at = ?4
             WHERE id = ?5",
            params![attempts, now_ts + delay, error, now, item.id],
        )?;
    }
    Ok(())
}

fn retry_delay_secs(attempts: i64) -> i64 {
    let exponent = u32::try_from(attempts.clamp(0, 6)).unwrap_or_default();
    (60_i64 * 2_i64.pow(exponent)).min(3600)
}

fn queue_count(db_path: &Path) -> CoreResult<i64> {
    let conn = Connection::open(db_path)?;
    Ok(
        conn.query_row("SELECT COUNT(*) FROM lastfm_scrobble_queue", [], |row| {
            row.get(0)
        })?,
    )
}

fn latest_queue_error(db_path: &Path) -> CoreResult<Option<String>> {
    let conn = Connection::open(db_path)?;
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
    fn builds_batched_scrobble_payload() {
        let items = vec![queue_item(1), queue_item(2)];
        let params = submit_params(&items, "key", "session");
        assert!(params.contains(&("method".to_string(), "track.scrobble".to_string())));
        assert!(params.contains(&("artist[0]".to_string(), "Artist".to_string())));
        assert!(params.contains(&("timestamp[1]".to_string(), "1700000002".to_string())));
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

    #[test]
    fn retry_backoff_is_capped() {
        assert_eq!(retry_delay_secs(1), 120);
        assert_eq!(retry_delay_secs(99), 3600);
    }
}
