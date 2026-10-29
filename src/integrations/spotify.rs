use anyhow::Context;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};

#[derive(Clone, Debug, Default)]
pub struct SpotifyTrack {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: Option<f64>,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SpotifyPlaylist {
    pub id: String,
    pub name: String,
    pub track_count: usize,
    pub image_url: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SpotifyAuth {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

pub const REDIRECT_URI: &str = "http://127.0.0.1:8898/callback";

fn public_id(input: &str) -> anyhow::Result<(String, String)> {
    let (kind, id) = if input.starts_with("spotify:") {
        let parts: Vec<_> = input.split(':').collect();
        anyhow::ensure!(
            parts.len() == 3,
            "Expected spotify:playlist:ID or spotify:album:ID"
        );
        (parts[1].to_string(), parts[2].to_string())
    } else {
        let url = url::Url::parse(input)?;
        anyhow::ensure!(
            url.host_str() == Some("open.spotify.com") && url.scheme() == "https",
            "Expected an open.spotify.com URL"
        );
        let parts: Vec<_> = url
            .path_segments()
            .context("Missing Spotify URL path")?
            .collect();
        let index = parts
            .iter()
            .position(|s| matches!(*s, "playlist" | "album"))
            .context("Expected a playlist or album URL")?;
        (
            parts[index].to_string(),
            parts
                .get(index + 1)
                .context("Missing Spotify ID")?
                .to_string(),
        )
    };
    anyhow::ensure!(
        matches!(kind.as_str(), "playlist" | "album")
            && !id.is_empty()
            && id.bytes().all(|b| b.is_ascii_alphanumeric()),
        "Invalid Spotify playlist/album ID"
    );
    Ok((kind, id))
}

fn embed_json(html: &str) -> anyhow::Result<Value> {
    let marker = html
        .find("id=\"__NEXT_DATA__\"")
        .or_else(|| html.find("id='__NEXT_DATA__'"))
        .context("Spotify embed has no __NEXT_DATA__ script")?;
    let start = marker + html[marker..].find('>').context("Invalid embed script")? + 1;
    let end = start
        + html[start..]
            .find("</script>")
            .context("Unterminated embed script")?;
    Ok(serde_json::from_str(&html[start..end])?)
}

pub async fn fetch_public(url: &str) -> anyhow::Result<(String, Vec<SpotifyTrack>)> {
    let (kind, id) = public_id(url.trim())?;
    let html = super::client()
        .get(format!("https://open.spotify.com/embed/{kind}/{id}"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let data = embed_json(&html)?;
    let entity = data
        .pointer("/props/pageProps/state/data/entity")
        .context("Spotify embed has no entity")?;
    let name = entity["name"]
        .as_str()
        .or_else(|| entity["title"].as_str())
        .unwrap_or("Spotify import")
        .to_string();
    let artwork = entity
        .pointer("/coverArt/sources/0/url")
        .and_then(Value::as_str)
        .map(str::to_string);
    let list = entity["trackList"]
        .as_array()
        .context("Spotify embed has no track list (private or unavailable?)")?;
    let tracks = list
        .iter()
        .filter_map(|item| {
            Some(SpotifyTrack {
                title: item["title"].as_str()?.to_string(),
                artist: item["subtitle"]
                    .as_str()
                    .unwrap_or("")
                    .replace('\u{a0}', " "),
                album: (kind == "album").then(|| name.clone()),
                duration_secs: item["duration"].as_f64().map(|v| v / 1000.0),
                artwork_url: artwork.clone(),
            })
        })
        .collect();
    // Spotify's anonymous embed exposes at most 100 entries; full imports need OAuth.
    Ok((name, tracks))
}

async fn token(fields: &[(&str, &str)], old_refresh: &str) -> anyhow::Result<SpotifyAuth> {
    let value: Value = super::client()
        .post("https://accounts.spotify.com/api/token")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(super::form(fields))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(SpotifyAuth {
        access_token: value["access_token"]
            .as_str()
            .context("Spotify returned no access token")?
            .to_string(),
        refresh_token: value["refresh_token"]
            .as_str()
            .unwrap_or(old_refresh)
            .to_string(),
        expires_at: super::now() + value["expires_in"].as_i64().unwrap_or(3600),
    })
}

async fn callback(listener: tokio::net::TcpListener, state: &str) -> anyhow::Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(5), async {
            let mut reader = BufReader::new(&mut stream);
            while line.len() < 8192 {
                let mut byte = [0u8; 1];
                use tokio::io::AsyncReadExt;
                if reader.read(&mut byte).await? == 0 {
                    break;
                }
                line.push(byte[0] as char);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Ok::<_, std::io::Error>(())
        })
        .await;
        if !matches!(read, Ok(Ok(()))) {
            continue;
        }
        let target = line.split_whitespace().nth(1).unwrap_or("");
        let Ok(url) = url::Url::parse(&format!("http://127.0.0.1:8898{target}")) else {
            continue;
        };
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        let valid = line.starts_with("GET ")
            && url.path() == "/callback"
            && pairs.get("state").map(String::as_str) == Some(state);
        let body = if valid {
            "<html><body>You can close this tab and return to Tonal.</body></html>"
        } else {
            "<html><body>Invalid callback. Return to Tonal and try again.</body></html>"
        };
        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            if valid { "200 OK" } else { "400 Bad Request" },
            body.len()
        );
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            stream.write_all(response.as_bytes()),
        )
        .await;
        let _ = stream.shutdown().await;
        if !valid {
            continue;
        }
        if let Some(error) = pairs.get("error") {
            anyhow::bail!("Spotify authorization declined: {error}");
        }
        return pairs
            .get("code")
            .cloned()
            .context("Spotify callback had no authorization code");
    }
}

pub async fn login(client_id: &str) -> anyhow::Result<SpotifyAuth> {
    anyhow::ensure!(
        !client_id.trim().is_empty(),
        "Spotify client ID is required"
    );
    let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8898")
        .await
        .context("Could not bind Spotify callback port 8898")?;
    let mut url = url::Url::parse("https://accounts.spotify.com/authorize")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT_URI),
        ("code_challenge_method", "S256"),
        ("code_challenge", &challenge),
        ("state", &state),
        (
            "scope",
            "playlist-read-private playlist-read-collaborative user-library-read",
        ),
    ]);
    let browser_url = url.to_string();
    tokio::task::spawn_blocking(move || open::that(browser_url)).await??;
    let code = tokio::time::timeout(Duration::from_secs(180), callback(listener, &state))
        .await
        .context("Spotify login timed out")??;
    token(
        &[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", &verifier),
        ],
        "",
    )
    .await
}

