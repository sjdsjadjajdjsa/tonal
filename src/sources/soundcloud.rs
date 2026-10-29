use crate::model::{ArtistPage, ArtistRef, SourceKind, Track};
use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;

async fn metadata(target: &str, limit: usize) -> Result<Value> {
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        super::ytdlp()
            .args([
                "-J",
                "--flat-playlist",
                "--playlist-end",
                &limit.to_string(),
                "--",
                target,
            ])
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await??;
    anyhow::ensure!(
        output.status.success(),
        "SoundCloud metadata: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).context("SoundCloud metadata JSON")
}

fn track(entry: &Value) -> Option<Track> {
    let url = entry["webpage_url"]
        .as_str()
        .or_else(|| entry["url"].as_str())?
        .to_owned();
    let id = entry["id"].as_str().unwrap_or(&url).to_owned();
    let artist = entry["uploader"]
        .as_str()
        .unwrap_or("Unknown artist")
        .to_owned();
    let profile = entry["uploader_url"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            let parsed = url::Url::parse(&url).ok()?;
            let user = parsed.path_segments()?.find(|part| !part.is_empty())?;
            Some(format!("https://soundcloud.com/{user}"))
        });
    let artwork_url = entry["thumbnails"]
        .as_array()
        .and_then(|thumbs| {
            thumbs
                .iter()
                .filter(|t| t["url"].as_str().is_some())
                .max_by_key(|t| t["width"].as_u64().unwrap_or(0))
        })
        .and_then(|t| t["url"].as_str())
        .map(str::to_owned)
        .or_else(|| entry["thumbnail"].as_str().map(str::to_owned));
    Some(Track {
        id: Track::make_id(SourceKind::SoundCloud, &id),
        source: SourceKind::SoundCloud,
        source_id: id,
        title: entry["title"].as_str()?.to_owned(),
        artist_refs: vec![ArtistRef {
            source: SourceKind::SoundCloud,
            name: artist.clone(),
            id: profile,
        }],
        artist,
        album: entry["album"].as_str().map(str::to_owned),
        album_id: None,
        duration_secs: entry["duration"].as_f64(),
        artwork_url,
        play_url: url,
    })
}

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<Track>> {
    let data = metadata(&format!("scsearch{}:{query}", limit.min(100)), limit).await?;
    Ok(data["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(track)
        .take(limit)
        .collect())
}

pub(super) async fn artist(r: &ArtistRef) -> Result<ArtistPage> {
    let profile =
        r.id.as_deref()
            .context("SoundCloud artist has no profile URL")?;
    let parsed = url::Url::parse(profile)?;
    anyhow::ensure!(
        matches!(
            parsed.host_str(),
            Some("soundcloud.com" | "www.soundcloud.com")
        ) && parsed.scheme() == "https",
        "Invalid SoundCloud profile URL"
    );
    let target = format!(
        "{}/tracks",
        profile.trim_end_matches('/').trim_end_matches("/tracks")
    );
    let data = metadata(&target, 50).await?;
    let mut artist = r.clone();
    if let Some(name) = data["uploader"].as_str() {
        artist.name = name.to_owned();
    }
    let top = data["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(track)
        .map(|mut song| {
            // Flat profile entries often contain only id, URL and title.
            if song.artist == "Unknown artist" {
                song.artist = artist.name.clone();
                song.artist_refs = vec![artist.clone()];
            }
            song
        })
        .take(50)
        .collect();
    Ok(ArtistPage {
        artist,
        top,
        ..Default::default()
    })
}

async fn client_id() -> Result<String> {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(id) = ID.get() {
        return Ok(id.clone());
    }
    let html = super::client()
        .get("https://soundcloud.com")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let scripts: Vec<_> = html
        .split("src=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .filter(|s| s.starts_with("https://a-v2.sndcdn.com/assets/") && s.ends_with(".js"))
        .collect();
    for script in scripts.into_iter().rev().take(4) {
        let code = super::client()
            .get(script)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        for marker in ["client_id:\"", "client_id=\""] {
            if let Some(id) = code
                .split(marker)
                .skip(1)
                .filter_map(|s| s.split('"').next())
                .find(|id| id.len() == 32 && id.chars().all(|c| c.is_ascii_alphanumeric()))
            {
                let _ = ID.set(id.to_owned());
                return Ok(id.to_owned());
            }
        }
    }
    anyhow::bail!("SoundCloud user search: could not discover public client id")
}

pub(super) async fn find_artist(name: &str) -> Result<Option<ArtistRef>> {
    let client_id = client_id().await?;
    let mut url = url::Url::parse("https://api-v2.soundcloud.com/search/users")?;
    {
        url.query_pairs_mut()
            .append_pair("q", name)
            .append_pair("limit", "20")
            .append_pair("client_id", &client_id);
    }
    let data: Value = super::client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(data["collection"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|user| {
            Some((
                ArtistRef {
                    source: SourceKind::SoundCloud,
                    name: user["username"].as_str()?.to_owned(),
                    id: Some(user["permalink_url"].as_str()?.to_owned()),
                },
                user["followers_count"].as_u64().unwrap_or(0),
            ))
        })
        .max_by_key(|(r, followers)| (super::name_score(&r.name, name), *followers))
        .map(|(r, _)| r))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flat_track_links_to_uploader() {
        let entry = json!({"id": "42", "title": "Song", "uploader": "Artist", "url": "https://soundcloud.com/person/song"});
        let song = track(&entry).expect("track");
        assert_eq!(
            song.artist_refs[0].id.as_deref(),
            Some("https://soundcloud.com/person")
        );
        assert_eq!(song.artist, "Artist");
        assert_eq!(song.album_id, None);
    }
}
