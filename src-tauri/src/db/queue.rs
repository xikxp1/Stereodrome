use rusqlite::Connection;

use crate::audio::queue::{QueueItem, RepeatMode};
use crate::error::AppResult;

/// Save all queue items and state to the database in a single transaction
pub fn save_queue(
    conn: &Connection,
    items: &[QueueItem],
    current_index: Option<usize>,
    shuffle: bool,
    repeat_mode: RepeatMode,
) -> AppResult<()> {
    conn.execute("BEGIN IMMEDIATE", [])?;

    let result = (|| {
        // Save items
        conn.execute("DELETE FROM queue_items", [])?;

        let mut stmt = conn.prepare(
            "INSERT INTO queue_items (position, song_id, title, artist, album, duration)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for (pos, item) in items.iter().enumerate() {
            stmt.execute((
                i64::try_from(pos).unwrap_or(i64::MAX),
                &item.song_id,
                &item.title,
                &item.artist,
                &item.album,
                item.duration,
            ))?;
        }

        // Save state
        let repeat_str = match repeat_mode {
            RepeatMode::Off => "Off",
            RepeatMode::All => "All",
            RepeatMode::One => "One",
        };

        conn.execute(
            "INSERT OR REPLACE INTO queue_state (id, current_index, shuffle, repeat_mode)
            VALUES (1, ?1, ?2, ?3)",
            (
                current_index.map(|index| i64::try_from(index).unwrap_or(i64::MAX)),
                i64::from(shuffle),
                repeat_str,
            ),
        )?;

        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute("COMMIT", [])?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(e)
        }
    }
}

/// Load all queue items from the database
pub fn load_queue_items(conn: &Connection) -> AppResult<Vec<QueueItem>> {
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
        .filter_map(std::result::Result::ok)
        .collect();

    Ok(items)
}

/// Load queue state from database
pub fn load_queue_state(conn: &Connection) -> AppResult<(Option<usize>, bool, RepeatMode)> {
    let result = conn.query_row(
        "SELECT current_index, shuffle, repeat_mode FROM queue_state WHERE id = 1",
        [],
        |row| {
            let current_index: Option<i64> = row.get(0)?;
            let shuffle: i64 = row.get(1)?;
            let repeat_mode_str: String = row.get(2)?;
            Ok((current_index, shuffle, repeat_mode_str))
        },
    );

    match result {
        Ok((current_index, shuffle, repeat_mode_str)) => {
            let repeat_mode = match repeat_mode_str.as_str() {
                "All" => RepeatMode::All,
                "One" => RepeatMode::One,
                _ => RepeatMode::Off,
            };
            Ok((
                current_index.and_then(|index| usize::try_from(index).ok()),
                shuffle != 0,
                repeat_mode,
            ))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // No state saved yet, return defaults
            Ok((None, false, RepeatMode::Off))
        }
        Err(e) => Err(e.into()),
    }
}
