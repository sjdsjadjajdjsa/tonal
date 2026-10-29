use crate::model::{ArtistPage, ArtistRef, SourceKind, Track};
use anyhow::{Context, Result};
use serde_json::Value;

async fn host() -> Result<String> {
    static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(host) = HOST.get() {
        return Ok(host.clone());
    }
    let hosts: Value = super::client()
        .get("https://api.audius.co")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let host = hosts["data"]
        .as_array()
        .and_then(|v| v.iter().filter_map(Value::as_str).next())
        .context("Audius returned no discovery nodes")?
        .trim_end_matches('/')
        .to_owned();
    let _ = HOST.set(host.clone());
    Ok(host)
}

async fn get(host: &str, path: &str, query: &[(&str, &str)]) -> Result<Value> {
    let mut url = url::Url::parse(&format!("{host}/v1/{path}"))?;
    url.query_pairs_mut()
        .append_pair("app_name", "tonal")
        .extend_pairs(query.iter().copied());
    Ok(super::client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

fn user_ref(user: &Value) -> ArtistRef {
    ArtistRef {
        source: SourceKind::Audius,
        name: user["name"]
            .as_str()
            .or_else(|| user["handle"].as_str())
            .unwrap_or("Unknown artist")
            .to_owned(),
        id: user["id"].as_str().map(str::to_owned),
    }
}

fn track(entry: &Value, host: &str) -> Option<Track> {
    let id = entry["id"].as_str()?.to_owned();
    let artist = user_ref(&entry["user"]);
    Some(Track {
        id: Track::make_id(SourceKind::Audius, &id),
        source: SourceKind::Audius,
        source_id: id.clone(),
        title: entry["title"].as_str()?.to_owned(),
        artist: artist.name.clone(),
        artist_refs: vec![artist],
        album: entry["album_name"].as_str().map(str::to_owned),
        album_id: entry["album_id"].as_str().map(str::to_owned),
        duration_secs: entry["duration"].as_f64(),
        artwork_url: entry["artwork"]["480x480"].as_str().map(str::to_owned),
        play_url: format!("{host}/v1/tracks/{id}/stream?app_name=tonal"),
    })
}

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<Track>> {
    let host = host().await?;
    let data = get(
        &host,
        "tracks/search",
        &[("query", query), ("limit", &limit.min(100).to_string())],
    )
    .await?;
    Ok(data["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| track(entry, &host))
        .take(limit)
        .collect())
}

pub(super) async fn find_artist(name: &str) -> Result<Option<ArtistRef>> {
    let host = host().await?;
    let data = get(&host, "users/search", &[("query", name), ("limit", "20")]).await?;
    Ok(data["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(user_ref)
        .filter(|r| r.id.is_some())
        .enumerate()
        .max_by_key(|(index, r)| (super::name_score(&r.name, name), std::cmp::Reverse(*index)))
        .map(|(_, r)| r))
}

pub(super) async fn artist(r: &ArtistRef) -> Result<ArtistPage> {
    let host = host().await?;
    let id = r.id.as_deref().context("Audius artist has no user id")?;
    anyhow::ensure!(
        !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')),
        "Invalid Audius user id"
    );
    let user_path = format!("users/{id}");
    let tracks_path = format!("users/{id}/tracks");
    let (user, tracks) = tokio::try_join!(
        get(&host, &user_path, &[]),
        get(&host, &tracks_path, &[("limit", "50")])
    )?;
    anyhow::ensure!(user["data"].is_object(), "Audius user not found");
    let artist = user_ref(&user["data"]);
    let top = tracks["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let mut song = track(entry, &host)?;
            if song.artist_refs.iter().all(|credit| credit.id.is_none()) {
                song.artist = artist.name.clone();
                song.artist_refs = vec![artist.clone()];
            }
            Some(song)
        })
        .take(50)
        .collect();
    Ok(ArtistPage {
        artist,
        image_url: user["data"]["profile_picture"]["480x480"]
            .as_str()
            .map(str::to_owned),
        top,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tracks_keep_audius_user_ids() {
        let song = track(
            &json!({"id": "song", "title": "Title", "user": {"id": "user", "name": "Artist"}}),
            "https://example.com",
        )
        .expect("track");
        assert_eq!(song.artist_refs[0].id.as_deref(), Some("user"));
        assert_eq!(song.artist, "Artist");
        assert!(
            song.play_url
                .ends_with("/v1/tracks/song/stream?app_name=tonal")
        );
    }
}
