use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::models::{
    Album, Artist, AudioProcessingSettings, ConnectivitySettings, Song, SyncSettings,
};
use crate::queue::{QueueItem, RepeatMode};
use crate::{CoreError, CoreResult};

pub const BACKUP_FORMAT: &str = "stereodrome-portable-backup";
pub const BACKUP_VERSION: u32 = 1;
const MAX_BACKUP_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableBackup {
    pub format: String,
    pub version: u32,
    pub created_at: String,
    #[serde(default)]
    pub source_fingerprint: Option<String>,
    pub library: BackupLibrary,
    pub playlists: Vec<BackupPlaylist>,
    pub playlist_songs: Vec<BackupPlaylistSong>,
    pub queue: BackupQueue,
    #[serde(default)]
    pub preferences: PortablePreferences,
    #[serde(default)]
    pub sync_metadata: LibrarySyncMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupLibrary {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPlaylist {
    pub id: String,
    pub name: String,
    pub song_count: i32,
    pub duration: i32,
    pub owner: Option<String>,
    pub cover_art_id: Option<String>,
    pub created_at: String,
    pub changed_at: String,
    pub synced_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPlaylistSong {
    pub playlist_id: String,
    pub song_id: String,
    pub position: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupQueue {
    pub items: Vec<QueueItem>,
    pub original_items: Vec<QueueItem>,
    pub current_index: Option<usize>,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortablePreferences {
    pub sync: Option<SyncSettings>,
    pub connectivity: Option<ConnectivitySettings>,
    pub audio_processing: Option<AudioProcessingSettings>,
    pub volume: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibrarySyncMetadata {
    pub newest_head_album_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupSummary {
    pub artists: usize,
    pub albums: usize,
    pub songs: usize,
    pub playlists: usize,
    pub queue_items: usize,
}

impl PortableBackup {
    #[must_use]
    pub fn summary(&self) -> BackupSummary {
        BackupSummary {
            artists: self.library.artists.len(),
            albums: self.library.albums.len(),
            songs: self.library.songs.len(),
            playlists: self.playlists.len(),
            queue_items: self.queue.items.len(),
        }
    }
}

/// Captures a consistent, validated portable snapshot from the supplied database.
///
/// # Errors
/// Returns an error when database rows cannot be read or the resulting backup is invalid.
pub fn export_from_connection(
    conn: &mut Connection,
    mut preferences: PortablePreferences,
) -> CoreResult<PortableBackup> {
    sanitize_preferences(&mut preferences);
    let tx = conn.transaction()?;
    let backup = PortableBackup {
        format: BACKUP_FORMAT.to_string(),
        version: BACKUP_VERSION,
        created_at: Utc::now().to_rfc3339(),
        source_fingerprint: read_server_fingerprint(&tx)?,
        library: BackupLibrary {
            artists: read_artists(&tx)?,
            albums: read_albums(&tx)?,
            songs: read_songs(&tx)?,
        },
        playlists: read_playlists(&tx)?,
        playlist_songs: read_playlist_songs(&tx)?,
        queue: read_queue(&tx)?,
        preferences,
        sync_metadata: LibrarySyncMetadata {
            newest_head_album_id: read_sync_value(&tx, "library_newest_head_album_id")?,
        },
    };
    tx.commit()?;
    validate(&backup)?;
    Ok(backup)
}

/// Atomically replaces portable database content after validating all references.
///
/// # Errors
/// Returns an error when validation or any database operation fails. The transaction is rolled back.
#[allow(clippy::too_many_lines)]
pub fn import_into_connection(
    conn: &mut Connection,
    backup: &PortableBackup,
) -> CoreResult<BackupSummary> {
    let mut normalized_backup = backup.clone();
    sanitize_preferences(&mut normalized_backup.preferences);
    let backup = &normalized_backup;
    validate(backup)?;
    let tx = conn.transaction()?;
    let destination_fingerprint = read_server_fingerprint(&tx)?;
    if backup.source_fingerprint.is_some()
        && destination_fingerprint.is_some()
        && backup.source_fingerprint != destination_fingerprint
    {
        return invalid("backup belongs to a different server account");
    }
    let mut local_offline_playlist_state = {
        let mut stmt = tx.prepare(
            "SELECT id, offline_saved_at FROM playlists WHERE offline_saved_at IS NOT NULL",
        )?;
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, String>(1)?, HashSet::<String>::new()),
            ))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?
    };
    {
        let mut stmt = tx.prepare("SELECT playlist_id, song_id FROM playlist_songs")?;
        for item in stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (playlist_id, song_id) = item?;
            if let Some((_, song_ids)) = local_offline_playlist_state.get_mut(&playlist_id) {
                song_ids.insert(song_id);
            }
        }
    }
    let downloaded_song_ids = {
        let mut stmt = tx.prepare(
            "SELECT DISTINCT song_id FROM download_items
             WHERE status = 'downloaded' AND path IS NOT NULL",
        )?;
        stmt.query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<HashSet<_>, _>>()?
    };
    let mut imported_playlist_song_ids = HashMap::<&str, HashSet<&str>>::new();
    for item in &backup.playlist_songs {
        imported_playlist_song_ids
            .entry(item.playlist_id.as_str())
            .or_default()
            .insert(item.song_id.as_str());
    }

    tx.execute_batch(
        "DELETE FROM playlist_songs;
         DELETE FROM playlists;
         DELETE FROM queue_original_items;
         DELETE FROM queue_items;
         DELETE FROM queue_state;
         DELETE FROM normalization_data;
         DELETE FROM songs;
         DELETE FROM albums;
         DELETE FROM artists;",
    )?;

    for artist in &backup.library.artists {
        tx.execute(
            "INSERT INTO artists (id, name, album_count, cover_art_id, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                artist.id,
                artist.name,
                artist.album_count,
                artist.cover_art_id,
                artist.synced_at
            ],
        )?;
    }
    for album in &backup.library.albums {
        tx.execute(
            "INSERT INTO albums
             (id, artist_id, name, year, song_count, duration, cover_art_id, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                album.id,
                album.artist_id,
                album.name,
                album.year,
                album.song_count,
                album.duration,
                album.cover_art_id,
                album.synced_at
            ],
        )?;
    }
    for song in &backup.library.songs {
        tx.execute(
            "INSERT INTO songs
             (id, album_id, artist_id, title, track_number, disc_number, duration,
              bit_rate, size, suffix, content_type, path, year, genre, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                song.id,
                song.album_id,
                song.artist_id,
                song.title,
                song.track_number,
                song.disc_number,
                song.duration,
                song.bit_rate,
                song.size,
                song.suffix,
                song.content_type,
                song.path,
                song.year,
                song.genre,
                song.synced_at
            ],
        )?;
    }
    for playlist in &backup.playlists {
        let imported_song_ids = imported_playlist_song_ids
            .get(playlist.id.as_str())
            .cloned()
            .unwrap_or_default();
        let offline_saved_at = local_offline_playlist_state.get(&playlist.id).and_then(
            |(saved_at, local_song_ids)| {
                let membership_matches = local_song_ids.len() == imported_song_ids.len()
                    && imported_song_ids
                        .iter()
                        .all(|song_id| local_song_ids.contains(*song_id));
                let all_downloaded = imported_song_ids
                    .iter()
                    .all(|song_id| downloaded_song_ids.contains(*song_id));
                (membership_matches && all_downloaded).then(|| saved_at.clone())
            },
        );
        tx.execute(
            "INSERT INTO playlists
             (id, name, song_count, duration, owner, cover_art_id, created_at,
              changed_at, offline_saved_at, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                playlist.id,
                playlist.name,
                playlist.song_count,
                playlist.duration,
                playlist.owner,
                playlist.cover_art_id,
                playlist.created_at,
                playlist.changed_at,
                offline_saved_at,
                playlist.synced_at,
            ],
        )?;
    }
    for item in &backup.playlist_songs {
        tx.execute(
            "INSERT INTO playlist_songs (playlist_id, song_id, position)
             VALUES (?1, ?2, ?3)",
            params![item.playlist_id, item.song_id, item.position],
        )?;
    }

    insert_queue_items(&tx, "queue_items", &backup.queue.items)?;
    insert_queue_items(&tx, "queue_original_items", &backup.queue.original_items)?;
    tx.execute(
        "INSERT INTO queue_state (id, current_index, shuffle, repeat_mode)
         VALUES (1, ?1, ?2, ?3)",
        params![
            backup
                .queue
                .current_index
                .map(i64::try_from)
                .transpose()
                .map_err(|_| CoreError::InvalidInput(
                    "queue index exceeds SQLite range".to_string()
                ))?,
            i64::from(backup.queue.shuffle),
            repeat_mode_name(backup.queue.repeat_mode)
        ],
    )?;

    let imported_at = Utc::now().to_rfc3339();
    write_sync_value(&tx, "library_last_success_at", &imported_at)?;
    for prefix in ["library_full", "library_incremental", "library_reconcile"] {
        write_sync_value(&tx, &format!("{prefix}_last_attempt_at"), &imported_at)?;
        write_sync_value(&tx, &format!("{prefix}_last_success_at"), &imported_at)?;
        write_sync_value(&tx, &format!("{prefix}_last_error"), "")?;
    }
    if let Some(head_id) = &backup.sync_metadata.newest_head_album_id {
        write_sync_value(&tx, "library_newest_head_album_id", head_id)?;
    } else {
        tx.execute(
            "DELETE FROM sync_state WHERE key = 'library_newest_head_album_id'",
            [],
        )?;
    }

    if let Some(settings) = &backup.preferences.sync {
        let settings = settings.clone().clamped();
        write_sync_value(&tx, "settings_sync", &serde_json::to_string(&settings)?)?;
    }
    if let Some(settings) = &backup.preferences.connectivity {
        write_sync_value(
            &tx,
            "settings_connectivity",
            &serde_json::to_string(settings)?,
        )?;
    }
    if let Some(settings) = &backup.preferences.audio_processing {
        let mut settings = settings.clone();
        crate::clamp_audio_processing_settings(&mut settings);
        write_sync_value(
            &tx,
            "settings_audio_processing",
            &serde_json::to_string(&settings)?,
        )?;
    }

    tx.execute(
        "DELETE FROM download_items WHERE song_id NOT IN (SELECT id FROM songs)",
        [],
    )?;
    let local_volume = tx
        .query_row(
            "SELECT app_volume FROM playback_state WHERE id = 1",
            [],
            |row| row.get::<_, f64>(0),
        )
        .optional()?
        .unwrap_or(1.0);
    let volume = backup
        .preferences
        .volume
        .unwrap_or(local_volume)
        .clamp(0.0, 1.0);
    tx.execute(
        "INSERT OR REPLACE INTO playback_state
         (id, current_song_id, position_seconds, duration_seconds, was_playing,
          app_volume, now_playing_song_id, scrobbled_song_id, updated_at)
         VALUES (1, NULL, 0, 0, 0, ?1, NULL, NULL, ?2)",
        params![volume, imported_at],
    )?;

    tx.commit()?;
    Ok(backup.summary())
}

/// Reads and validates a portable backup with a bounded file size.
///
/// # Errors
/// Returns an error when the file cannot be read, parsed, or validated.
pub fn read_from_file(path: &Path) -> CoreResult<PortableBackup> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_BACKUP_BYTES {
        return Err(CoreError::InvalidInput(format!(
            "backup exceeds the {} MB size limit",
            MAX_BACKUP_BYTES / 1024 / 1024
        )));
    }
    let mut backup = serde_json::from_slice::<PortableBackup>(&std::fs::read(path)?)?;
    sanitize_preferences(&mut backup.preferences);
    validate(&backup)?;
    Ok(backup)
}

/// Validates and writes a backup through a temporary file.
///
/// # Errors
/// Returns an error when validation or filesystem operations fail.
pub fn write_to_file(path: &Path, backup: &PortableBackup) -> CoreResult<()> {
    validate(backup)?;
    let parent = path.parent().ok_or_else(|| {
        CoreError::InvalidInput("backup destination has no parent directory".to_string())
    })?;
    std::fs::create_dir_all(parent)?;
    let temp_path = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("backup"),
        std::process::id()
    ));
    let result = (|| -> CoreResult<()> {
        let mut file = std::fs::File::create(&temp_path)?;
        serde_json::to_writer(&mut file, backup)?;
        file.flush()?;
        file.sync_all()?;
        replace_file(&temp_path, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp_path);
    }
    result
}

