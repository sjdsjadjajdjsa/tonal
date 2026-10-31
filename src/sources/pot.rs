//! Keeps yt-dlp working when YouTube says "Sign in to confirm you're not a bot".
//!
//! Layers, all optional and fail-soft:
//! 1. A PO Token provider: the bgutil-ytdlp-pot-provider plugin in script mode,
//!    run by Deno. Tonal downloads it into its cache on first use; no server.
//! 2. Other player clients when the default set is refused.
//! 3. IPv4 only, since IPv6 ranges get flagged more often.
//! 4. With and without browser cookies (cookie extraction itself can fail).
//!
//! The strategy that last worked is saved and tried first. mpv's ytdl hook reads
//! the same options from a yt-dlp config file, which is rewritten whenever the
//! strategy changes, so a running mpv picks it up on its next load.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::process::Command;

const BGUTIL: &str = "2.0.2";
const REPO: &str = "https://github.com/Brainicism/bgutil-ytdlp-pot-provider";
/// Player client sets to try, `None` being yt-dlp's default. mweb is what the
/// yt-dlp wiki recommends with a PO Token; tv_simply also takes one, and
/// android_vr needs none (but may only offer a muxed 360p stream).
const CLIENTS: [Option<&str>; 4] = [None, Some("mweb"), Some("tv_simply"), Some("android_vr")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strategy {
    clients: usize,
    ipv4: bool,
    cookies: bool,
}

impl Default for Strategy {
    fn default() -> Self {
        Self {
            clients: 0,
            ipv4: false,
            cookies: true,
        }
    }
}

impl Strategy {
    fn encode(self) -> String {
        format!(
            "clients={} ipv4={} cookies={}",
            self.clients, self.ipv4 as u8, self.cookies as u8
        )
    }

    fn decode(text: &str) -> Option<Self> {
        let mut s = Self::default();
        for pair in text.split_whitespace() {
            let (key, value) = pair.split_once('=')?;
            match key {
                "clients" => s.clients = value.parse().ok().filter(|i| *i < CLIENTS.len())?,
                "ipv4" => s.ipv4 = value == "1",
                "cookies" => s.cookies = value == "1",
                _ => {}
            }
        }
        Some(s)
    }

    fn describe(self) -> String {
        let mut parts = vec![format!(
            "clients {}",
            CLIENTS[self.clients].unwrap_or("default")
        )];
        if pot_ready() {
            parts.push("PO token".into());
        }
        if self.ipv4 {
            parts.push("IPv4".into());
        }
        if self.cookies && super::cookies_browser().is_some() {
            parts.push("browser cookies".into());
        }
        parts.join(", ")
    }
}

fn root() -> PathBuf {
    crate::model::paths::cache_dir().join("yt-dlp")
}

fn pot_dir() -> PathBuf {
    root().join(format!("bgutil-{BGUTIL}"))
}

fn server_home() -> PathBuf {
    pot_dir()
        .join(format!("bgutil-ytdlp-pot-provider-{BGUTIL}"))
        .join("server")
}

fn pot_ready() -> bool {
    pot_dir().join("ready").is_file()
}

static CURRENT: Mutex<Option<Strategy>> = Mutex::new(None);

pub fn current() -> Strategy {
    let mut current = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    *current.get_or_insert_with(|| {
        std::fs::read_to_string(root().join("strategy"))
            .ok()
            .and_then(|text| Strategy::decode(&text))
            .unwrap_or_default()
    })
}

fn remember(strategy: Strategy) {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(strategy);
    let _ = std::fs::create_dir_all(root());
    let _ = std::fs::write(root().join("strategy"), strategy.encode());
    write_config();
}

/// yt-dlp arguments for a strategy, cookies and PO token provider included.
pub fn args(strategy: Strategy) -> Vec<String> {
    let mut args = Vec::new();
    if strategy.cookies
        && let Some(browser) = super::cookies_browser()
    {
        args.extend(["--cookies-from-browser".into(), browser]);
    }
    if pot_ready() {
        args.extend([
            "--plugin-dirs".into(),
            pot_dir().join("plugins").display().to_string(),
            "--extractor-args".into(),
            format!(
                "youtubepot-bgutilscript:server_home={}",
                server_home().display()
            ),
        ]);
    }
    if let Some(clients) = CLIENTS[strategy.clients] {
        args.extend([
            "--extractor-args".into(),
            format!("youtube:player_client={clients}"),
        ]);
    }
    if strategy.ipv4 {
        args.push("--force-ipv4".into());
    }
    args
}

/// Writes the current strategy as a yt-dlp config file for mpv's ytdl hook.
pub fn write_config() -> Option<PathBuf> {
    let mut text = String::from("# Written by Tonal, rewritten when YouTube access changes\n");
    let mut args = args(current()).into_iter().peekable();
    while let Some(arg) = args.next() {
        text.push_str(&arg);
        if let Some(value) = args.next_if(|next| !next.starts_with("--")) {
            text.push(' ');
            text.push_str(&quote(&value));
        }
        text.push('\n');
    }
    let path = root().join("tonal.conf");
    std::fs::create_dir_all(root()).ok()?;
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        let tmp = path.with_extension("conf.tmp");
        std::fs::write(&tmp, &text).ok()?;
        if std::fs::rename(&tmp, &path).is_err() {
            std::fs::write(&path, &text).ok()?;
        }
    }
    Some(path)
}

