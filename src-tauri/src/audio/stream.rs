use submarine::Client;

use crate::error::{AppError, AppResult};

/// Fetch audio bytes from the Subsonic server using submarine client
pub async fn fetch_audio_bytes(client: &Client, song_id: &str) -> AppResult<Vec<u8>> {
    client
        .stream(
            song_id,
            None::<i32>,    // max_bit_rate
            None::<String>, // format
            None::<i64>,    // time_offset
            None::<String>, // size
            None,           // estimate_content_length
            None,           // converted
        )
        .await
        .map_err(|e| AppError::Audio(format!("Failed to fetch audio: {}", e)))
}
