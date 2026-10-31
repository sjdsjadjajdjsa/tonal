//! Caches direct stream URLs so playback can start without repeating yt-dlp.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(4 * 60 * 60);
const CAPACITY: usize = 200;

struct Entry {
    direct: String,
    remembered: Instant,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<String, Entry>,
}

impl Cache {
    fn lookup_at(&mut self, url: &str, now: Instant) -> Option<String> {
        if self.entries.get(url).is_some_and(|entry| now.duration_since(entry.remembered) >= TTL) {
            self.entries.remove(url);
        }
        self.entries.get(url).map(|entry| entry.direct.clone())
    }

    fn remember_at(&mut self, url: &str, direct: &str, now: Instant) {
        self.entries.retain(|_, entry| now.duration_since(entry.remembered) < TTL);
        self.entries.insert(url.to_string(), Entry {
            direct: direct.to_string(),
            remembered: now,
        });
        while self.entries.len() > CAPACITY {
            let oldest = self.entries.iter()
                .min_by_key(|(_, entry)| entry.remembered)
                .map(|(url, _)| url.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
    }
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// Returns the cached direct stream URL only while its four-hour TTL is fresh.
pub fn lookup(url: &str) -> Option<String> {
    cache().lock().unwrap_or_else(|e| e.into_inner()).lookup_at(url, Instant::now())
}

/// Caches a direct stream URL for four hours, keeping at most 200 entries.
pub fn remember(url: &str, direct: &str) {
    cache().lock().unwrap_or_else(|e| e.into_inner()).remember_at(url, direct, Instant::now());
}

/// Drops the cached direct stream URL for a source URL.
pub fn forget(url: &str) {
    cache().lock().unwrap_or_else(|e| e.into_inner()).entries.remove(url);
}

/// Resolves and caches the last direct stream URL returned by yt-dlp within 60 seconds.
pub async fn resolve(url: &str) -> Result<String, String> {
    let mut command = super::ytdlp();
    command.args(["-f", "bestaudio/best", "-g", "--no-playlist", "--no-warnings", url])
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(60), command.output())
        .await
        .map_err(|_| "yt-dlp timed out".to_string())?
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(direct) = stdout.lines().rev().map(str::trim).find(|line| !line.is_empty()) {
            remember(url, direct);
            return Ok(direct.to_string());
        }
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(stderr.lines()
        .find_map(|line| line.strip_prefix("ERROR: "))
        .or_else(|| stderr.lines().last())
        .unwrap_or("yt-dlp returned no stream URL")
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_hits_after_remember() {
        remember("resolve-test-hit", "https://stream/hit");
        assert_eq!(lookup("resolve-test-hit").as_deref(), Some("https://stream/hit"));
        forget("resolve-test-hit");
    }

    #[test]
    fn forget_makes_lookup_miss() {
        remember("resolve-test-forget", "https://stream/forget");
        forget("resolve-test-forget");
        assert_eq!(lookup("resolve-test-forget"), None);
    }

    #[test]
    fn expired_entry_misses() {
        let mut cache = Cache::default();
        let now = Instant::now();
        cache.remember_at("source", "direct", now);
        assert_eq!(cache.lookup_at("source", now + TTL - Duration::from_nanos(1)).as_deref(), Some("direct"));
        assert_eq!(cache.lookup_at("source", now + TTL), None);
    }

    #[test]
    fn capacity_evicts_oldest() {
        let mut cache = Cache::default();
        let now = Instant::now();
        for index in 0..=CAPACITY {
            cache.remember_at(&index.to_string(), "direct", now + Duration::from_secs(index as u64));
        }
        assert_eq!(cache.entries.len(), CAPACITY);
        assert!(!cache.entries.contains_key("0"));
        assert!(cache.entries.contains_key("1"));
        assert!(cache.entries.contains_key(&CAPACITY.to_string()));
    }

    #[test]
    fn expired_entries_are_evicted_before_oldest() {
        let mut cache = Cache::default();
        let now = Instant::now();
        cache.remember_at("expired", "direct", now);
        cache.remember_at("fresh", "direct", now + TTL - Duration::from_secs(1));
        cache.remember_at("new", "direct", now + TTL);
        assert_eq!(cache.entries.len(), 2);
        assert!(!cache.entries.contains_key("expired"));
        assert!(cache.entries.contains_key("fresh"));
    }
}