#[cfg(not(target_os = "windows"))]
fn replace_file(temp_path: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(temp_path, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(temp_path: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    if !destination.exists() {
        return std::fs::rename(temp_path, destination);
    }

    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let temp_wide = temp_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: Both path buffers are null-terminated and remain alive for the duration of the call.
    let replaced = unsafe {
        ReplaceFileW(
            destination_wide.as_ptr(),
            temp_wide.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Validates format compatibility and cross-record invariants.
///
/// # Errors
/// Returns an invalid-input error when any invariant is violated.
pub fn validate(backup: &PortableBackup) -> CoreResult<()> {
    if backup.format != BACKUP_FORMAT {
        return invalid("file is not a Stereodrome portable backup");
    }
    if backup.version != BACKUP_VERSION {
        return invalid(format!(
            "unsupported backup version {}; expected {BACKUP_VERSION}",
            backup.version
        ));
    }
    if DateTime::parse_from_rfc3339(&backup.created_at).is_err() {
        return invalid("backup creation timestamp is invalid");
    }
    if backup
        .preferences
        .volume
        .is_some_and(|volume| !volume.is_finite())
    {
        return invalid("backup volume must be finite");
    }

    let artist_ids = unique_ids(
        backup
            .library
            .artists
            .iter()
            .map(|artist| artist.id.as_str()),
        "artist",
    )?;
    let album_ids = unique_ids(
        backup.library.albums.iter().map(|album| album.id.as_str()),
        "album",
    )?;
    let song_ids = unique_ids(
        backup.library.songs.iter().map(|song| song.id.as_str()),
        "song",
    )?;
    let playlist_ids = unique_ids(
        backup.playlists.iter().map(|playlist| playlist.id.as_str()),
        "playlist",
    )?;

    for album in &backup.library.albums {
        if !artist_ids.contains(album.artist_id.as_str()) {
            return invalid(format!("album {} references a missing artist", album.id));
        }
    }
    for song in &backup.library.songs {
        if !artist_ids.contains(song.artist_id.as_str())
            || !album_ids.contains(song.album_id.as_str())
        {
            return invalid(format!("song {} has a missing album or artist", song.id));
        }
    }
    let mut playlist_positions = HashSet::new();
    for item in &backup.playlist_songs {
        if item.position < 0
            || !playlist_ids.contains(item.playlist_id.as_str())
            || !song_ids.contains(item.song_id.as_str())
        {
            return invalid("playlist membership has an invalid reference or position");
        }
        if !playlist_positions.insert((item.playlist_id.as_str(), item.position)) {
            return invalid("playlist contains duplicate positions");
        }
    }
    validate_queue(&backup.queue, &song_ids)?;
    Ok(())
}

fn validate_queue(queue: &BackupQueue, song_ids: &HashSet<&str>) -> CoreResult<()> {
    if queue
        .current_index
        .is_some_and(|index| index >= queue.items.len())
    {
        return invalid("queue current index is out of range");
    }
    if queue
        .items
        .iter()
        .any(|item| !song_ids.contains(item.song_id.as_str()))
        || queue
            .original_items
            .iter()
            .any(|item| !song_ids.contains(item.song_id.as_str()))
    {
        return invalid("queue references a song that is not in the backup");
    }
    if queue.original_items.is_empty() && queue.items.is_empty() {
        return Ok(());
    }
    let counts = |items: &[QueueItem]| {
        let mut counts = HashMap::<String, usize>::new();
        for item in items {
            *counts.entry(item.song_id.clone()).or_default() += 1;
        }
        counts
    };
    if counts(&queue.items) != counts(&queue.original_items) {
        return invalid("queue canonical order does not match visible items");
    }
    Ok(())
}

fn sanitize_preferences(preferences: &mut PortablePreferences) {
    if let Some(sync) = preferences.sync.take() {
        preferences.sync = Some(sync.clamped());
    }
    if let Some(volume) = &mut preferences.volume
        && volume.is_finite()
    {
        *volume = volume.clamp(0.0, 1.0);
    }
    let Some(audio) = &mut preferences.audio_processing else {
        return;
    };
    let defaults = AudioProcessingSettings::default();
    if !audio.target_lufs.is_finite() {
        audio.target_lufs = defaults.target_lufs;
    }
    if !audio.preamp_db.is_finite() {
        audio.preamp_db = defaults.preamp_db;
    }
    for band in &mut audio.equalizer_bands_db {
        if !band.is_finite() {
            *band = 0.0;
        }
    }
    crate::clamp_audio_processing_settings(audio);
    audio.preamp_db = audio.preamp_db.clamp(-10.0, 10.0);
    audio.crossfade_duration_ms = audio.crossfade_duration_ms.clamp(1000, 12_000);
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>, kind: &str) -> CoreResult<HashSet<&'a str>> {
    let mut unique = HashSet::new();
    for id in ids {
        if id.trim().is_empty() {
            return invalid(format!("{kind} ID cannot be empty"));
        }
        if !unique.insert(id) {
            return invalid(format!("backup contains duplicate {kind} ID {id}"));
        }
    }
    Ok(unique)
}

fn invalid<T>(message: impl Into<String>) -> CoreResult<T> {
    Err(CoreError::InvalidInput(message.into()))
}

fn read_artists(conn: &Connection) -> CoreResult<Vec<Artist>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, album_count, cover_art_id, synced_at FROM artists ORDER BY id",
    )?;
    Ok(stmt
        .query_map([], Artist::from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn read_albums(conn: &Connection) -> CoreResult<Vec<Album>> {
    let mut stmt = conn.prepare(
        "SELECT al.id, al.artist_id, al.name, al.year, al.song_count, al.duration,
                al.cover_art_id, al.synced_at, ar.name
         FROM albums al LEFT JOIN artists ar ON ar.id = al.artist_id ORDER BY al.id",
    )?;
    Ok(stmt
        .query_map([], Album::from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn read_songs(conn: &Connection) -> CoreResult<Vec<Song>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.album_id, s.artist_id, s.title, s.track_number, s.disc_number,
                s.duration, s.bit_rate, s.size, s.suffix, s.content_type, s.path,
                s.year, s.genre, s.synced_at, ar.name, al.name
         FROM songs s
         LEFT JOIN artists ar ON ar.id = s.artist_id
         LEFT JOIN albums al ON al.id = s.album_id
         ORDER BY s.id",
    )?;
    Ok(stmt
        .query_map([], Song::from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn read_playlists(conn: &Connection) -> CoreResult<Vec<BackupPlaylist>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, song_count, duration, owner, cover_art_id, created_at,
                changed_at, synced_at
         FROM playlists ORDER BY id",
    )?;
    Ok(stmt
        .query_map([], |row| {
            Ok(BackupPlaylist {
                id: row.get(0)?,
                name: row.get(1)?,
                song_count: row.get(2)?,
                duration: row.get(3)?,
                owner: row.get(4)?,
                cover_art_id: row.get(5)?,
                created_at: row.get(6)?,
                changed_at: row.get(7)?,
                synced_at: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn read_playlist_songs(conn: &Connection) -> CoreResult<Vec<BackupPlaylistSong>> {
    let mut stmt = conn.prepare(
        "SELECT playlist_id, song_id, position
         FROM playlist_songs ORDER BY playlist_id, position",
    )?;
    Ok(stmt
        .query_map([], |row| {
            Ok(BackupPlaylistSong {
                playlist_id: row.get(0)?,
                song_id: row.get(1)?,
                position: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn read_queue(conn: &Connection) -> CoreResult<BackupQueue> {
    let items = read_queue_items(conn, "queue_items")?;
    let original_items = read_queue_items(conn, "queue_original_items")?;
    let state = conn.query_row(
        "SELECT current_index, shuffle, repeat_mode FROM queue_state WHERE id = 1",
        [],
        |row| {
            let current_index = row
                .get::<_, Option<i64>>(0)?
                .map(usize::try_from)
                .transpose()
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Integer,
                        Box::new(error),
                    )
                })?;
            let repeat_mode = match row.get::<_, String>(2)?.as_str() {
                "All" => RepeatMode::All,
                "One" => RepeatMode::One,
                _ => RepeatMode::Off,
            };
            Ok((current_index, row.get::<_, i64>(1)? != 0, repeat_mode))
        },
    );
    let (current_index, shuffle, repeat_mode) = match state {
        Ok(state) => state,
        Err(rusqlite::Error::QueryReturnedNoRows) => (None, false, RepeatMode::Off),
        Err(error) => return Err(error.into()),
    };
    Ok(BackupQueue {
        items,
        original_items,
        current_index,
        shuffle,
        repeat_mode,
    })
}

fn read_queue_items(conn: &Connection, table: &str) -> CoreResult<Vec<QueueItem>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT song_id, title, artist, album, duration FROM {table} ORDER BY position"
    ))?;
    Ok(stmt
        .query_map([], |row| {
            Ok(QueueItem {
                song_id: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                duration: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn insert_queue_items(conn: &Connection, table: &str, items: &[QueueItem]) -> CoreResult<()> {
    let mut stmt = conn.prepare(&format!(
        "INSERT INTO {table} (position, song_id, title, artist, album, duration)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
    ))?;
    for (position, item) in items.iter().enumerate() {
        let position = i64::try_from(position).map_err(|_| {
            CoreError::InvalidInput("queue position exceeds SQLite range".to_string())
        })?;
        stmt.execute(params![
            position,
            item.song_id,
            item.title,
            item.artist,
            item.album,
            item.duration
        ])?;
    }
    Ok(())
}

fn read_sync_value(conn: &Connection, key: &str) -> CoreResult<Option<String>> {
    let result = conn.query_row(
        "SELECT value FROM sync_state WHERE key = ?1",
        [key],
        |row| row.get(0),
    );
    match result {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_server_fingerprint(conn: &Connection) -> CoreResult<Option<String>> {
    let account = conn
        .query_row(
            "SELECT url, username FROM server_config WHERE id = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    Ok(account.map(|(url, username)| {
        let normalized = format!("{}\n{}", url.trim_end_matches('/').to_lowercase(), username);
        format!("{:x}", md5::compute(normalized))
    }))
}

fn write_sync_value(conn: &Connection, key: &str, value: &str) -> CoreResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_state (key, value, updated_at) VALUES (?1, ?2, ?3)",
        params![key, value, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

fn repeat_mode_name(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "Off",
        RepeatMode::All => "All",
        RepeatMode::One => "One",
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use super::*;

    #[test]
    fn portable_backup_round_trip_replaces_data_and_preserves_secrets() {
        let source_path = temp_db_path("backup-source");
        let destination_path = temp_db_path("backup-destination");
        crate::db::init(&source_path).expect("initialize source");
        crate::db::init(&destination_path).expect("initialize destination");
        let mut source = Connection::open(&source_path).expect("open source");
        seed_library(&source, "source");
        source
            .execute(
                "INSERT OR REPLACE INTO server_config (id, url, username)
                 VALUES (1, 'https://server', 'user')",
                [],
            )
            .expect("seed source server config");
        let preferences = PortablePreferences {
            sync: Some(SyncSettings {
                incremental_enabled: false,
                incremental_interval_minutes: 30,
                full_reconcile_enabled: true,
                full_reconcile_interval_hours: 48,
            }),
            connectivity: Some(ConnectivitySettings {
                manual_offline_enabled: true,
            }),
            audio_processing: Some(AudioProcessingSettings::default()),
            volume: Some(0.35),
        };
        let backup = export_from_connection(&mut source, preferences).expect("export backup");

        let mut destination = Connection::open(&destination_path).expect("open destination");
        seed_library(&destination, "destination");
        destination
            .execute(
                "INSERT OR REPLACE INTO server_config (id, url, username)
                 VALUES (1, 'https://server', 'user')",
                [],
            )
            .expect("seed server config");
        destination
            .execute(
                "INSERT INTO artists (id, name, synced_at)
                 VALUES ('source-artist', 'Artist', 'now')",
                [],
            )
            .expect("seed matching local artist");
        destination
            .execute(
                "INSERT INTO albums (id, artist_id, name, synced_at)
                 VALUES ('source-album', 'source-artist', 'Album', 'now')",
                [],
            )
            .expect("seed matching local album");
        destination
            .execute(
                "INSERT INTO songs (id, album_id, artist_id, title, synced_at)
                 VALUES ('source-song', 'source-album', 'source-artist', 'Song', 'now')",
                [],
            )
            .expect("seed matching local song");
        destination
            .execute(
                "INSERT INTO playlists
                 (id, name, created_at, changed_at, offline_saved_at, synced_at)
                 VALUES ('source-playlist', 'Existing Offline Save', 'now', 'now', 'now', 'now')",
                [],
            )
            .expect("seed local offline playlist state");
        destination
            .execute(
                "INSERT INTO playlist_songs (playlist_id, song_id, position)
                 VALUES ('source-playlist', 'source-song', 0)",
                [],
            )
            .expect("seed local offline playlist membership");
        destination
            .execute(
                "INSERT INTO download_items
                 (entity_type, entity_id, song_id, status, path, updated_at)
                 VALUES ('song', 'source-song', 'source-song', 'downloaded', '/cache/song', 'now')",
                [],
            )
            .expect("seed local downloaded song");
        write_sync_value(&destination, "lastfm_session", "secret-session").expect("seed secret");

        let summary =
            import_into_connection(&mut destination, &backup).expect("import portable backup");

        assert_eq!(summary.songs, 1);
        assert_eq!(table_count(&destination, "artists"), 1);
        assert_eq!(table_count(&destination, "playlists"), 1);
        assert_eq!(
            destination
                .query_row(
                    "SELECT offline_saved_at FROM playlists WHERE id = 'source-playlist'",
                    [],
                    |row| row.get::<_, Option<String>>(0)
                )
                .expect("read preserved offline state"),
            Some("now".to_string())
        );
        assert_eq!(
            destination
                .query_row("SELECT id FROM songs", [], |row| row.get::<_, String>(0))
                .expect("read imported song"),
            "source-song"
        );
        assert_eq!(
            destination
                .query_row("SELECT url FROM server_config", [], |row| {
                    row.get::<_, String>(0)
                })
                .expect("read preserved server"),
            "https://server"
        );
        assert_eq!(
            read_sync_value(&destination, "lastfm_session").expect("read secret"),
            Some("secret-session".to_string())
        );
        assert!(
            read_sync_value(&destination, "library_incremental_last_success_at")
                .expect("read imported sync timestamp")
                .is_some()
        );
        assert_eq!(
            destination
                .query_row(
                    "SELECT app_volume FROM playback_state WHERE id = 1",
                    [],
                    |row| row.get::<_, f64>(0)
                )
                .expect("read imported volume"),
            0.35
        );

        drop(source);
        drop(destination);
        std::fs::remove_file(source_path).ok();
        std::fs::remove_file(destination_path).ok();
    }

    #[test]
    fn invalid_backup_version_is_rejected_before_replacement() {
        let path = temp_db_path("backup-invalid-version");
        crate::db::init(&path).expect("initialize database");
        let mut conn = Connection::open(&path).expect("open database");
        seed_library(&conn, "existing");
        let mut backup = export_from_connection(&mut conn, PortablePreferences::default())
            .expect("export backup");
        backup.version += 1;

        assert!(import_into_connection(&mut conn, &backup).is_err());
        assert_eq!(
            conn.query_row("SELECT id FROM songs", [], |row| row.get::<_, String>(0))
                .expect("existing data remains"),
            "existing-song"
        );

        drop(conn);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn backup_for_different_server_is_rejected_before_replacement() {
        let source_path = temp_db_path("backup-server-source");
        let destination_path = temp_db_path("backup-server-destination");
        crate::db::init(&source_path).expect("initialize source");
        crate::db::init(&destination_path).expect("initialize destination");
        let mut source = Connection::open(&source_path).expect("open source");
        seed_library(&source, "source");
        source
            .execute(
                "INSERT INTO server_config (id, url, username)
                 VALUES (1, 'https://source', 'user')",
                [],
            )
            .expect("seed source account");
        let backup = export_from_connection(&mut source, PortablePreferences::default())
            .expect("export backup");

        let mut destination = Connection::open(&destination_path).expect("open destination");
        seed_library(&destination, "destination");
        destination
            .execute(
                "INSERT INTO server_config (id, url, username)
                 VALUES (1, 'https://destination', 'user')",
                [],
            )
            .expect("seed destination account");

        assert!(import_into_connection(&mut destination, &backup).is_err());
        assert_eq!(
            destination
                .query_row("SELECT id FROM songs", [], |row| row.get::<_, String>(0))
                .expect("destination data remains"),
            "destination-song"
        );

        drop(source);
        drop(destination);
        std::fs::remove_file(source_path).ok();
        std::fs::remove_file(destination_path).ok();
    }

    #[test]
    fn serialized_backup_excludes_connection_and_lastfm_secrets() {
        let path = temp_db_path("backup-no-secrets");
        crate::db::init(&path).expect("initialize database");
        let mut conn = Connection::open(&path).expect("open database");
        seed_library(&conn, "source");
        conn.execute(
            "INSERT OR REPLACE INTO server_config (id, url, username)
             VALUES (1, 'https://private.example', 'private-user')",
            [],
        )
        .expect("seed server config");
        write_sync_value(&conn, "lastfm_session", "private-session").expect("seed lastfm");

        let backup = export_from_connection(&mut conn, PortablePreferences::default())
            .expect("export backup");
        let json = serde_json::to_string(&backup).expect("serialize backup");

        assert!(!json.contains("private.example"));
        assert!(!json.contains("private-user"));
        assert!(!json.contains("private-session"));

        drop(conn);
        std::fs::remove_file(path).ok();
    }

    fn seed_library(conn: &Connection, prefix: &str) {
        let artist_id = format!("{prefix}-artist");
        let album_id = format!("{prefix}-album");
        let song_id = format!("{prefix}-song");
        let playlist_id = format!("{prefix}-playlist");
        conn.execute(
            "INSERT INTO artists (id, name, synced_at) VALUES (?1, 'Artist', 'now')",
            [&artist_id],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at)
             VALUES (?1, ?2, 'Album', 'now')",
            params![album_id, artist_id],
        )
        .expect("insert album");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, synced_at)
             VALUES (?1, ?2, ?3, 'Song', 'now')",
            params![song_id, album_id, artist_id],
        )
        .expect("insert song");
        conn.execute(
            "INSERT INTO playlists
             (id, name, created_at, changed_at, synced_at)
             VALUES (?1, 'Playlist', 'now', 'now', 'now')",
            [&playlist_id],
        )
        .expect("insert playlist");
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_id, position)
             VALUES (?1, ?2, 0)",
            params![playlist_id, song_id],
        )
        .expect("insert playlist song");
        for table in ["queue_items", "queue_original_items"] {
            conn.execute(
                &format!(
                    "INSERT INTO {table}
                     (position, song_id, title, artist, album, duration)
                     VALUES (0, ?1, 'Song', 'Artist', 'Album', 180)"
                ),
                [&song_id],
            )
            .expect("insert queue item");
        }
        conn.execute(
            "INSERT OR REPLACE INTO queue_state
             (id, current_index, shuffle, repeat_mode) VALUES (1, 0, 0, 'Off')",
            [],
        )
        .expect("insert queue state");
    }

    fn table_count(conn: &Connection, table: &str) -> usize {
        let count = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count rows");
        usize::try_from(count).expect("non-negative row count")
    }

    fn temp_db_path(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "stereodrome-{label}-{}-{nonce}.db",
            std::process::id()
        ))
    }
}
