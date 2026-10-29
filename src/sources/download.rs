use crate::model::{SourceKind, Track};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
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

fn download_command(url: &str, dir: &Path, artwork: bool) -> Command {
    let template = dir.join("%(artist,uploader)s - %(title)s.%(ext)s");
    let mut child = super::ytdlp();
    child.args(["-x", "--audio-format", "opus", "--embed-metadata"]);
    if artwork {
        child.arg("--embed-thumbnail");
    }
    child
        .args(["--newline", "--no-quiet", "-o"])
        .arg(&template)
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
    let mut child = download_command(&track.play_url, dir, artwork).spawn()?;
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
    tokio::time::timeout(Duration::from_secs(600), job).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_dash_url_is_not_an_option() {
        let command = download_command("--exec=untrusted", Path::new("downloads"), true);
        let args: Vec<_> = command.as_std().get_args().collect();
        assert_eq!(args[args.len() - 2], "--");
        assert_eq!(args[args.len() - 1], "--exec=untrusted");
    }
}
