use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::CoreResult;
use crate::queue::{PlayQueue, QueueItem, QueueState, RepeatMode};

const SCHEMA: &str = include_str!("../../../src-tauri/src/db/schema.sql");

/// Opens a connection with the pragmas every core connection relies on:
/// `synchronous=NORMAL` so WAL commits skip the per-transaction fsync, and a
/// busy timeout so concurrent writers back off instead of failing.
pub fn open_connection(path: &Path) -> CoreResult<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

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
    let conn = open_connection(path)?;
    // WAL is a persistent database property; setting it once here covers
    // every later connection, including the desktop shell's direct opens.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(SCHEMA)?;
    run_migrations(&conn)?;
    Ok(())
}

fn run_migrations(conn: &Connection) -> CoreResult<()> {
    let playlist_columns: Vec<String> = conn
        .prepare("PRAGMA table_info(playlists)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(std::result::Result::ok)
        .collect();

    if !playlist_columns.contains(&"offline_saved_at".to_string()) {
        conn.execute("ALTER TABLE playlists ADD COLUMN offline_saved_at TEXT", [])?;
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS queue_original_items (
            position INTEGER PRIMARY KEY,
            song_id TEXT NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT NOT NULL,
            duration INTEGER NOT NULL
        );",
    )?;

    Ok(())
}

pub fn load_queue(path: &Path) -> CoreResult<PlayQueue> {
    let conn = open_connection(path)?;
    let items = load_queue_items(&conn)?;
    let original_order = load_queue_original_items(&conn)?;
    let (current_index, shuffle, repeat_mode) = load_queue_state(&conn)?;
    Ok(PlayQueue::load_with_original_order(
        items,
        original_order,
        current_index,
        shuffle,
        repeat_mode,
    ))
}

pub fn save_queue(path: &Path, state: &QueueState, original_order: &[QueueItem]) -> CoreResult<()> {
    let mut conn = open_connection(path)?;
    let tx = conn.transaction()?;

    tx.execute("DELETE FROM queue_items", [])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO queue_items (position, song_id, title, artist, album, duration)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for (pos, item) in state.items.iter().enumerate() {
            let pos = i64::try_from(pos).map_err(|_| {
                crate::CoreError::InvalidInput("queue position exceeds SQLite range".to_string())
            })?;
            stmt.execute((
                pos,
                &item.song_id,
                &item.title,
                &item.artist,
                &item.album,
                item.duration,
            ))?;
        }
    }

    tx.execute("DELETE FROM queue_original_items", [])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO queue_original_items
             (position, song_id, title, artist, album, duration)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for (pos, item) in original_order.iter().enumerate() {
            let pos = i64::try_from(pos).map_err(|_| {
                crate::CoreError::InvalidInput("queue position exceeds SQLite range".to_string())
            })?;
            stmt.execute((
                pos,
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

    let current_index = state
        .current_index
        .map(i64::try_from)
        .transpose()
        .map_err(|_| {
            crate::CoreError::InvalidInput("queue index exceeds SQLite range".to_string())
        })?;
    tx.execute(
        "INSERT OR REPLACE INTO queue_state (id, current_index, shuffle, repeat_mode)
         VALUES (1, ?1, ?2, ?3)",
        (current_index, i64::from(state.shuffle), repeat_mode),
    )?;

    tx.commit()?;
    Ok(())
}

fn load_queue_items(conn: &Connection) -> CoreResult<Vec<QueueItem>> {
    load_ordered_queue_items(conn, "queue_items")
}

fn load_queue_original_items(conn: &Connection) -> CoreResult<Vec<QueueItem>> {
    load_ordered_queue_items(conn, "queue_original_items")
}

fn load_ordered_queue_items(conn: &Connection, table: &str) -> CoreResult<Vec<QueueItem>> {
    let mut stmt = conn.prepare(
        format!("SELECT song_id, title, artist, album, duration FROM {table} ORDER BY position")
            .as_str(),
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
            let current_index = current_index
                .map(usize::try_from)
                .transpose()
                .map_err(|_| {
                    crate::CoreError::InvalidInput(
                        "persisted queue index is outside the supported range".to_string(),
                    )
                })?;
            Ok((current_index, shuffle != 0, repeat_mode))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok((None, false, RepeatMode::Off)),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn queue_item(id: &str) -> QueueItem {
        QueueItem {
            song_id: id.to_string(),
            title: format!("Song {id}"),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration: 180,
        }
    }

    #[test]
    fn shuffled_queue_restores_canonical_order_after_reload() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "stereodrome-queue-persistence-{}-{nonce}.db",
            std::process::id()
        ));
        init(&path).expect("initialize test database");

        let mut queue = PlayQueue::load_with_original_order(
            vec![queue_item("c"), queue_item("a"), queue_item("b")],
            vec![queue_item("a"), queue_item("b"), queue_item("c")],
            Some(0),
            true,
            RepeatMode::Off,
        );
        let state = QueueState::from_queue(&mut queue);
        save_queue(&path, &state, queue.original_order()).expect("save shuffled queue");

        let mut restored = load_queue(&path).expect("reload shuffled queue");
        restored.toggle_shuffle();

        assert_eq!(
            restored
                .items()
                .iter()
                .map(|item| item.song_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(restored.current_index(), Some(2));

        std::fs::remove_file(path).expect("remove test database");
    }
}
