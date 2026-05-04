use std::path::Path;

use rusqlite::Connection;

use crate::CoreResult;

const SCHEMA: &str = include_str!("../../../src-tauri/src/db/schema.sql");

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
    conn.execute_batch(SCHEMA)?;
    Ok(())
}