/// Single-quotes for yt-dlp's shell-like config parser (backslashes stay literal).
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

static SETUP_DONE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static SETTING_UP: AtomicBool = AtomicBool::new(false);

/// Starts the one-time PO token provider setup in the background.
pub fn prepare() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if pot_ready() || STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        STARTED.store(false, Ordering::SeqCst);
        return;
    };
    SETTING_UP.store(true, Ordering::SeqCst);
    runtime.spawn(async {
        let started = Instant::now();
        match setup().await {
            Ok(()) => {
                log::info!(
                    "YouTube PO token provider ready in {:.0}s",
                    started.elapsed().as_secs_f64()
                );
                write_config();
            }
            Err(e) => log::warn!("YouTube PO token provider unavailable: {e:#}"),
        }
        SETTING_UP.store(false, Ordering::SeqCst);
        SETUP_DONE.notify_waiters();
    });
}

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(super::CREATE_NO_WINDOW);
    command
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .env("DENO_NO_PROMPT", "1")
        .env("DENO_NO_UPDATE_CHECK", "1")
        .env("NO_COLOR", "1");
    command
}

async fn run(mut command: Command, secs: u64) -> anyhow::Result<String> {
    let output = tokio::time::timeout(Duration::from_secs(secs), command.output()).await??;
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    anyhow::ensure!(
        output.status.success(),
        "{}",
        text.lines()
            .find(|l| l.to_lowercase().contains("error"))
            .or_else(|| text.lines().last())
            .unwrap_or("failed")
    );
    Ok(text)
}

/// Deno 2.4.3+ runs the provider's TypeScript script directly. Node would need a
/// TypeScript build step, so the provider is skipped without Deno.
async fn deno_supported() -> bool {
    let mut version = command("deno");
    version.arg("--version");
    let Ok(text) = run(version, 10).await else {
        return false;
    };
    let version: Vec<u32> = text
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .split('.')
        .map(|n| n.parse().unwrap_or(0))
        .collect();
    version >= vec![2, 4, 3]
}

