use crate::model::{Settings, Track};
use anyhow::Context;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const QUEUE_LIMIT: usize = 500;
const QUEUE_MAX_AGE: i64 = 14 * 24 * 3600;
const QUEUE_ATTEMPTS: u32 = 20;
const FLUSH_BATCH: usize = 50;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Pending {
    track: Track,
    started: i64,
    lastfm: bool,
    listenbrainz: bool,
    #[serde(default)]
    attempts: u32,
}

impl Pending {
    fn same(&self, other: &Pending) -> bool {
        self.track.id == other.track.id && self.started == other.started
    }

    /// Records a retry; each service is `Some(failed)` when it was retried, `None` otherwise.
    fn retried(&mut self, lastfm: Option<bool>, listenbrainz: Option<bool>) {
        if lastfm == Some(true) || listenbrainz == Some(true) {
            self.attempts += 1;
        }
        self.lastfm = lastfm.unwrap_or(self.lastfm);
        self.listenbrainz = listenbrainz.unwrap_or(self.listenbrainz);
    }
}

fn queue_path() -> PathBuf {
    crate::model::paths::data_dir().join("scrobble-queue.json")
}

fn queue_lock() -> &'static parking_lot::Mutex<()> {
    static LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    &LOCK
}

fn load_queue(path: &Path) -> Vec<Pending> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_queue(path: &Path, queue: &[Pending]) -> anyhow::Result<()> {
    if queue.is_empty() {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => return Ok(()),
        }
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(queue)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn prune(queue: &mut Vec<Pending>, now: i64) {
    queue.retain(|p| {
        (p.lastfm || p.listenbrainz)
            && p.attempts < QUEUE_ATTEMPTS
            && p.started >= now - QUEUE_MAX_AGE
    });
    if queue.len() > QUEUE_LIMIT {
        let excess = queue.len() - QUEUE_LIMIT;
        queue.drain(..excess);
    }
}

fn merge(queue: &mut Vec<Pending>, entry: Pending) {
    if let Some(existing) = queue.iter_mut().find(|p| p.same(&entry)) {
        existing.lastfm |= entry.lastfm;
        existing.listenbrainz |= entry.listenbrainz;
    } else {
        queue.push(entry);
    }
}

fn enqueue(path: &Path, entry: Pending, now: i64) -> anyhow::Result<()> {
    let _guard = queue_lock().lock();
    let mut queue = load_queue(path);
    merge(&mut queue, entry);
    prune(&mut queue, now);
    save_queue(path, &queue)
}

fn settle(path: &Path, processed: Vec<Pending>, now: i64) -> anyhow::Result<()> {
    let _guard = queue_lock().lock();
    let mut queue = load_queue(path);
    queue.retain(|p| !processed.iter().any(|done| done.same(p)));
    for entry in processed {
        if entry.lastfm || entry.listenbrainz {
            merge(&mut queue, entry);
        }
    }
    prune(&mut queue, now);
    save_queue(path, &queue)
}

fn pending_snapshot(path: &Path) -> Vec<Pending> {
    let _guard = queue_lock().lock();
    load_queue(path)
}

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
        if lastfm.is_err() || listenbrainz.is_err() {
            let entry = Pending {
                track: track.clone(),
                started: started_at_unix,
                lastfm: lastfm.is_err(),
                listenbrainz: listenbrainz.is_err(),
                attempts: 0,
            };
            if let Err(e) = enqueue(&queue_path(), entry, super::now()) {
                log::warn!("Could not queue scrobble for retry: {e:#}");
            }
        }
        self.flush(lastfm.is_ok(), listenbrainz.is_ok()).await;
        lastfm.context("Scrobble queued for retry")?;
        listenbrainz.context("Scrobble queued for retry")
    }

    async fn flush(&self, lastfm: bool, listenbrainz: bool) {
        static FLUSHING: AtomicBool = AtomicBool::new(false);
        if !(lastfm || listenbrainz) || FLUSHING.swap(true, Ordering::AcqRel) {
            return;
        }
        let path = queue_path();
        let pending = pending_snapshot(&path);
        let mut processed = Vec::new();
        for mut entry in pending
            .into_iter()
            .filter(|p| (p.lastfm && lastfm) || (p.listenbrainz && listenbrainz))
            .take(FLUSH_BATCH)
        {
            let lastfm_failed = if entry.lastfm && lastfm {
                Some(
                    self.lastfm(&entry.track, Some(entry.started))
                        .await
                        .is_err(),
                )
            } else {
                None
            };
            let listenbrainz_failed = if entry.listenbrainz && listenbrainz {
                Some(
                    self.listenbrainz(&entry.track, Some(entry.started))
                        .await
                        .is_err(),
                )
            } else {
                None
            };
            entry.retried(lastfm_failed, listenbrainz_failed);
            processed.push(entry);
        }
        if !processed.is_empty()
            && let Err(e) = settle(&path, processed, super::now())
        {
            log::warn!("Could not update scrobble retry queue: {e:#}");
        }
        FLUSHING.store(false, Ordering::Release);
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
    fn pending(id: &str, started: i64) -> Pending {
        Pending {
            track: Track {
                id: id.into(),
                ..Default::default()
            },
            started,
            lastfm: true,
            listenbrainz: false,
            attempts: 0,
        }
    }
    #[test]
    fn retry_queue_persists_merges_and_settles() {
        let dir = std::env::temp_dir().join(format!("tonal-scrobble-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");
        let now = 1_000_000_000;
        enqueue(&path, pending("a", now - 10), now).unwrap();
        enqueue(
            &path,
            Pending {
                lastfm: false,
                listenbrainz: true,
                ..pending("a", now - 10)
            },
            now,
        )
        .unwrap();
        enqueue(&path, pending("b", now - 5), now).unwrap();
        enqueue(&path, pending("old", now - QUEUE_MAX_AGE - 1), now).unwrap();
        let queue = load_queue(&path);
        assert_eq!(queue.len(), 2);
        assert!(queue[0].lastfm && queue[0].listenbrainz);
        let done = Pending {
            lastfm: false,
            listenbrainz: false,
            ..queue[1].clone()
        };
        let retry = Pending {
            lastfm: false,
            attempts: 1,
            ..queue[0].clone()
        };
        settle(&path, vec![retry, done], now).unwrap();
        let queue = load_queue(&path);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].track.id, "a");
        assert!(!queue[0].lastfm && queue[0].listenbrainz && queue[0].attempts == 1);
        let last = Pending {
            listenbrainz: false,
            ..queue[0].clone()
        };
        settle(&path, vec![last], now).unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn retry_queue_is_capped_oldest_first() {
        let now = 1_000_000_000;
        let mut queue: Vec<_> = (0..QUEUE_LIMIT + 3)
            .map(|i| pending(&i.to_string(), now - 100 + i as i64))
            .collect();
        queue.push(Pending {
            attempts: QUEUE_ATTEMPTS,
            ..pending("tired", now)
        });
        prune(&mut queue, now);
        assert_eq!(queue.len(), QUEUE_LIMIT);
        assert_eq!(queue[0].track.id, "3");
    }
    #[test]
    fn retry_counts_only_services_that_were_retried() {
        let both = Pending {
            listenbrainz: true,
            ..pending("a", 0)
        };
        let mut entry = both.clone();
        entry.retried(Some(false), None);
        assert!(!entry.lastfm && entry.listenbrainz);
        assert_eq!(entry.attempts, 0);
        entry.retried(None, Some(true));
        assert!(entry.listenbrainz);
        assert_eq!(entry.attempts, 1);
        let mut entry = both;
        entry.retried(Some(true), Some(false));
        assert!(entry.lastfm && !entry.listenbrainz);
        assert_eq!(entry.attempts, 1);
    }
}
