use crate::error::{AppError, AppResult};
use crate::state::ServerConfig;

/// Build the stream URL for a song from the Subsonic server
pub fn build_stream_url(config: &ServerConfig, song_id: &str) -> String {
    let auth_params = build_auth_params(config);
    format!(
        "{}/rest/stream?id={}&{}",
        config.url.trim_end_matches('/'),
        song_id,
        auth_params
    )
}

/// Build authentication parameters for Subsonic API requests
fn build_auth_params(config: &ServerConfig) -> String {
    use md5::{Digest, Md5};
    use rand::Rng;

    // Generate random salt
    let salt: String = rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(12)
        .map(char::from)
        .collect();

    // Create token: md5(password + salt)
    let mut hasher = Md5::new();
    hasher.update(format!("{}{}", config.password, salt));
    let token = format!("{:x}", hasher.finalize());

    format!(
        "u={}&t={}&s={}&v=1.16.1&c=stereodrome&f=json",
        urlencoding::encode(&config.username),
        token,
        salt
    )
}

/// Fetch audio bytes from the Subsonic server
pub async fn fetch_audio_bytes(config: &ServerConfig, song_id: &str) -> AppResult<Vec<u8>> {
    let url = build_stream_url(config, song_id);

    let client = reqwest::Client::new();
    let response = client.get(&url).send().await.map_err(AppError::Network)?;

    if !response.status().is_success() {
        return Err(AppError::Audio(format!(
            "Failed to fetch audio: HTTP {}",
            response.status()
        )));
    }

    let bytes = response.bytes().await.map_err(AppError::Network)?;

    Ok(bytes.to_vec())
}