async fn fetch(url: &str) -> anyhow::Result<Vec<u8>> {
    let response = super::client()
        .get(url)
        .timeout(Duration::from_secs(120))
        .send()
        .await?
        .error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

async fn setup() -> anyhow::Result<()> {
    anyhow::ensure!(super::on_path("deno"), "Deno is not installed");
    anyhow::ensure!(deno_supported().await, "Deno 2.4.3 or newer is needed");
    let dir = pot_dir();
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(dir.join("plugins")).await?;
    let plugin = fetch(&format!(
        "{REPO}/releases/download/{BGUTIL}/bgutil-ytdlp-pot-provider.zip"
    ))
    .await?;
    tokio::fs::write(dir.join("plugins/bgutil-ytdlp-pot-provider.zip"), plugin).await?;
    let source = fetch(&format!("{REPO}/archive/refs/tags/{BGUTIL}.tar.gz")).await?;
    tokio::fs::write(dir.join("source.tar.gz"), source).await?;
    // Relative paths: GNU tar on Windows reads "C:" as a remote host.
    let mut tar = command("tar");
    tar.args(["-xzf", "source.tar.gz"]).current_dir(&dir);
    run(tar, 60).await?;
    let _ = tokio::fs::remove_file(dir.join("source.tar.gz")).await;
    let server = server_home();
    // Only the runtime dependencies; canvas is an optional native jsdom add-on.
    let package = server.join("package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read(&package).await?)?;
    if let Some(object) = manifest.as_object_mut() {
        object.remove("devDependencies");
        object.remove("allowScripts");
    }
    if let Some(deps) = manifest["dependencies"].as_object_mut() {
        deps.remove("canvas");
    }
    tokio::fs::write(&package, serde_json::to_vec_pretty(&manifest)?).await?;
    for lock in ["deno.json", "deno.lock", "package-lock.json"] {
        let _ = tokio::fs::remove_file(server.join(lock)).await;
    }
    let mut install = command("deno");
    install.arg("install").current_dir(&server);
    run(install, 300).await?;
    // Run the script the way the plugin does to be sure it starts.
    let modules = server.join("node_modules").display().to_string();
    let token_cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".cache"))
        })
        .unwrap_or_else(|| server.clone())
        .join("bgutil-ytdlp-pot-provider")
        .display()
        .to_string();
    let mut check = command("deno");
    check
        .args([
            "run".to_string(),
            "--allow-env".into(),
            "--allow-net".into(),
            format!("--allow-ffi={modules}"),
            format!("--allow-write={token_cache}"),
            format!("--allow-read={token_cache},{modules}"),
            "src/generate_once.ts".into(),
            "--version".into(),
        ])
        .current_dir(&server);
    run(check, 60).await?;
    tokio::fs::write(dir.join("ready"), BGUTIL).await?;
    Ok(())
}

/// How a failed yt-dlp run should be handled.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// Bot check, sign-in wall, 403 or missing formats: try another layer.
    Blocked,
    /// Rate limited: back off, then try another layer.
    RateLimited,
    /// Browser cookies could not be read: stop using them.
    Cookies,
    /// The video itself is unavailable; no layer will help.
    Fatal,
}

fn classify(error: &str) -> Failure {
    let lower = error.to_lowercase();
    if lower.contains("private video") || lower.contains("video unavailable") {
        Failure::Fatal
    } else if lower.contains("429")
        || lower.contains("too many requests")
        || lower.contains("try again later")
    {
        Failure::RateLimited
    } else if lower.contains("not a bot")
        || lower.contains("sign in")
        || lower.contains("403")
        || lower.contains("requested format is not available")
        || lower.contains("page needs to be reloaded")
        || lower.contains("po token")
    {
        Failure::Blocked
    } else if lower.contains("cookie") || lower.contains("keyring") || lower.contains("decrypt") {
        Failure::Cookies
    } else {
        Failure::Fatal
    }
}

/// Strategies in the order they are tried: the last one that worked, then the
/// client sets, then IPv4, then without cookies.
fn candidates(first: Strategy, have_cookies: bool) -> Vec<Strategy> {
    let mut list = vec![first];
    let cookie_options: &[bool] = if have_cookies {
        &[true, false]
    } else {
        &[false]
    };
    for &cookies in cookie_options {
        for ipv4 in [false, true] {
            for clients in 0..CLIENTS.len() {
                let s = Strategy {
                    clients,
                    ipv4,
                    cookies,
                };
                if !list.iter().any(|seen| same(*seen, s, have_cookies)) {
                    list.push(s);
                }
            }
        }
    }
    list
}

/// Cookies only matter when a browser is set.
fn same(a: Strategy, b: Strategy, have_cookies: bool) -> bool {
    a.clients == b.clients && a.ipv4 == b.ipv4 && (!have_cookies || a.cookies == b.cookies)
}