pub async fn refresh(client_id: &str, refresh_token: &str) -> anyhow::Result<SpotifyAuth> {
    token(
        &[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ],
        refresh_token,
    )
    .await
}

async fn pages(auth: &SpotifyAuth, first: &str) -> anyhow::Result<Vec<Value>> {
    let mut next = Some(first.to_string());
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    while let Some(url) = next.take() {
        anyhow::ensure!(seen.insert(url.clone()), "Spotify pagination loop");
        let parsed = url::Url::parse(&url)?;
        anyhow::ensure!(
            parsed.scheme() == "https" && parsed.host_str() == Some("api.spotify.com"),
            "Invalid Spotify pagination URL"
        );
        let response = super::client()
            .get(parsed)
            .bearer_auth(&auth.access_token)
            .send()
            .await?;
        let response = response.error_for_status()?;
        let page: Value = response.json().await?;
        items.extend(
            page["items"]
                .as_array()
                .context("Spotify response has no items")?
                .iter()
                .cloned(),
        );
        next = page["next"].as_str().map(str::to_string);
    }
    Ok(items)
}

pub async fn my_playlists(auth: &SpotifyAuth) -> anyhow::Result<Vec<SpotifyPlaylist>> {
    Ok(
        pages(auth, "https://api.spotify.com/v1/me/playlists?limit=50")
            .await?
            .iter()
            .filter_map(|p| {
                Some(SpotifyPlaylist {
                    id: p["id"].as_str()?.to_string(),
                    name: p["name"].as_str().unwrap_or("Untitled").to_string(),
                    track_count: p
                        .pointer("/items/total")
                        .or_else(|| p.pointer("/tracks/total"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize,
                    image_url: p
                        .pointer("/images/0/url")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
            })
            .collect(),
    )
}

fn api_track(item: &Value) -> Option<SpotifyTrack> {
    let track = item
        .get("item")
        .filter(|v| !v.is_null())
        .or_else(|| item.get("track"))?;
    if track["type"].as_str().is_some_and(|s| s != "track") {
        return None;
    }
    Some(SpotifyTrack {
        title: track["name"].as_str()?.to_string(),
        artist: track["artists"]
            .as_array()?
            .iter()
            .filter_map(|v| v["name"].as_str())
            .collect::<Vec<_>>()
            .join(", "),
        album: track
            .pointer("/album/name")
            .and_then(Value::as_str)
            .map(str::to_string),
        duration_secs: track["duration_ms"].as_f64().map(|v| v / 1000.0),
        artwork_url: track
            .pointer("/album/images/0/url")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub async fn playlist_tracks(
    auth: &SpotifyAuth,
    playlist_id: &str,
) -> anyhow::Result<Vec<SpotifyTrack>> {
    anyhow::ensure!(
        !playlist_id.is_empty() && playlist_id.bytes().all(|b| b.is_ascii_alphanumeric()),
        "Invalid playlist ID"
    );
    let legacy = format!("https://api.spotify.com/v1/playlists/{playlist_id}/tracks?limit=50");
    let items = match pages(auth, &legacy).await {
        Ok(items) => items,
        Err(error)
            if error
                .downcast_ref::<reqwest::Error>()
                .and_then(reqwest::Error::status)
                .is_some_and(|s| matches!(s.as_u16(), 403 | 404 | 410)) =>
        {
            // The Oct 2026 API uses /items and item; access is owner/collaborator only.
            pages(auth, &format!("https://api.spotify.com/v1/playlists/{playlist_id}/items?limit=50")).await
                .context("Spotify only permits playlist items for owners/collaborators; use public embed import for other playlists")?
        }
        Err(error) => return Err(error),
    };
    Ok(items.iter().filter_map(api_track).collect())
}

pub async fn liked_tracks(auth: &SpotifyAuth) -> anyhow::Result<Vec<SpotifyTrack>> {
    Ok(pages(auth, "https://api.spotify.com/v1/me/tracks?limit=50")
        .await?
        .iter()
        .filter_map(api_track)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_public_urls_and_uris() {
        assert_eq!(
            public_id("spotify:playlist:abc123").unwrap(),
            ("playlist".into(), "abc123".into())
        );
        assert_eq!(
            public_id("https://open.spotify.com/intl-de/album/abc123?si=test").unwrap(),
            ("album".into(), "abc123".into())
        );
        assert!(public_id("https://example.com/playlist/abc").is_err());
        assert!(public_id("spotify:playlist:bad/id").is_err());
    }
    #[test]
    fn reads_current_and_legacy_track_fields() {
        let track = serde_json::json!({"name":"Song", "artists":[{"name":"Artist"}], "duration_ms":123000, "type":"track"});
        assert_eq!(
            api_track(&serde_json::json!({"item":track}))
                .unwrap()
                .duration_secs,
            Some(123.0)
        );
        assert_eq!(
            api_track(&serde_json::json!({"track":track}))
                .unwrap()
                .title,
            "Song"
        );
        assert!(api_track(&serde_json::json!({"track":null})).is_none());
    }
}
