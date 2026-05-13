use rand::{RngExt as _, distr::Alphanumeric};
use urlencoding::encode;

use crate::ServerConfig;

const API_VERSION: &str = "1.16.1";
const CLIENT_NAME: &str = "StereodromeMobile";

pub fn signed_url(config: &ServerConfig, endpoint: &str, params: &[(&str, &str)]) -> String {
    let salt = rand::rng()
        .sample_iter(Alphanumeric)
        .take(12)
        .map(char::from)
        .collect::<String>();
    let token = format!("{:x}", md5::compute(format!("{}{}", config.password, salt)));
    let base = config.url.trim_end_matches('/');
    let mut query = vec![
        ("u", config.username.as_str()),
        ("t", token.as_str()),
        ("s", salt.as_str()),
        ("v", API_VERSION),
        ("c", CLIENT_NAME),
    ];
    query.extend_from_slice(params);

    let query = query
        .into_iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&");

    format!("{base}/rest/{endpoint}.view?{query}")
}
