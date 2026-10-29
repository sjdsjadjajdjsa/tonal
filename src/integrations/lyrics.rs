use crate::model::Track;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Lyrics {
    pub synced: Vec<(f64, String)>,
    pub plain: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    expires_at: i64,
    lyrics: Option<Lyrics>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    #[serde(default)]
    track_name: String,
    #[serde(default)]
    artist_name: String,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    instrumental: bool,
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
}

pub fn parse_lrc(text: &str) -> Vec<(f64, String)> {
    let mut result = Vec::new();
    let mut offset = 0.0;
    for line in text.lines() {
        let mut rest = line.trim();
        if let Some(ms) = rest
            .strip_prefix("[offset:")
            .and_then(|s| s.strip_suffix(']'))
            .and_then(|s| s.parse::<f64>().ok())
        {
            if ms.is_finite() {
                offset = ms / 1000.0;
            }
            continue;
        }
        let mut times = Vec::new();
        while let Some(tag) = rest.strip_prefix('[') {
            let Some((stamp, tail)) = tag.split_once(']') else {
                break;
            };
            rest = tail;
            let Some((minutes, seconds)) = stamp.split_once(':') else {
                continue;
            };
            if let (Ok(m), Ok(s)) = (minutes.parse::<u32>(), seconds.parse::<f64>())
                && s.is_finite()
                && (0.0..60.0).contains(&s)
            {
                times.push(m as f64 * 60.0 + s);
            }
        }
        for time in times {
            result.push((time, rest.trim().to_string()));
        }
    }
    for (time, _) in &mut result {
        *time = (*time + offset).max(0.0);
    }
    result.sort_by(|a, b| a.0.total_cmp(&b.0));
    result
}

fn lyrics(entry: Entry) -> Option<Lyrics> {
    if entry.instrumental {
        return None;
    }
    let synced = entry
        .synced_lyrics
        .as_deref()
        .map(parse_lrc)
        .unwrap_or_default();
    let plain = entry.plain_lyrics.filter(|s| !s.trim().is_empty());
    if synced.is_empty() && plain.is_none() {
        None
    } else {
        Some(Lyrics { synced, plain })
    }
}

pub async fn fetch(track: &Track) -> anyhow::Result<Option<Lyrics>> {
    let key = serde_json::to_vec(&(
        &track.title,
        &track.artist,
        &track.album,
        track.duration_secs,
    ))?;
    let digest = Sha256::digest(key);
    let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let dir = crate::model::paths::cache_dir().join("lyrics");
    let path = dir.join(format!("{hash}.json"));
    if let Ok(bytes) = tokio::fs::read(&path).await
        && let Ok(cached) = serde_json::from_slice::<Cached>(&bytes)
        && cached.expires_at > super::now()
    {
        return Ok(cached.lyrics);
    }
    let mut url = url::Url::parse("https://lrclib.net/api/get")?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("track_name", &track.title)
            .append_pair("artist_name", &track.artist);
        if let Some(album) = &track.album {
            query.append_pair("album_name", album);
        }
        if let Some(duration) = track.duration_secs.filter(|v| v.is_finite() && *v > 0.0) {
            query.append_pair("duration", &duration.round().to_string());
        }
    }
    let response = super::client().get(url).send().await?;
    let mut found = None;
    if response.status().is_success() {
        found = lyrics(response.json::<Entry>().await?);
    } else if response.status() != reqwest::StatusCode::NOT_FOUND {
        response.error_for_status()?;
    }
    if found.is_none() {
        let mut url = url::Url::parse("https://lrclib.net/api/search")?;
        url.query_pairs_mut()
            .append_pair("q", &format!("{} {}", track.artist, track.title));
        let mut entries: Vec<Entry> = super::client()
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let score = |e: &Entry| {
            let mut score = 0.0;
            if e.track_name.eq_ignore_ascii_case(track.title.trim()) {
                score += 100.0;
            }
            if e.artist_name.eq_ignore_ascii_case(track.artist.trim()) {
                score += 100.0;
            }
            if let Some(duration) = track.duration_secs {
                score -= (duration - e.duration).abs().min(300.0);
            }
            if e.synced_lyrics.is_some() {
                score += 1.0;
            }
            score
        };
        entries.sort_by(|a, b| score(b).total_cmp(&score(a)));
        found = entries.into_iter().find_map(lyrics);
    }
    let clean = clean_title(&track.title);
    if found.as_ref().is_none_or(|l| l.synced.is_empty()) && clean != track.title.trim() {
        let mut url = url::Url::parse("https://lrclib.net/api/search")?;
        url.query_pairs_mut()
            .append_pair("track_name", &clean)
            .append_pair("artist_name", &track.artist);
        let entries: Vec<Entry> = super::client()
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let best = entries
            .into_iter()
            .filter(|e| e.synced_lyrics.is_some())
            .min_by(|a, b| {
                let gap = |e: &Entry| track.duration_secs.map_or(0.0, |d| (d - e.duration).abs());
                gap(a).total_cmp(&gap(b))
            })
            .and_then(lyrics);
        if best.is_some() {
            found = best;
        }
    }
    let cached = Cached {
        expires_at: super::now() + 7 * 24 * 60 * 60,
        lyrics: found.clone(),
    };
    if tokio::fs::create_dir_all(&dir).await.is_ok() {
        let tmp = dir.join(format!("{hash}-{:016x}.tmp", rand::random::<u64>()));
        if tokio::fs::write(&tmp, serde_json::to_vec(&cached)?)
            .await
            .is_ok()
            && tokio::fs::rename(&tmp, &path).await.is_err()
        {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
    }
    Ok(found)
}

fn clean_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut depth = 0usize;
    for ch in title.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let lower = out.to_ascii_lowercase();
    let cut = [" - ", " feat.", " ft.", " featuring "]
        .iter()
        .filter_map(|marker| lower.find(marker))
        .min()
        .unwrap_or(out.len());
    out[..cut].split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn cleans_titles_for_search() {
        assert_eq!(
            super::clean_title("One More Time (Radio Edit)"),
            "One More Time"
        );
        assert_eq!(super::clean_title("Ribs - Remastered 2023"), "Ribs");
        assert_eq!(
            super::clean_title("Get Lucky [feat. Pharrell] ft. Nile"),
            "Get Lucky"
        );
        assert_eq!(super::clean_title("HUMBLE."), "HUMBLE.");
    }
    use super::*;
    #[test]
    fn parses_multiple_timestamps_and_sorts() {
        let parsed = parse_lrc(
            "[ar:Daft Punk]\n[01:02.50][00:02.1] One more time\n[00:01.123]Hello\n[00:03]Again\n[bad]Ignored\n[00:99.0]Invalid\n[00:NaN]Invalid",
        );
        assert_eq!(
            parsed,
            vec![
                (1.123, "Hello".into()),
                (2.1, "One more time".into()),
                (3.0, "Again".into()),
                (62.5, "One more time".into())
            ]
        );
    }
    #[test]
    fn supports_offset_and_blank_lines() {
        assert_eq!(
            parse_lrc("[00:01.00]Hi\n[00:02.00]\n[offset:-500]"),
            vec![(0.5, "Hi".into()), (1.5, "".into())]
        );
        assert!(parse_lrc("plain lyrics\n[ti:Title]").is_empty());
    }
}
