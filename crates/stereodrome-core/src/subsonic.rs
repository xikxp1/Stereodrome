use rand::{RngExt as _, distr::Alphanumeric};
use urlencoding::encode;

use crate::{API_VERSION, CLIENT_NAME, ServerConfig};

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

#[cfg(test)]
mod tests {
    use super::{CLIENT_NAME, signed_url};
    use crate::ServerConfig;

    #[test]
    fn signed_url_carries_the_shared_client_identity() {
        let config = ServerConfig {
            url: "https://music.example.com/".to_string(),
            username: "listener".to_string(),
            password: "hunter2".to_string(),
        };

        let url = signed_url(&config, "stream", &[("id", "song 1")]);

        assert!(url.starts_with("https://music.example.com/rest/stream.view?"));
        assert!(url.contains(&format!("c={CLIENT_NAME}")));
        assert!(url.contains("v=1.16.1"));
        assert!(url.contains("u=listener"));
        assert!(url.contains("id=song%201"), "query values are encoded");
        // The password is only ever sent as a salted token.
        assert!(!url.contains("hunter2"));
    }

    #[test]
    fn client_name_distinguishes_desktop_from_mobile() {
        if cfg!(any(target_os = "android", target_os = "ios")) {
            assert_eq!(CLIENT_NAME, "StereodromeMobile");
        } else {
            assert_eq!(CLIENT_NAME, "StereodromeDesktop");
        }
    }
}
