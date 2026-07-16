use std::path::Path;

use rusqlite::Connection;

use crate::CoreResult;
use crate::queue::{PlayQueue, QueueItem, QueueState, RepeatMode};

use crate::DESKTOP_SCHEMA;

pub const SONG_SELECT_WITH_JOINS: &str = "
    SELECT s.id, s.album_id, s.artist_id, s.title, s.track_number, s.disc_number,
           s.duration, s.bit_rate, s.size, s.suffix, s.content_type, s.path,
           s.year, s.genre, s.synced_at, ar.name, al.name
    FROM songs s
    LEFT JOIN artists ar ON s.artist_id = ar.id
    LEFT JOIN albums al ON s.album_id = al.id
    WHERE s.album_id = ?1
    ORDER BY s.disc_number, s.track_number, s.title COLLATE NOCASE";

pub const SONG_SELECT_BY_ARTIST: &str = "
    SELECT s.id, s.album_id, s.artist_id, s.title, s.track_number, s.disc_number,
           s.duration, s.bit_rate, s.size, s.suffix, s.content_type, s.path,
           s.year, s.genre, s.synced_at, ar.name, al.name
    FROM songs s
    LEFT JOIN artists ar ON s.artist_id = ar.id
    LEFT JOIN albums al ON s.album_id = al.id
    WHERE s.artist_id = ?1
    ORDER BY al.year, al.name COLLATE NOCASE, s.disc_number, s.track_number";

pub const SONG_SELECT_ALL: &str = "
    SELECT s.id, s.album_id, s.artist_id, s.title, s.track_number, s.disc_number,
           s.duration, s.bit_rate, s.size, s.suffix, s.content_type, s.path,
           s.year, s.genre, s.synced_at, ar.name, al.name
    FROM songs s
    LEFT JOIN artists ar ON s.artist_id = ar.id
    LEFT JOIN albums al ON s.album_id = al.id
    ORDER BY s.title COLLATE NOCASE";

pub fn init(path: &Path) -> CoreResult<()> {
    let conn = Connection::open(path)?;
    conn.execute_batch(DESKTOP_SCHEMA)?;
    run_migrations(&conn)?;
    Ok(())
}

fn run_migrations(conn: &Connection) -> CoreResult<()> {
    let playlist_columns: Vec<String> = conn
        .prepare("PRAGMA table_info(playlists)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|row| row.ok())
        .collect();

    if !playlist_columns.contains(&"offline_saved_at".to_string()) {
        conn.execute("ALTER TABLE playlists ADD COLUMN offline_saved_at TEXT", [])?;
    }

    Ok(())
}

pub fn load_queue(path: &Path) -> CoreResult<PlayQueue> {
    let conn = Connection::open(path)?;
    let items = load_queue_items(&conn)?;
    let (current_index, shuffle, repeat_mode) = load_queue_state(&conn)?;
    Ok(PlayQueue::load(items, current_index, shuffle, repeat_mode))
}

pub fn save_queue(path: &Path, state: &QueueState) -> CoreResult<()> {
    let mut conn = Connection::open(path)?;
    let tx = conn.transaction()?;

    tx.execute("DELETE FROM queue_items", [])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO queue_items (position, song_id, title, artist, album, duration)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for (pos, item) in state.items.iter().enumerate() {
            stmt.execute((
                pos as i64,
                &item.song_id,
                &item.title,
                &item.artist,
                &item.album,
                item.duration,
            ))?;
        }
    }

    let repeat_mode = match state.repeat_mode {
        RepeatMode::Off => "Off",
        RepeatMode::All => "All",
        RepeatMode::One => "One",
    };

    tx.execute(
        "INSERT OR REPLACE INTO queue_state (id, current_index, shuffle, repeat_mode)
         VALUES (1, ?1, ?2, ?3)",
        (
            state.current_index.map(|i| i as i64),
            state.shuffle as i64,
            repeat_mode,
        ),
    )?;

    tx.commit()?;
    Ok(())
}

fn load_queue_items(conn: &Connection) -> CoreResult<Vec<QueueItem>> {
    let mut stmt = conn.prepare(
        "SELECT song_id, title, artist, album, duration FROM queue_items ORDER BY position",
    )?;

    let items = stmt
        .query_map([], |row| {
            Ok(QueueItem {
                song_id: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                duration: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(items)
}

fn load_queue_state(conn: &Connection) -> CoreResult<(Option<usize>, bool, RepeatMode)> {
    let result = conn.query_row(
        "SELECT current_index, shuffle, repeat_mode FROM queue_state WHERE id = 1",
        [],
        |row| {
            let current_index: Option<i64> = row.get(0)?;
            let shuffle: i64 = row.get(1)?;
            let repeat_mode: String = row.get(2)?;
            Ok((current_index, shuffle, repeat_mode))
        },
    );

    match result {
        Ok((current_index, shuffle, repeat_mode)) => {
            let repeat_mode = match repeat_mode.as_str() {
                "All" => RepeatMode::All,
                "One" => RepeatMode::One,
                _ => RepeatMode::Off,
            };
            Ok((current_index.map(|i| i as usize), shuffle != 0, repeat_mode))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok((None, false, RepeatMode::Off)),
        Err(error) => Err(error.into()),
    }
}
