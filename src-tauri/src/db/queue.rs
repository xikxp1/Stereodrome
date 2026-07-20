use rusqlite::Connection;

use crate::audio::queue::{QueueItem, RepeatMode};
use crate::error::AppResult;

/// Save all queue items and state to the database in a single transaction
pub fn save_queue(
    conn: &Connection,
    items: &[QueueItem],
    original_order: &[QueueItem],
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
        drop(stmt);

        conn.execute("DELETE FROM queue_original_items", [])?;
        let mut stmt = conn.prepare(
            "INSERT INTO queue_original_items
             (position, song_id, title, artist, album, duration)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for (pos, item) in original_order.iter().enumerate() {
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
    load_ordered_queue_items(conn, "queue_items")
}

pub fn load_queue_original_items(conn: &Connection) -> AppResult<Vec<QueueItem>> {
    load_ordered_queue_items(conn, "queue_original_items")
}

fn load_ordered_queue_items(conn: &Connection, table: &str) -> AppResult<Vec<QueueItem>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_item(id: &str) -> QueueItem {
        QueueItem {
            song_id: id.to_string(),
            title: format!("Song {id}"),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration: 180,
        }
    }

    fn song_ids(items: &[QueueItem]) -> Vec<&str> {
        items.iter().map(|item| item.song_id.as_str()).collect()
    }

    #[test]
    fn queue_persistence_round_trips_original_shuffle_order() {
        let conn = Connection::open_in_memory().expect("open test database");
        crate::db::init_db(&conn).expect("initialize test database");
        let visible = vec![queue_item("c"), queue_item("a"), queue_item("b")];
        let original = vec![queue_item("a"), queue_item("b"), queue_item("c")];

        save_queue(&conn, &visible, &original, Some(0), true, RepeatMode::Off)
            .expect("save shuffled queue");

        let loaded_visible = load_queue_items(&conn).expect("load visible queue order");
        let loaded_original = load_queue_original_items(&conn).expect("load original queue order");
        assert_eq!(song_ids(&loaded_visible), vec!["c", "a", "b"]);
        assert_eq!(song_ids(&loaded_original), vec!["a", "b", "c"]);
    }
}