async fn probe(url: &str, strategy: Strategy) -> Result<String, String> {
    let mut command = command("yt-dlp");
    if let Some(runtime) = super::js_runtime() {
        command.args(["--js-runtimes", runtime]);
    }
    command.args(args(strategy)).args([
        "-f",
        "bestaudio/best",
        "-g",
        "--no-playlist",
        "--no-warnings",
        url,
    ]);
    let output = tokio::time::timeout(Duration::from_secs(60), command.output())
        .await
        .map_err(|_| "yt-dlp timed out".to_string())?
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        return String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .map(str::to_string)
            .ok_or_else(|| "yt-dlp printed no stream URL".to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(stderr
        .lines()
        .find_map(|l| l.strip_prefix("ERROR: "))
        .or_else(|| stderr.lines().last())
        .unwrap_or("yt-dlp failed")
        .trim()
        .to_string())
}

/// Serializes unblock runs; holds when the last rate limit backs off until.
static GATE: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);

/// Finds a way past a YouTube refusal for `url`, trying each layer in turn.
/// On success the strategy is remembered (and mpv's config rewritten) and its
/// description and the stream URL it resolved returned; otherwise the last
/// yt-dlp error. Runs one at a time, waits out a recent rate limit, and gives
/// up after two minutes.
pub async fn unblock(url: &str) -> Result<(String, Option<String>), String> {
    prepare();
    let setup_done = SETUP_DONE.notified();
    if SETTING_UP.load(Ordering::SeqCst) {
        // Give the provider setup that is running a chance to finish.
        let _ = tokio::time::timeout(Duration::from_secs(90), setup_done).await;
    }
    let mut cooldown = GATE.lock().await;
    let deadline = Instant::now() + Duration::from_secs(120);
    let have_cookies = super::cookies_browser().is_some();
    let mut backoff = Duration::from_secs(4);
    let mut cookies_broken = false;
    let mut last = String::from("YouTube refused every strategy");
    for strategy in candidates(current(), have_cookies) {
        if Instant::now() > deadline {
            break;
        }
        if cookies_broken && strategy.cookies && have_cookies {
            continue;
        }
        if let Some(until) = cooldown.take() {
            tokio::time::sleep_until(until.into()).await;
        }
        match probe(url, strategy).await {
            Ok(direct) => {
                log::info!("YouTube access: {} works", strategy.describe());
                remember(strategy);
                return Ok((strategy.describe(), Some(direct)));
            }
            Err(error) => {
                log::info!("YouTube access: {} failed: {error}", strategy.describe());
                match classify(&error) {
                    Failure::Fatal => return Err(error),
                    Failure::Cookies => cookies_broken = true,
                    Failure::RateLimited => {
                        *cooldown = Some(Instant::now() + backoff);
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                    }
                    Failure::Blocked => {}
                }
                last = error;
            }
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_round_trips() {
        let s = Strategy {
            clients: 2,
            ipv4: true,
            cookies: false,
        };
        assert_eq!(Strategy::decode(&s.encode()), Some(s));
        assert_eq!(Strategy::decode("clients=99"), None);
    }

    #[test]
    fn candidates_start_with_the_last_winner_and_skip_duplicates() {
        let first = Strategy {
            clients: 1,
            ipv4: true,
            cookies: true,
        };
        let list = candidates(first, true);
        assert_eq!(list[0], first);
        assert_eq!(list.len(), CLIENTS.len() * 4);
        assert!(list[1].cookies && !list[1].ipv4);
        assert!(!list.last().unwrap().cookies);
        assert_eq!(candidates(first, false).len(), CLIENTS.len() * 2);
    }

    #[test]
    fn errors_pick_the_next_layer() {
        assert_eq!(
            classify(
                "[youtube] x: Sign in to confirm you’re not a bot. Use --cookies-from-browser"
            ),
            Failure::Blocked
        );
        assert_eq!(
            classify("HTTP Error 429: Too Many Requests"),
            Failure::RateLimited
        );
        assert_eq!(
            classify("Could not copy Chrome cookie database"),
            Failure::Cookies
        );
        assert_eq!(classify("[youtube] x: Video unavailable"), Failure::Fatal);
    }

    #[test]
    fn config_quotes_values() {
        assert_eq!(quote(r"C:\Users\o'neil"), r#"'C:\Users\o'"'"'neil'"#);
    }
}
