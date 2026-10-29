mod audius;
pub mod download;
pub mod local;
mod soundcloud;
mod youtube;

use crate::model::{SourceKind, Track};
use std::sync::OnceLock;
use std::time::Duration;

pub(crate) fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36")
        .build().expect("HTTP client"))
}

static COOKIES_BROWSER: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

pub fn set_cookies_browser(browser: &str) {
    if let Ok(mut value) = COOKIES_BROWSER.write() {
        *value = browser.trim().to_lowercase();
    }
}

pub fn cookies_browser() -> Option<String> {
    COOKIES_BROWSER
        .read()
        .ok()
        .map(|v| v.clone())
        .filter(|v| !v.is_empty())
}

#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) fn ytdlp() -> tokio::process::Command {
    let mut command = tokio::process::Command::new("yt-dlp");
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    if let Some(browser) = cookies_browser() {
        command.args(["--cookies-from-browser", &browser]);
    }
    if let Some(runtime) = js_runtime() {
        command.args(["--js-runtimes", runtime]);
    }
    command
}

/// yt-dlp needs a JavaScript runtime to solve YouTube's challenges. It looks for
/// Deno by default; this also enables Node or Bun when one of those is installed.
/// mpv's ytdl hook options, matching what ytdlp() passes on the command line.
pub fn ytdl_raw_options() -> String {
    let mut options = Vec::new();
    if let Some(browser) = cookies_browser() {
        options.push(format!("cookies-from-browser={browser}"));
    }
    if let Some(runtime) = js_runtime() {
        options.push(format!("js-runtimes={runtime}"));
    }
    options.join(",")
}

pub fn js_runtime() -> Option<&'static str> {
    static RUNTIME: OnceLock<Option<&'static str>> = OnceLock::new();
    *RUNTIME.get_or_init(|| {
        ["deno", "node", "bun"]
            .into_iter()
            .find(|name| on_path(name))
    })
}

/// Updates yt-dlp in place when it is the standalone build (bundled or downloaded).
/// Returns the version line yt-dlp prints, or an error that says how to update it
/// when a package manager owns it.
pub async fn update_ytdlp() -> anyhow::Result<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(120),
        ytdlp_plain().args(["--update-to", "stable"]).output(),
    )
    .await??;
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    if text.contains("package manager") || text.contains("pip") {
        anyhow::bail!("yt-dlp is installed by your system, update it with your package manager");
    }
    anyhow::ensure!(
        output.status.success(),
        "{}",
        text.lines().last().unwrap_or("yt-dlp update failed")
    );
    Ok(text
        .lines()
        .rev()
        .find(|l| l.contains("Updated yt-dlp") || l.contains("up to date"))
        .unwrap_or("yt-dlp updated")
        .trim()
        .to_string())
}

fn ytdlp_plain() -> tokio::process::Command {
    let mut command = tokio::process::Command::new("yt-dlp");
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command.kill_on_drop(true);
    command
}

pub async fn search(kind: SourceKind, query: &str, limit: usize) -> anyhow::Result<Vec<Track>> {
    if limit == 0 || query.trim().is_empty() {
        return Ok(Vec::new());
    }
    match kind {
        SourceKind::YouTube => youtube::search(query, limit).await,
        SourceKind::SoundCloud => soundcloud::search(query, limit).await,
        SourceKind::Audius => audius::search(query, limit).await,
        SourceKind::Local => Ok(Vec::new()),
    }
}

pub async fn radio(seed: &Track, limit: usize) -> anyhow::Result<Vec<Track>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if seed.source == SourceKind::YouTube {
        youtube::radio(seed, limit).await
    } else {
        let results = search(seed.source, &seed.artist, limit + 1).await?;
        Ok(results
            .into_iter()
            .filter(|track| track.id != seed.id)
            .take(limit)
            .collect())
    }
}

pub async fn match_track(
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
) -> anyhow::Result<Option<Track>> {
    let candidates = search(SourceKind::YouTube, &format!("{artist} {title}"), 20).await?;
    fn words(s: &str) -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect()
    }
    fn similarity(a: &str, b: &str) -> f64 {
        let a = words(a);
        let b = words(b);
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }
        2.0 * a.intersection(&b).count() as f64 / (a.len() + b.len()) as f64
    }
    Ok(candidates
        .into_iter()
        .filter_map(|track| {
            let title_score = similarity(title, &track.title);
            let artist_score = similarity(artist, &track.artist);
            if title_score < 0.4 || artist_score < 0.3 {
                return None;
            }
            let duration_score = match (duration_secs, track.duration_secs) {
                (Some(expected), Some(actual)) if (expected - actual).abs() <= 5.0 => 0.35,
                (Some(expected), Some(actual)) if (expected - actual).abs() > 30.0 => -0.35,
                _ => 0.0,
            };
            let score = title_score * 0.65 + artist_score * 0.35 + duration_score;
            Some((score, track))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, track)| track))
}

pub fn missing_tools() -> Vec<String> {
    ["mpv", "yt-dlp"]
        .into_iter()
        .filter(|name| !on_path(name))
        .map(str::to_owned)
        .collect()
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            #[cfg(windows)]
            {
                [".exe", ".cmd", ".bat", ""]
                    .iter()
                    .any(|suffix| dir.join(format!("{name}{suffix}")).is_file())
            }
            #[cfg(not(windows))]
            {
                dir.join(name).is_file()
            }
        })
    })
}
