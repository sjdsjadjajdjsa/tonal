//! Updates from Tonal's GitHub releases. The Mac app, the Windows installer and the
//! AppImage replace themselves and start again; other installs open the release page.

use anyhow::Result;
use parking_lot::Mutex;
use serde::Deserialize;
use std::{path::PathBuf, process::Command, time::Duration};

const LATEST: &str = "https://api.github.com/repos/sjdsjadjajdjsa/tonal/releases/latest";
pub const PAGE: &str = "https://github.com/sjdsjadjajdjsa/tonal/releases/latest";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Unknown,
    Checking,
    Current,
    /// A newer version; `url` is the download that installs itself, if this install has one.
    Available {
        version: String,
        url: Option<String>,
    },
    Installing,
    Failed,
}

static STATE: Mutex<State> = Mutex::new(State::Unknown);

pub fn state() -> State {
    STATE.lock().clone()
}

pub fn set_state(state: State) {
    *STATE.lock() = state;
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim_start_matches('v').splitn(3, '.');
    let mut next = || {
        parts
            .next()?
            .split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()
    };
    Some((next()?, next()?, next()?))
}

/// True when `latest` is a later version than `current`.
pub fn newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

/// The version running; debug builds can pretend to be older to test updating.
fn running() -> String {
    #[cfg(feature = "debug-hooks")]
    if let Ok(version) = std::env::var("TONAL_DEBUG_VERSION") {
        return version;
    }
    VERSION.to_owned()
}

/// The download this install can put in place of itself.
fn asset() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        bundle().map(|_| "tonal-macos-arm64.dmg")
    } else if cfg!(windows) {
        installed_by_setup().then_some("tonal-setup-x86_64.exe")
    } else {
        appimage().map(|_| "tonal-x86_64.AppImage")
    }
}

/// Asks GitHub for the latest release.
pub async fn check() -> Result<State> {
    let release: Release = super::client()
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if !newer(&release.tag_name, &running()) {
        return Ok(State::Current);
    }
    let url = asset().and_then(|name| {
        release
            .assets
            .into_iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url)
    });
    Ok(State::Available {
        version: release.tag_name.trim_start_matches('v').to_owned(),
        url,
    })
}

/// Downloads `url` and puts it in place of this install. Once it returns, quit Tonal:
/// the new version starts as soon as this one has closed.
pub async fn install(url: &str) -> Result<()> {
    let bytes = super::client()
        .get(url)
        .timeout(Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let dir = crate::model::paths::cache_dir().join("update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let name = url.rsplit('/').next().unwrap_or("update");
    let file = dir.join(name);
    std::fs::write(&file, &bytes)?;
    tokio::task::spawn_blocking(move || replace(&file, &dir)).await?
}

#[cfg(target_os = "macos")]
fn run(command: &mut Command) -> Result<()> {
    use anyhow::{Context, bail};
    let out = command.output().with_context(|| format!("{command:?}"))?;
    if !out.status.success() {
        bail!(
            "{command:?} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Runs `program` with `args` once this process has exited.
#[cfg(unix)]
fn after_exit(program: &std::path::Path, args: &[&std::path::Path]) -> Result<()> {
    let script = "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; shift; exec \"$@\"";
    Command::new("sh")
        .args(["-c", script, "sh"])
        .arg(std::process::id().to_string())
        .arg(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn replace(dmg: &std::path::Path, dir: &std::path::Path) -> Result<()> {
    use anyhow::Context;
    let app = bundle().context("Tonal isn't running from Tonal.app")?;
    let mount = dir.join("mount");
    std::fs::create_dir_all(&mount)?;
    run(Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
        .arg(&mount)
        .arg(dmg))?;
    let staged = app.with_file_name("Tonal.update.app");
    let _ = std::fs::remove_dir_all(&staged);
    let copied = run(Command::new("ditto")
        .arg(mount.join("Tonal.app"))
        .arg(&staged));
    let _ = run(Command::new("hdiutil")
        .arg("detach")
        .arg(&mount)
        .arg("-quiet"));
    copied?;
    let old = app.with_file_name("Tonal.old.app");
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(&app, &old)?;
    if let Err(e) = std::fs::rename(&staged, &app) {
        let _ = std::fs::rename(&old, &app);
        return Err(e.into());
    }
    let _ = std::fs::remove_dir_all(&old);
    let _ = run(Command::new("xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&app));
    after_exit(std::path::Path::new("open"), &[&app])
}

#[cfg(windows)]
fn replace(setup: &std::path::Path, _dir: &std::path::Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = std::env::current_exe()?;
    let script = format!(
        "start /wait \"\" \"{}\" /SILENT /SUPPRESSMSGBOXES /NORESTART & start \"\" \"{}\"",
        setup.display(),
        exe.display()
    );
    Command::new("cmd")
        .arg("/C")
        .raw_arg(script)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn replace(image: &std::path::Path, _dir: &std::path::Path) -> Result<()> {
    use anyhow::Context;
    use std::os::unix::fs::PermissionsExt;
    let target = appimage().context("Tonal isn't running as an AppImage")?;
    let staged = target.with_extension("new");
    std::fs::copy(image, &staged)?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&staged, &target)?;
    after_exit(&target, &[])
}

/// The Tonal.app this runs from, when it runs from one.
fn bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension()? == "app").then(|| app.to_path_buf())
}

/// True when the Windows installer put Tonal here, so running it again updates it.
fn installed_by_setup() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|e| Some(e.parent()?.join("unins000.exe").is_file()))
        .unwrap_or(false)
}

fn appimage() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_by_number() {
        assert!(newer("v0.5.2", "0.5.1"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "0.99.0"));
        assert!(!newer("v0.5.1", "0.5.1"));
        assert!(!newer("v0.5.0", "0.5.1"));
        assert!(!newer("nightly", "0.5.1"));
    }

    #[test]
    fn reads_a_github_release() {
        let json = r#"{"tag_name":"v1.2.0","published_at":"2026-11-10T00:00:00Z","assets":[
            {"id":1,"name":"tonal-macos-arm64.dmg","size":9124409,"browser_download_url":"https://github.com/sjdsjadjajdjsa/tonal/releases/download/v1.2.0/tonal-macos-arm64.dmg"},
            {"id":2,"name":"SHA256SUMS","size":542,"browser_download_url":"https://github.com/sjdsjadjajdjsa/tonal/releases/download/v1.2.0/SHA256SUMS"}]}"#;
        let release: Release = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v1.2.0");
        assert_eq!(release.assets[0].name, "tonal-macos-arm64.dmg");
        assert!(
            release.assets[0]
                .browser_download_url
                .ends_with("/download/v1.2.0/tonal-macos-arm64.dmg")
        );
    }
}
