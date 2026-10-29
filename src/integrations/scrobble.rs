use crate::model::{Settings, Track};
use anyhow::Context;
use md5::{Digest, Md5};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Clone)]
pub struct Scrobbler {
    settings: Settings,
}

fn signature(fields: &BTreeMap<String, String>, secret: &str) -> String {
    let mut bytes = String::new();
    for (key, value) in fields {
        if !matches!(key.as_str(), "format" | "callback" | "api_sig") {
            bytes.push_str(key);
            bytes.push_str(value);
        }
    }
    bytes.push_str(secret);
    Md5::digest(bytes.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn lastfm_request(
    mut fields: BTreeMap<String, String>,
    secret: &str,
) -> anyhow::Result<Value> {
    fields.insert("api_sig".into(), signature(&fields, secret));
    fields.insert("format".into(), "json".into());
    let auth = fields.get("method").is_some_and(|s| s.starts_with("auth."));
    let response = if auth {
        let mut url = url::Url::parse("https://ws.audioscrobbler.com/2.0/")?;
        url.query_pairs_mut().extend_pairs(fields.iter());
        super::client().get(url).send().await?
    } else {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields.iter())
            .finish();
        super::client()
            .post("https://ws.audioscrobbler.com/2.0/")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body)
            .send()
            .await?
    };
    let status = response.status();
    let value: Value = response.json().await?;
    // Last.fm can return auth error 14 with HTTP 403 while browser approval is pending.
    anyhow::ensure!(
        status.is_success() || value["error"].is_number(),
        "Last.fm HTTP {status}"
    );
    Ok(value)
}

fn lastfm_ok(value: &Value) -> anyhow::Result<()> {
    if let Some(error) = value["error"].as_i64() {
        anyhow::bail!(
            "Last.fm error {error}: {}",
            value["message"].as_str().unwrap_or("Unknown error")
        );
    }
    if let Some(ignored) = value
        .pointer("/scrobbles/@attr/ignored")
        .and_then(Value::as_str)
    {
        anyhow::ensure!(
            ignored == "0",
            "Last.fm ignored this scrobble: {}",
            value["scrobbles"]
        );
    }
    Ok(())
}

impl Scrobbler {
    pub fn new(settings: &Settings) -> Self {
        Self {
            settings: settings.clone(),
        }
    }

    pub fn update_settings(&mut self, settings: &Settings) {
        self.settings = settings.clone();
    }

    async fn lastfm(&self, track: &Track, started: Option<i64>) -> anyhow::Result<()> {
        let settings = &self.settings;
        let Some(session) = settings
            .lastfm_session
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        else {
            return Ok(());
        };
        if settings.lastfm_api_key.trim().is_empty() || settings.lastfm_api_secret.trim().is_empty()
        {
            return Ok(());
        }
        let mut fields = BTreeMap::from([
            (
                "method".into(),
                if started.is_some() {
                    "track.scrobble"
                } else {
                    "track.updateNowPlaying"
                }
                .into(),
            ),
            ("api_key".into(), settings.lastfm_api_key.clone()),
            ("sk".into(), session.to_string()),
            ("artist".into(), track.artist.clone()),
            ("track".into(), track.title.clone()),
        ]);
        if let Some(album) = &track.album {
            fields.insert("album".into(), album.clone());
        }
        if let Some(duration) = track.duration_secs.filter(|d| d.is_finite() && *d > 0.0) {
            fields.insert("duration".into(), (duration.round() as i64).to_string());
        }
        if let Some(started) = started {
            fields.insert("timestamp".into(), started.to_string());
        }
        lastfm_ok(&lastfm_request(fields, &settings.lastfm_api_secret).await?)
    }

