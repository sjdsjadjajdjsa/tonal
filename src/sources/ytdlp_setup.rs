//! macOS: without an installed yt-dlp, Tonal downloads yt-dlp's official macOS
//! build into its cache folder and checks it against the release's SHA2-256SUMS.
//! It is the folder build: the single-file one unpacks itself on every run and
//! takes seconds to start. main() puts the folder last on PATH, so Homebrew's
//! yt-dlp (which Homebrew's mpv brings along) wins when it is there.

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const LATEST: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
const ASSET: &str = "yt-dlp_macos.zip";

static BUSY: AtomicBool = AtomicBool::new(false);

/// The folder holding Tonal's own yt-dlp.
pub fn dir() -> PathBuf {
    crate::model::paths::cache_dir().join("yt-dlp-macos")
}

/// True when the yt-dlp on PATH is the one Tonal downloaded.
pub fn owns_ytdlp() -> bool {
    super::find_on_path("yt-dlp").is_some_and(|path| path.starts_with(dir()))
}

/// Downloads the newest yt-dlp into dir(), in place of an older one there.
/// Returns what changed.
pub async fn install() -> Result<String> {
    ensure!(
        !BUSY.swap(true, Ordering::SeqCst),
        "yt-dlp is already being set up"
    );
    let result = install_newest().await;
    BUSY.store(false, Ordering::SeqCst);
    result
}

async fn install_newest() -> Result<String> {
    // "latest" redirects to the release's own folder (and from there on to GitHub's
    // file servers); taking the zip from that folder keeps it and the checksum
    // from the same release.
    let redirect = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()?
        .get(LATEST)
        .send()
        .await?;
    let location = redirect
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let (folder, version) = release_folder(location).with_context(|| {
        format!(
            "unexpected answer from GitHub (HTTP {})",
            redirect.status().as_u16()
        )
    })?;
    let client = super::client();
    let sums = client
        .get(format!("{folder}/SHA2-256SUMS"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let expected =
        checksum(&sums, ASSET).context("yt-dlp's release has no checksum for its macOS build")?;
    let installed = dir().join("yt-dlp");
    let current = version_of(&installed).await;
    if current.as_deref() == Some(version.as_str()) {
        return Ok("yt-dlp is up to date".into());
    }

    let cache = crate::model::paths::cache_dir();
    let zip = cache.join("yt-dlp-macos.zip");
    let mut response = client
        .get(format!("{folder}/{ASSET}"))
        .timeout(Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?;
    let mut file = tokio::fs::File::create(&zip).await?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response.chunk().await? {
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != expected {
        let _ = std::fs::remove_file(&zip);
        bail!("the yt-dlp download was damaged, try again");
    }

    let staged = cache.join("yt-dlp-macos.new");
    let unpacked = staged.clone();
    let result = tokio::task::spawn_blocking(move || unpack(&zip, &unpacked)).await?;
    let _ = std::fs::remove_file(cache.join("yt-dlp-macos.zip"));
    result?;
    let works = version_of(&staged.join("yt-dlp")).await;
    if works.is_none() {
        let _ = std::fs::remove_dir_all(&staged);
        bail!("the downloaded yt-dlp doesn't start");
    }

    let target = dir();
    let old = cache.join("yt-dlp-macos.old");
    let _ = std::fs::remove_dir_all(&old);
    if target.exists() {
        std::fs::rename(&target, &old)?;
    }
    std::fs::rename(&staged, &target)?;
    let _ = std::fs::remove_dir_all(&old);
    Ok(match current {
        Some(_) => format!("Updated yt-dlp to {version}"),
        None => format!("yt-dlp {version} is ready"),
    })
}

/// Unpacks the release zip into `staged` as `staged/yt-dlp` and `staged/_internal`.
fn unpack(zip: &Path, staged: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::remove_dir_all(staged);
    zip::ZipArchive::new(std::fs::File::open(zip)?)?.extract(staged)?;
    let program = staged.join("yt-dlp");
    std::fs::rename(staged.join("yt-dlp_macos"), &program)
        .context("the yt-dlp download has no yt-dlp_macos in it")?;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// What `program --version` prints, or None when it doesn't run.
async fn version_of(program: &Path) -> Option<String> {
    if !program.is_file() {
        return None;
    }
    let mut command = tokio::process::Command::new(program);
    command.arg("--version").kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .ok()?
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The release folder and version in the link "latest" redirected to, like
/// `https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.26/SHA2-256SUMS`.
fn release_folder(url: &str) -> Option<(String, String)> {
    let (folder, _) = url.rsplit_once('/')?;
    let (parent, version) = folder.rsplit_once('/')?;
    (parent.ends_with("/releases/download") && !version.is_empty())
        .then(|| (folder.to_owned(), version.to_owned()))
}

/// The SHA-256 listed for `name` in a SHA2-256SUMS file.
fn checksum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim_start().trim_start_matches('*') == name).then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_release_folder_after_the_redirect() {
        assert_eq!(
            release_folder(
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.26/SHA2-256SUMS"
            ),
            Some((
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.26".into(),
                "2026.10.26".into()
            ))
        );
        assert_eq!(release_folder(LATEST), None);
    }

    #[test]
    fn picks_the_checksum_of_the_zip_not_the_single_file() {
        let sums = "0f92  yt-dlp_macos\n20D9  yt-dlp_macos.zip\nabcd *yt-dlp.exe\n";
        assert_eq!(checksum(sums, ASSET).as_deref(), Some("20d9"));
        assert_eq!(checksum(sums, "yt-dlp.exe").as_deref(), Some("abcd"));
        assert_eq!(checksum(sums, "yt-dlp_linux"), None);
    }
}
