pub mod autoeq;
pub mod lyrics;
pub mod scrobble;
pub mod spotify;

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("Tonal/0.1")
            .build()
            .expect("HTTP client initialization failed")
    })
}

pub(super) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(super) fn form(fields: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().copied())
        .finish()
}
