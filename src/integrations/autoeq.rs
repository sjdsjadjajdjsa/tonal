//! Headphone corrections from AutoEQ (github.com/jaakkopasanen/AutoEq), fetched on demand.
//!
//! The project's `results/INDEX.md` lists every measured headphone as a markdown link to
//! its folder, best source first. Each folder has a `<name> ParametricEQ.txt` with a preamp
//! and up to ten filters. Both are cached; nothing is bundled.

use crate::model::{EqFilter, FilterKind, Headphone, paths::cache_dir};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};

const RESULTS: &str = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results";
/// The index changes slowly; refresh it once a week.
const INDEX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// One headphone model in the index.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    /// Folder under `results/`, percent-encoded as in the index.
    pub path: String,
    /// How many sources measured it: a fair sign of how common it is.
    pub measurements: usize,
}

fn dir() -> PathBuf {
    cache_dir().join("autoeq")
}

/// Every model once: the index lists the preferred measurement of a model first.
pub fn parse_index(markdown: &str) -> Vec<Entry> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut entries: Vec<Entry> = Vec::new();
    for line in markdown.lines() {
        let Some((name, rest)) = line
            .strip_prefix("- [")
            .and_then(|rest| rest.split_once("](./"))
        else {
            continue;
        };
        let Some((path, _)) = rest.rsplit_once(") by ") else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        match seen.entry(name.to_lowercase()) {
            std::collections::hash_map::Entry::Occupied(at) => {
                entries[*at.get()].measurements += 1;
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(entries.len());
                entries.push(Entry {
                    name: name.to_string(),
                    path: path.replace('#', "%23").replace('?', "%3F"),
                    measurements: 1,
                });
            }
        }
    }
    entries
}

/// Preamp and filters of a `ParametricEQ.txt`. Disabled and unknown filters are skipped.
pub fn parse_parametric(text: &str) -> Option<(f64, Vec<EqFilter>)> {
    let mut preamp = None;
    let mut filters = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.first() == Some(&"Preamp:") {
            preamp = words.get(1).and_then(|v| v.parse::<f64>().ok());
            continue;
        }
        if words.first() != Some(&"Filter") || !words.contains(&"ON") {
            continue;
        }
        let value = |key: &str| {
            let at = words.iter().position(|w| *w == key)?;
            words.get(at + 1)?.parse::<f64>().ok()
        };
        let kind = if words.contains(&"PK") {
            FilterKind::Peak
        } else if words.contains(&"LSC") || words.contains(&"LS") {
            FilterKind::LowShelf
        } else if words.contains(&"HSC") || words.contains(&"HS") {
            FilterKind::HighShelf
        } else {
            continue;
        };
        let (Some(freq), Some(gain)) = (value("Fc"), value("Gain")) else {
            continue;
        };
        let q = value("Q").unwrap_or(0.71);
        if freq > 0.0 && q > 0.0 && gain.is_finite() {
            filters.push(EqFilter {
                kind,
                freq,
                gain,
                q,
            });
        }
    }
    (preamp.is_some() || !filters.is_empty()).then(|| (preamp.unwrap_or(0.0).min(0.0), filters))
}

/// Models whose name holds every word of the query, closest matches first.
pub fn search<'a>(entries: &'a [Entry], query: &str, limit: usize) -> Vec<&'a Entry> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<_> = entries
        .iter()
        .filter_map(|e| {
            let name = e.name.to_lowercase();
            // "hd600" finds "HD 600".
            let compact = name.replace(' ', "");
            if !words
                .iter()
                .all(|w| name.contains(w) || compact.contains(w))
            {
                return None;
            }
            // Whole-word hits first, then common models, then the plain model before
            // its variants.
            let starts = name.split_whitespace().any(|n| n.starts_with(words[0]));
            let rank = (!starts, std::cmp::Reverse(e.measurements), name.len());
            Some((rank, e))
        })
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().take(limit).map(|(_, e)| e).collect()
}

