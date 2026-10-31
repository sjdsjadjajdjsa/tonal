mod audius;
pub mod download;
pub mod local;
mod pot;
pub(crate) mod resolve;
mod soundcloud;
mod youtube;
mod youtube_browse;

use crate::model::{AlbumPage, AlbumRef, ArtistPage, ArtistRef, SourceKind, Track};
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

/// yt-dlp with browser cookies, a JavaScript runtime and the YouTube access
/// strategy that last worked (PO token provider, player clients, IPv4).
pub(crate) fn ytdlp() -> tokio::process::Command {
    let mut command = tokio::process::Command::new("yt-dlp");
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    pot::prepare();
    command.args(pot::args(pot::current()));
    if let Some(runtime) = js_runtime() {
        command.args(["--js-runtimes", runtime]);
    }
    command
}

/// mpv's ytdl hook options, matching what ytdlp() passes on the command line.
/// The access strategy goes through a yt-dlp config file that Tonal rewrites
/// when it changes, so running mpv instances follow without a restart.
pub fn ytdl_raw_options() -> String {
    pot::prepare();
    let mut options = Vec::new();
    match pot::write_config() {
        Some(path) => options.push(format!("config-locations={}", path.display())),
        None => {
            if let Some(browser) = cookies_browser() {
                options.push(format!("cookies-from-browser={browser}"));
            }
        }
    }
    if let Some(runtime) = js_runtime() {
        options.push(format!("js-runtimes={runtime}"));
    }
    options.join(",")
}

/// Tries each YouTube access layer until yt-dlp resolves `url`, remembering the
/// one that worked for every later yt-dlp and mpv load. Returns a description
/// of the working layer and the stream URL it resolved, or yt-dlp's last error.
pub async fn unblock_youtube(url: &str) -> Result<(String, Option<String>), String> {
    pot::unblock(url).await
}

/// yt-dlp needs a JavaScript runtime to solve YouTube's challenges. It looks for
/// Deno by default; this also enables Node or Bun when one of those is installed.
pub fn js_runtime() -> Option<&'static str> {
    ["deno", "node", "bun"]
        .into_iter()
        .find(|name| on_path(name))
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

/// Fetch a source artist, resolving legacy name-only references first.
pub async fn artist(r: &ArtistRef) -> anyhow::Result<ArtistPage> {
    anyhow::ensure!(
        r.source != SourceKind::Local,
        "local artists are built by the app"
    );
    let resolved;
    let r = if r.id.as_deref().is_none_or(|id| id.trim().is_empty()) {
        resolved = find_artist(r.source, &r.name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("{} artist not found: {}", r.source.label(), r.name))?;
        &resolved
    } else {
        r
    };
    match r.source {
        SourceKind::YouTube => youtube_browse::artist(r).await,
        SourceKind::SoundCloud => soundcloud::artist(r).await,
        SourceKind::Audius => audius::artist(r).await,
        SourceKind::Local => anyhow::bail!("local artists are built by the app"),
    }
}

/// YouTube Music album metadata and ordered playable tracks.
pub async fn album(r: &AlbumRef) -> anyhow::Result<AlbumPage> {
    anyhow::ensure!(
        r.source == SourceKind::YouTube,
        "{} album browsing is not supported; only YouTube Music albums are available",
        r.source.label()
    );
    let resolved;
    let r = if r.id.as_deref().is_none_or(|id| id.trim().is_empty()) {
        resolved = find_album(r.source, &r.title, &r.artist)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("YouTube album not found: {} — {}", r.artist, r.title)
            })?;
        &resolved
    } else {
        r
    };
    youtube_browse::album(r).await
}

pub async fn find_artist(source: SourceKind, name: &str) -> anyhow::Result<Option<ArtistRef>> {
    if source == SourceKind::Local {
        anyhow::bail!("local artists are built by the app");
    }
    if name.trim().is_empty() {
        return Ok(None);
    }
    match source {
        SourceKind::YouTube => youtube_browse::find_artist(name).await,
        SourceKind::SoundCloud => soundcloud::find_artist(name).await,
        SourceKind::Audius => audius::find_artist(name).await,
        SourceKind::Local => anyhow::bail!("local artists are built by the app"),
    }
}

pub async fn find_album(
    source: SourceKind,
    title: &str,
    artist: &str,
) -> anyhow::Result<Option<AlbumRef>> {
    anyhow::ensure!(
        source == SourceKind::YouTube,
        "{} album lookup is not supported; only YouTube Music albums are available",
        source.label()
    );
    if title.trim().is_empty() {
        return Ok(None);
    }
    youtube_browse::find_album(title, artist).await
}

