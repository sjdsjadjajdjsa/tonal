use crate::model::{ArtistRef, SourceKind, Track};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;

const VERSION: &str = "1.20261025.12.00";
// The WEB_REMIX search filter for the Songs tab (not the general/top-results page).
const SONGS: &str = "EgWKAQIIAWoKEAMQBBAJEAoQBQ%3D%3D";

pub(super) fn context() -> Value {
    json!({"client": {"clientName": "WEB_REMIX", "clientVersion": VERSION, "hl": "en", "gl": "US"}})
}

pub(super) async fn post(endpoint: &str, body: Value) -> Result<Value> {
    Ok(super::client()
        .post(format!(
            "https://music.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false"
        ))
        .header("Origin", "https://music.youtube.com")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub(super) fn runs(value: &Value) -> String {
    value
        .get("runs")
        .and_then(Value::as_array)
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .or_else(|| {
            value
                .get("simpleText")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn duration(text: &str) -> Option<f64> {
    let parts: Vec<_> = text
        .split(':')
        .map(|part| part.trim().parse::<u64>())
        .collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut secs = 0_u64;
    for part in parts {
        secs = secs.checked_mul(60)?.checked_add(part.ok()?)?;
    }
    Some(secs as f64)
}

pub(super) fn thumbnail(value: &Value) -> Option<String> {
    let thumbnails = value.get("thumbnails")?.as_array()?;
    let chosen = thumbnails
        .iter()
        .filter(|v| {
            v["width"]
                .as_u64()
                .is_some_and(|width| (226..=544).contains(&width))
        })
        .max_by_key(|v| v["width"].as_u64().unwrap_or(0))
        .or_else(|| thumbnails.last())?;
    let mut url = chosen["url"].as_str()?.to_owned();
    if let Some(width) = chosen["width"].as_u64().filter(|width| *width < 226) {
        url = url.replace(&format!("=w{width}-h{width}"), "=w544-h544");
    }
    Some(url)
}

pub(super) fn collect<'a>(node: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(map) => {
            for (name, child) in map {
                if name == key {
                    out.push(child);
                }
                collect(child, key, out);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect(child, key, out);
            }
        }
        _ => (),
    }
}

fn page_type(run: &Value) -> &str {
    run["navigationEndpoint"]["browseEndpoint"]["browseEndpointContextSupportedConfigs"]
        ["browseEndpointContextMusicConfig"]["pageType"].as_str().unwrap_or("")
}

pub(super) fn browse_id(value: &Value) -> Option<&str> {
    value["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()
}

fn album_run(run: &Value) -> bool {
    browse_id(run).is_some_and(|id| id.starts_with("MPRE"))
}

/// Artist credits are individual runs; punctuation and later metadata groups
/// must not turn into anonymous artist links.
pub(super) fn artists(text: &Value) -> Vec<ArtistRef> {
    let mut result = Vec::new();
    for run in text["runs"].as_array().into_iter().flatten() {
        let name = run["text"].as_str().unwrap_or("").trim();
        if name.contains('•') {
            if !result.is_empty() {
                break;
            }
            continue;
        }
        if name.is_empty()
            || matches!(
                name,
                "&" | ","
                    | "and"
                    | "Song"
                    | "Video"
                    | "Podcast"
                    | "Album"
                    | "Single"
                    | "EP"
                    | "Artist"
            )
            || duration(name).is_some()
            || name.ends_with(" views")
            || name.ends_with(" view")
            || name.ends_with(" plays")
            || name.ends_with(" subscribers")
            || (name.len() == 4 && name.parse::<u16>().is_ok())
        {
            continue;
        }
        let id = browse_id(run);
        if id.is_some_and(|id| !id.starts_with("UC")) {
            continue;
        }
        if page_type(run).is_empty() || page_type(run) == "MUSIC_PAGE_TYPE_ARTIST" {
            let credit = ArtistRef {
                source: SourceKind::YouTube,
                name: name.to_owned(),
                id: id.map(str::to_owned),
            };
            if !result.contains(&credit) {
                result.push(credit);
            }
        }
    }
    result
}

fn clean_artist(text: &str) -> String {
    text.split('•')
        .map(str::trim)
        .find(|part| {
            !part.is_empty()
                && !matches!(*part, "Song" | "Video" | "Podcast")
                && !part.ends_with(" views")
                && !part.ends_with(" view")
        })
        .unwrap_or("Unknown artist")
        .to_owned()
}

pub(super) fn song(item: &Value) -> Option<Track> {
    let columns = item["flexColumns"].as_array()?;
    let title_text = &columns.first()?["musicResponsiveListItemFlexColumnRenderer"]["text"];
    let title = runs(title_text);
    let video_id = item["playlistItemData"]["videoId"].as_str()
        .or_else(|| title_text["runs"].as_array()?.first()?["navigationEndpoint"]["watchEndpoint"]["videoId"].as_str())
        .or_else(|| item["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]["musicPlayButtonRenderer"]["playNavigationEndpoint"]["watchEndpoint"]["videoId"].as_str())?;
    if title.is_empty() {
        return None;
    }
    let artist_refs = columns
        .get(1)
        .map(|column| artists(&column["musicResponsiveListItemFlexColumnRenderer"]["text"]))
        .unwrap_or_default();
    let artist = artist_refs
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut album = None;
    let mut album_id = None;
    let mut length = None;
    for column in columns.iter().skip(1) {
        for field in column["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let name = field["text"].as_str().unwrap_or("").trim();
            if album_run(field) {
                album = Some(name.to_owned());
                album_id = browse_id(field).map(str::to_owned);
            } else if let Some(secs) = duration(name) {
                length = Some(secs);
            }
        }
    }
    if length.is_none() {
        length = item["fixedColumns"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|column| {
                duration(&runs(
                    &column["musicResponsiveListItemFixedColumnRenderer"]["text"],
                ))
            });
    }
    Some(Track {
        id: Track::make_id(SourceKind::YouTube, video_id),
        source: SourceKind::YouTube,
        source_id: video_id.to_owned(),
        title,
        artist,
        album,
        album_id,
        artist_refs,
        duration_secs: length,
        artwork_url: thumbnail(&item["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]),
        play_url: format!("https://music.youtube.com/watch?v={video_id}"),
    })
}

fn playlist_song(item: &Value) -> Option<Track> {
    let id = item["videoId"].as_str()?;
    let title = runs(&item["title"]);
    if title.is_empty() {
        return None;
    }
    let artist = clean_artist(&runs(&item["shortBylineText"]));
    let mut artist_refs = artists(&item["longBylineText"]);
    for credit in artists(&item["shortBylineText"]) {
        if !artist_refs.iter().any(|known| known.name == credit.name) {
            artist_refs.push(credit);
        }
    }
    if artist_refs.is_empty() {
        artist_refs.push(ArtistRef {
            source: SourceKind::YouTube,
            name: artist.clone(),
            id: None,
        });
    }
    let album_id = item["longBylineText"]["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|run| album_run(run))
        .and_then(browse_id)
        .map(str::to_owned);
    let album = item["longBylineText"]["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|run| album_run(run))
        .and_then(|run| run["text"].as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    Some(Track {
        id: Track::make_id(SourceKind::YouTube, id),
        source: SourceKind::YouTube,
        source_id: id.into(),
        title,
        artist,
        album,
        duration_secs: duration(&runs(&item["lengthText"])),
        artwork_url: thumbnail(&item["thumbnail"]),
        play_url: format!("https://music.youtube.com/watch?v={id}"),
        artist_refs,
        album_id,
    })
}

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<Track>> {
    match post(
        "search",
        json!({"context": context(), "query": query, "params": SONGS}),
    )
    .await
    {
        Ok(response) => {
            // Only the Songs shelf, in the order returned by the Songs tab; recursive
            // traversal can accidentally include unrelated cards and continuation data.
            let shelves = response["contents"]["tabbedSearchResultsRenderer"]["tabs"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|tab| {
                    tab["tabRenderer"]["content"]["sectionListRenderer"]["contents"]
                        .as_array()
                        .into_iter()
                        .flatten()
                });
            let mut tracks: Vec<_> = shelves
                .filter_map(|section| section.get("musicShelfRenderer"))
                .filter(|shelf| runs(&shelf["title"]) == "Songs")
                .flat_map(|shelf| shelf["contents"].as_array().into_iter().flatten())
                .filter_map(|item| song(&item["musicResponsiveListItemRenderer"]))
                .collect();
            // Anonymous InnerTube occasionally puts a title-only remix ahead of the
            // searched artist. Promote exact artist searches without reordering songs
            // by that artist (or changing mixed artist/title queries).
            let query_artist = query.trim().to_lowercase();
            if tracks
                .iter()
                .any(|track| track.artist.to_lowercase() == query_artist)
            {
                tracks.sort_by_key(|track| track.artist.to_lowercase() != query_artist);
            }
            tracks.truncate(limit);
            if !tracks.is_empty() {
                return Ok(tracks);
            }
        }
        Err(error) => log::warn!("YouTube Music search failed: {error}"),
    }
    fallback(query, limit).await
}

async fn fallback(query: &str, limit: usize) -> Result<Vec<Track>> {
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        super::ytdlp()
            .arg("--flat-playlist")
            .arg("-J")
            .arg(format!("ytsearch{}:{query}", limit.min(100)))
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await??;
    if !output.status.success() {
        anyhow::bail!("yt-dlp search: {}", String::from_utf8_lossy(&output.stderr));
    }
    let data: Value = serde_json::from_slice(&output.stdout).context("yt-dlp search JSON")?;
    Ok(data["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry["id"].as_str()?.to_owned();
            let raw = entry["title"].as_str()?.to_owned();
            let mut title = raw;
            for suffix in [
                "(Official Video)",
                "[Official Video]",
                "(Official Audio)",
                "[Official Audio]",
                "(Lyrics)",
                "[Lyrics]",
                "(Lyric Video)",
                "[Lyric Video]",
            ] {
                title = title.replace(suffix, "");
            }
            let mut artist = entry["uploader"]
                .as_str()
                .unwrap_or("Unknown artist")
                .to_owned();
            if let Some((left, right)) = title.split_once(" - ") {
                artist = left.trim().to_owned();
                title = right.trim().to_owned();
            }
            Some(Track {
                id: Track::make_id(SourceKind::YouTube, &id),
                source: SourceKind::YouTube,
                source_id: id.clone(),
                title: title.trim().to_owned(),
                artist_refs: vec![ArtistRef {
                    source: SourceKind::YouTube,
                    name: artist.clone(),
                    id: entry["channel_id"]
                        .as_str()
                        .filter(|id| id.starts_with("UC"))
                        .map(str::to_owned),
                }],
                artist,
                album: entry["album"].as_str().map(str::to_owned),
                duration_secs: entry["duration"].as_f64(),
                artwork_url: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")),
                play_url: format!("https://music.youtube.com/watch?v={id}"),
                album_id: entry["album_id"]
                    .as_str()
                    .filter(|id| id.starts_with("MPRE"))
                    .map(str::to_owned),
            })
        })
        .take(limit)
        .collect())
}

pub(super) async fn radio(seed: &Track, limit: usize) -> Result<Vec<Track>> {
    let response = post(
        "next",
        json!({"context": context(), "videoId": seed.source_id,
        "playlistId": format!("RDAMVM{}", seed.source_id)}),
    )
    .await?;
    let mut items = Vec::new();
    collect(&response, "playlistPanelVideoRenderer", &mut items);
    Ok(items
        .into_iter()
        .filter_map(playlist_song)
        .filter(|track| track.source_id != seed.source_id)
        .take(limit)
        .collect())
}