async fn fetch_text(url: &str) -> anyhow::Result<String> {
    Ok(super::client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

/// The list of models, from the cache when it is fresh, else from GitHub.
pub async fn index() -> anyhow::Result<Vec<Entry>> {
    let path = dir().join("INDEX.md");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < INDEX_AGE);
    let cached = tokio::fs::read_to_string(&path).await.ok();
    if fresh && let Some(text) = &cached {
        return Ok(parse_index(text));
    }
    match fetch_text(&format!("{RESULTS}/INDEX.md")).await {
        Ok(text) => {
            let _ = tokio::fs::create_dir_all(dir()).await;
            let _ = tokio::fs::write(&path, &text).await;
            Ok(parse_index(&text))
        }
        // Offline: a stale list is better than none.
        Err(e) => cached.map(|t| parse_index(&t)).ok_or(e),
    }
}

/// The correction for one model, cached by its folder.
pub async fn correction(entry: &Entry) -> anyhow::Result<Headphone> {
    let digest = Sha256::digest(entry.path.as_bytes());
    let hash: String = digest.iter().take(12).map(|b| format!("{b:02x}")).collect();
    let path = dir().join(format!("{hash}.txt"));
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(_) => {
            let folder = entry.path.rsplit('/').next().unwrap_or_default();
            let url = format!("{RESULTS}/{}/{folder}%20ParametricEQ.txt", entry.path);
            let text = fetch_text(&url).await?;
            let _ = tokio::fs::create_dir_all(dir()).await;
            let _ = tokio::fs::write(&path, &text).await;
            text
        }
    };
    let (preamp, filters) = parse_parametric(&text)
        .ok_or_else(|| anyhow::anyhow!("No parametric EQ for {}", entry.name))?;
    Ok(Headphone {
        name: entry.name.clone(),
        preamp,
        filters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Preamp: -6.3 dB\r\nFilter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70\r\nFilter 2: ON PK Fc 125 Hz Gain -2.7 dB Q 0.55\r\nFilter 3: OFF PK Fc 900 Hz Gain 9.0 dB Q 1.00\r\nFilter 4: ON HSC Fc 10000 Hz Gain -3.1 dB Q 0.70\r\n";

    #[test]
    fn parses_a_parametric_eq() {
        let (preamp, filters) = parse_parametric(SAMPLE).unwrap();
        assert_eq!(preamp, -6.3);
        assert_eq!(
            filters,
            [
                EqFilter {
                    kind: FilterKind::LowShelf,
                    freq: 105.0,
                    gain: 6.5,
                    q: 0.7
                },
                EqFilter {
                    kind: FilterKind::Peak,
                    freq: 125.0,
                    gain: -2.7,
                    q: 0.55
                },
                EqFilter {
                    kind: FilterKind::HighShelf,
                    freq: 10000.0,
                    gain: -3.1,
                    q: 0.7
                },
            ]
        );
        assert!(parse_parametric("404: Not Found").is_none());
    }

    #[test]
    fn index_keeps_the_first_measurement_and_odd_names() {
        let index = "# Index\n\n- [Sennheiser HD 600](./oratory1990/over-ear/Sennheiser%20HD%20600) by oratory1990\n- [Sennheiser HD 600](./crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20HD%20600) by crinacle on GRAS 43AG-7\n- [1MORE Aero (ANC Off)](./HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)) by HypetheSonics on GRAS RA0045\n";
        let entries = parse_index(index);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].measurements, 2);
        assert_eq!(
            entries[0].path,
            "oratory1990/over-ear/Sennheiser%20HD%20600"
        );
        assert_eq!(
            entries[1].path,
            "HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)"
        );
        for query in ["hd 600", "HD600", "senn 600"] {
            assert_eq!(search(&entries, query, 5)[0].name, "Sennheiser HD 600");
        }
        assert!(search(&entries, "aero", 5)[0].name.starts_with("1MORE"));
        assert!(search(&entries, "", 5).is_empty());
    }
}
