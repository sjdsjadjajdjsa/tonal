use crate::model::{SourceKind, Track};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc::UnboundedSender,
};

pub async fn download(
    track: &Track,
    dir: &Path,
    progress: UnboundedSender<f32>,
) -> Result<PathBuf> {
    if track.source == SourceKind::Local {
        let _ = progress.send(1.0);
        return Ok(PathBuf::from(&track.play_url));
    }
    tokio::fs::create_dir_all(dir).await?;
    remove_stale_temps(dir).await;
    let _ = progress.send(0.0);
    match download_attempt(track, dir, &progress, true).await {
        Err(error) if error.to_string().contains("module mutagen was not found") => {
            log::warn!(
                "yt-dlp cannot embed artwork without mutagen; retrying with audio metadata only"
            );
            download_attempt(track, dir, &progress, false).await
        }
        result => result,
    }
}

fn temp_name(pid: u32, n: u64) -> String {
    format!(".tonal-{pid}-{n}.part")
}

/// A partial-download folder left by another (earlier) process.
fn is_stale_temp(name: &str, pid: u32) -> bool {
    name.strip_prefix(".tonal-")
        .and_then(|rest| rest.strip_suffix(".part"))
        .and_then(|rest| rest.split_once('-'))
        .is_some_and(|(owner, _)| owner != pid.to_string())
}

async fn remove_stale_temps(dir: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let pid = std::process::id();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| is_stale_temp(name, pid))
        {
            let _ = tokio::fs::remove_dir_all(entry.path()).await;
        }
    }
}

fn download_command(url: &str, dir: &Path, temp: &Path, artwork: bool) -> Command {
    let mut child = super::ytdlp();
    child.args(["-x", "--audio-format", "opus", "--embed-metadata"]);
    if artwork {
        child.arg("--embed-thumbnail");
    }
    let mut home = std::ffi::OsString::from("home:");
    home.push(dir);
    let mut partial = std::ffi::OsString::from("temp:");
    partial.push(temp);
    child
        .args(["--newline", "--no-quiet", "-P"])
        .arg(home)
        .arg("-P")
        .arg(partial)
        .args(["-o", "%(artist,uploader)s - %(title)s.%(ext)s"])
        .args(["--print", "after_move:filepath", "--", url])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    child
}

async fn download_attempt(
    track: &Track,
    dir: &Path,
    progress: &UnboundedSender<f32>,
    artwork: bool,
) -> Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temp = dir.join(temp_name(
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));
    let mut child = download_command(&track.play_url, dir, &temp, artwork).spawn()?;
    #[cfg(windows)]
    crate::job::adopt(child.raw_handle());
    let stdout = child.stdout.take().context("yt-dlp stdout unavailable")?;
    let stderr = child.stderr.take().context("yt-dlp stderr unavailable")?;
    let errors = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut output = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            output.push_str(&line);
            output.push('\n');
        }
        output
    });
    let job = async {
        let mut lines = BufReader::new(stdout).lines();
        let mut final_path = None;
        while let Some(line) = lines.next_line().await? {
            if let Some(percent) = line
                .strip_prefix("[download]")
                .and_then(|rest| rest.trim().split('%').next())
                .and_then(|text| text.trim().parse::<f32>().ok())
            {
                let _ = progress.send((percent / 100.0).clamp(0.0, 1.0));
            }
            if line.starts_with(dir.to_string_lossy().as_ref()) {
                final_path = Some(PathBuf::from(line));
            }
        }
        let status = child.wait().await?;
        let error = errors.await.unwrap_or_default();
        if !status.success() {
            anyhow::bail!("yt-dlp download failed: {error}");
        }
        let path = final_path.context("yt-dlp did not report a downloaded path")?;
        if !path.is_file() {
            anyhow::bail!("yt-dlp reported missing file: {}", path.display());
        }
        let _ = progress.send(1.0);
        Ok(path)
    };
    let result = tokio::time::timeout(Duration::from_secs(600), job).await;
    if result.is_err() {
        let _ = child.kill().await;
    }
    let _ = tokio::fs::remove_dir_all(&temp).await;
    result?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_dash_url_is_not_an_option() {
        let command = download_command(
            "--exec=untrusted",
            Path::new("downloads"),
            Path::new("downloads/.part"),
            true,
        );
        let args: Vec<_> = command.as_std().get_args().collect();
        assert_eq!(args[args.len() - 2], "--");
        assert_eq!(args[args.len() - 1], "--exec=untrusted");
    }

    #[test]
    fn partial_files_go_to_their_own_folder() {
        let command = download_command(
            "https://youtu.be/x",
            Path::new("downloads"),
            Path::new("downloads/.part"),
            false,
        );
        let args: Vec<_> = command.as_std().get_args().collect();
        let after = |flag: &str| {
            args.iter()
                .enumerate()
                .filter(|(_, a)| **a == flag)
                .map(|(i, _)| args[i + 1])
                .collect::<Vec<_>>()
        };
        assert_eq!(after("-P"), ["home:downloads", "temp:downloads/.part"]);
        assert_eq!(after("-o"), ["%(artist,uploader)s - %(title)s.%(ext)s"]);
    }

    #[test]
    fn only_other_processes_temps_are_stale() {
        assert!(is_stale_temp(&temp_name(41, 0), 42));
        assert!(!is_stale_temp(&temp_name(42, 7), 42));
        assert!(!is_stale_temp(".tonal-41.part", 42));
        assert!(!is_stale_temp("Artist - Song.opus", 42));
        assert!(!is_stale_temp(".tonal-41-0.partial", 42));
    }

    #[tokio::test]
    async fn stale_temps_are_removed_and_others_kept() {
        let dir = std::env::temp_dir().join(format!("tonal-download-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let stale = dir.join(temp_name(std::process::id().wrapping_add(1), 0));
        let live = dir.join(temp_name(std::process::id(), 0));
        let song = dir.join("Artist - Song.opus");
        std::fs::create_dir_all(stale.join("inner")).unwrap();
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(&song, b"").unwrap();
        remove_stale_temps(&dir).await;
        assert!(!stale.exists());
        assert!(live.exists() && song.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