    async fn listenbrainz(&self, track: &Track, started: Option<i64>) -> anyhow::Result<()> {
        let token = self.settings.listenbrainz_token.trim();
        if token.is_empty() {
            return Ok(());
        }
        let mut metadata = json!({"artist_name":track.artist, "track_name":track.title,
            "additional_info":{"submission_client":"Tonal", "submission_client_version":"0.1"}});
        if let Some(album) = &track.album {
            metadata["release_name"] = json!(album);
        }
        if let Some(duration) = track.duration_secs.filter(|d| d.is_finite() && *d > 0.0) {
            metadata["additional_info"]["duration_ms"] = json!((duration * 1000.0).round() as u64);
        }
        let mut listen = json!({"track_metadata":metadata});
        if let Some(started) = started {
            listen["listened_at"] = json!(started);
        }
        let body = json!({"listen_type":if started.is_some() { "single" } else { "playing_now" }, "payload":[listen]});
        let value: Value = super::client()
            .post("https://api.listenbrainz.org/1/submit-listens")
            .header(reqwest::header::AUTHORIZATION, format!("Token {token}"))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        anyhow::ensure!(
            value["status"].as_str() == Some("ok"),
            "ListenBrainz rejected listen: {value}"
        );
        Ok(())
    }

    pub async fn now_playing(&self, track: &Track) -> anyhow::Result<()> {
        if !self.settings.scrobble_enabled {
            return Ok(());
        }
        let (lastfm, listenbrainz) =
            tokio::join!(self.lastfm(track, None), self.listenbrainz(track, None));
        lastfm?;
        listenbrainz
    }

    pub async fn scrobble(&self, track: &Track, started_at_unix: i64) -> anyhow::Result<()> {
        if !self.settings.scrobble_enabled {
            return Ok(());
        }
        let (lastfm, listenbrainz) = tokio::join!(
            self.lastfm(track, Some(started_at_unix)),
            self.listenbrainz(track, Some(started_at_unix))
        );
        lastfm?;
        listenbrainz
    }
}

pub fn should_scrobble(played_secs: f64, duration_secs: f64) -> bool {
    played_secs.is_finite()
        && duration_secs.is_finite()
        && duration_secs > 30.0
        && (played_secs >= duration_secs / 2.0 || played_secs >= 240.0)
}

pub async fn lastfm_login(api_key: &str, api_secret: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !api_key.trim().is_empty() && !api_secret.trim().is_empty(),
        "Last.fm API key and secret are required"
    );
    let fields = BTreeMap::from([
        ("method".into(), "auth.getToken".into()),
        ("api_key".into(), api_key.to_string()),
    ]);
    let value = lastfm_request(fields, api_secret).await?;
    lastfm_ok(&value)?;
    let token = value["token"]
        .as_str()
        .context("Last.fm returned no auth token")?
        .to_string();
    let mut url = url::Url::parse("https://www.last.fm/api/auth")?;
    url.query_pairs_mut()
        .append_pair("api_key", api_key)
        .append_pair("token", &token);
    tokio::task::spawn_blocking(move || open::that(url.as_str())).await??;
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let fields = BTreeMap::from([
                ("method".into(), "auth.getSession".into()),
                ("api_key".into(), api_key.to_string()),
                ("token".into(), token.clone()),
            ]);
            let value = lastfm_request(fields, api_secret).await?;
            if value["error"].as_i64() == Some(14) {
                continue;
            }
            lastfm_ok(&value)?;
            return value
                .pointer("/session/key")
                .and_then(Value::as_str)
                .map(str::to_string)
                .context("Last.fm returned no session key");
        }
    })
    .await
    .context("Last.fm login timed out after two minutes")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signs_sorted_unencoded_parameters() {
        let fields = BTreeMap::from([
            ("b".into(), "two words".into()),
            ("a".into(), "1".into()),
            ("format".into(), "json".into()),
        ]);
        let expected: String = Md5::digest(b"a1btwo wordssecret")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(signature(&fields, "secret"), expected);
    }
    #[test]
    fn scrobble_thresholds() {
        assert!(!should_scrobble(30.0, 30.0));
        assert!(!should_scrobble(59.0, 120.0));
        assert!(should_scrobble(60.0, 120.0));
        assert!(should_scrobble(240.0, 1000.0));
        assert!(!should_scrobble(f64::NAN, 100.0));
    }
}