fn name_score(actual: &str, expected: &str) -> usize {
    fn normalized(name: &str) -> String {
        name.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }
    if normalized(actual) == normalized(expected) {
        return 1000;
    }
    let actual = actual.to_lowercase();
    let expected = expected.to_lowercase();
    let words: std::collections::HashSet<_> = expected.split_whitespace().collect();
    let overlap = actual
        .split_whitespace()
        .filter(|word| words.contains(word))
        .count();
    overlap * 100 / actual.split_whitespace().count().max(words.len()).max(1)
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

/// Best match for a song on `source` (duration within ~5s preferred). Used for
/// Spotify import (YouTube) and for playback fallback when YouTube blocks a song.
pub async fn match_track(
    source: SourceKind,
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
) -> anyhow::Result<Option<Track>> {
    Ok(match_tracks(source, title, artist, duration_secs)
        .await?
        .into_iter()
        .next())
}

/// Every acceptable match on `source`, best first.
pub async fn match_tracks(
    source: SourceKind,
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
) -> anyhow::Result<Vec<Track>> {
    let candidates = search(source, &format!("{artist} {title}"), 20).await?;
    Ok(ranked_matches(candidates, title, artist, duration_secs))
}

#[cfg(test)]
fn best_match(
    candidates: Vec<Track>,
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
) -> Option<Track> {
    ranked_matches(candidates, title, artist, duration_secs)
        .into_iter()
        .next()
}

fn ranked_matches(
    candidates: Vec<Track>,
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
) -> Vec<Track> {
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
    let artist_words = words(artist);
    let mut scored: Vec<_> = candidates
        .into_iter()
        .filter_map(|track| {
            let title_score = similarity(title, &track.title);
            // Uploads often name the artist in the title ("Artist - Song").
            let artist_score =
                if !artist_words.is_empty() && artist_words.is_subset(&words(&track.title)) {
                    1.0
                } else {
                    similarity(artist, &track.artist)
                };
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
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, track)| track).collect()
}

/// True when `url` is hosted on YouTube, judged by its parsed host, so a
/// "youtube.com/" inside another site's path or query does not count.
pub fn is_youtube_url(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| {
            matches!(
                host.as_str(),
                "youtube.com"
                    | "www.youtube.com"
                    | "music.youtube.com"
                    | "m.youtube.com"
                    | "youtu.be"
            )
        })
}

/// True for URLs mpv can only play after yt-dlp resolves them: YouTube and
/// SoundCloud. Audius and local files load directly.
pub fn is_streamed_url(url: &str) -> bool {
    is_youtube_url(url)
        || url::Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
            .is_some_and(|host| host == "soundcloud.com" || host == "www.soundcloud.com")
}

/// Debug hook: TONAL_DEBUG_FAKE_BOT=1 makes every YouTube load fail as if bot-blocked.
#[cfg(feature = "debug-hooks")]
fn fake_bot(url: &str) -> bool {
    is_youtube_url(url) && std::env::var_os("TONAL_DEBUG_FAKE_BOT").is_some_and(|v| v == "1")
}

/// The URL mpv loads for a track: a cached stream URL when one is fresh, so
/// plays start without yt-dlp. Only the debug hook changes it, to an
/// unloadable path so YouTube fails the way a bot block does.
pub fn mpv_url(url: &str) -> String {
    #[cfg(feature = "debug-hooks")]
    if fake_bot(url) {
        return "/nonexistent/tonal-debug-fake-bot-block".into();
    }
    resolve::lookup(url).unwrap_or_else(|| url.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_futures_are_send() {
        fn assert_send<T: Send>(_: T) {}
        let artist_ref = ArtistRef::default();
        let album_ref = AlbumRef::default();
        let track = Track::default();
        assert_send(artist(&artist_ref));
        assert_send(album(&album_ref));
        for source in [
            SourceKind::YouTube,
            SourceKind::SoundCloud,
            SourceKind::Audius,
            SourceKind::Local,
        ] {
            assert_send(find_artist(source, "Artist"));
            assert_send(find_album(source, "Album", "Artist"));
            assert_send(search(source, "Song", 5));
        }
        assert_send(radio(&track, 5));
        assert_send(match_track(SourceKind::SoundCloud, "Song", "Artist", None));
    }

    fn candidate(title: &str, artist: &str, duration: f64) -> Track {
        Track {
            id: format!("soundcloud:{title}"),
            title: title.into(),
            artist: artist.into(),
            duration_secs: Some(duration),
            ..Default::default()
        }
    }

    #[test]
    fn uploads_with_artist_in_title_match() {
        let found = best_match(
            vec![
                candidate("Something else", "Daft Punk", 320.0),
                candidate("Daft Punk - One More Time", "some uploader", 321.0),
            ],
            "One More Time",
            "Daft Punk",
            Some(320.0),
        );
        assert_eq!(found.unwrap().title, "Daft Punk - One More Time");
        let none = best_match(
            vec![candidate("Other", "Nobody", 10.0)],
            "One More Time",
            "Daft Punk",
            None,
        );
        assert!(none.is_none());
    }

}
