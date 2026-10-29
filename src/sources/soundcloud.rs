use crate::model::{SourceKind, Track};
use anyhow::Result;
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<Track>> {
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        super::ytdlp()
            .arg("-J")
            .arg("--flat-playlist")
            .arg(format!("scsearch{}:{query}", limit.min(100)))
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await??;
    if !output.status.success() {
        anyhow::bail!(
            "SoundCloud search: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let data: Value = serde_json::from_slice(&output.stdout)?;
    Ok(data["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let url = entry["webpage_url"]
                .as_str()
                .or_else(|| entry["url"].as_str())?
                .to_owned();
            let id = entry["id"].as_str().unwrap_or(&url).to_owned();
            let artwork_url = entry["thumbnails"]
                .as_array()
                .and_then(|thumbnails| {
                    thumbnails
                        .iter()
                        .filter(|t| t["url"].as_str().is_some())
                        .max_by_key(|t| t["width"].as_u64().unwrap_or(0))
                })
                .and_then(|t| t["url"].as_str())
                .map(str::to_owned);
            Some(Track {
                id: Track::make_id(SourceKind::SoundCloud, &id),
                source: SourceKind::SoundCloud,
                source_id: id,
                title: entry["title"].as_str()?.to_owned(),
                artist: entry["uploader"]
                    .as_str()
                    .unwrap_or("Unknown artist")
                    .to_owned(),
                album: None,
                duration_secs: entry["duration"].as_f64(),
                artwork_url,
                play_url: url,
            })
        })
        .take(limit)
        .collect())
}
