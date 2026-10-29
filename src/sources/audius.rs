use crate::model::{SourceKind, Track};
use anyhow::{Context, Result};
use serde_json::Value;

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<Track>> {
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
        .context("Audius returned no discovery nodes")?;
    let mut url = url::Url::parse(&format!("{host}/v1/tracks/search"))?;
    url.query_pairs_mut()
        .append_pair("query", query)
        .append_pair("app_name", "tonal");
    let data: Value = super::client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(data["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry["id"].as_str()?.to_owned();
            let artist = entry["user"]["name"]
                .as_str()
                .or_else(|| entry["user"]["handle"].as_str())
                .unwrap_or("Unknown artist")
                .to_owned();
            Some(Track {
                id: Track::make_id(SourceKind::Audius, &id),
                source: SourceKind::Audius,
                source_id: id.clone(),
                title: entry["title"].as_str()?.to_owned(),
                artist,
                album: entry["album_name"].as_str().map(str::to_owned),
                duration_secs: entry["duration"].as_f64(),
                artwork_url: entry["artwork"]["480x480"].as_str().map(str::to_owned),
                play_url: format!("{host}/v1/tracks/{id}/stream?app_name=tonal"),
            })
        })
        .take(limit)
        .collect())
}
