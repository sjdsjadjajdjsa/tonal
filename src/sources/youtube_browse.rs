//! Defensive parsers for anonymous YouTube Music browse/search responses.
use crate::model::{AlbumPage, AlbumRef, ArtistCard, ArtistPage, ArtistRef, SourceKind};
use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::youtube::{artists, browse_id, collect, context, post, runs, song, thumbnail};

const ARTISTS: &str = "EgWKAQIgAWoKEAMQBBAJEAoQBQ%3D%3D";
const ALBUMS: &str = "EgWKAQIYAWoKEAMQBBAJEAoQBQ%3D%3D";

fn nodes<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut result = Vec::new();
    collect(value, key, &mut result);
    result
}

fn image(value: &Value) -> Option<String> {
    nodes(value, "thumbnails")
        .into_iter()
        .find_map(|array| thumbnail(&json!({"thumbnails": array})))
}

fn year(text: &Value) -> Option<String> {
    runs(text)
        .split(|c: char| !c.is_ascii_digit())
        .find(|part| {
            part.len() == 4
                && part
                    .parse::<u16>()
                    .is_ok_and(|year| (1800..=2200).contains(&year))
        })
        .map(str::to_owned)
}

fn title(item: &Value) -> String {
    let name = runs(&item["title"]);
    if !name.is_empty() {
        return name;
    }
    item["flexColumns"]
        .as_array()
        .and_then(|cols| cols.first())
        .map(|col| runs(&col["musicResponsiveListItemFlexColumnRenderer"]["text"]))
        .unwrap_or_default()
}

fn subtitle(item: &Value) -> &Value {
    item.get("subtitle").unwrap_or_else(|| {
        &item["flexColumns"][1]["musicResponsiveListItemFlexColumnRenderer"]["text"]
    })
}

fn item_id(item: &Value, prefix: &str) -> Option<String> {
    browse_id(item)
        .filter(|id| id.starts_with(prefix))
        .map(str::to_owned)
        .or_else(|| {
            nodes(&item["title"], "browseId")
                .into_iter()
                .filter_map(Value::as_str)
                .find(|id| id.starts_with(prefix))
                .map(str::to_owned)
        })
}

fn album_ref(item: &Value, fallback_artist: &str) -> Option<AlbumRef> {
    let id = item_id(item, "MPRE")?;
    let title = title(item);
    if title.is_empty() {
        return None;
    }
    let credits = artists(subtitle(item));
    Some(AlbumRef {
        source: SourceKind::YouTube,
        title,
        id: Some(id),
        artist: if credits.is_empty() {
            fallback_artist.to_owned()
        } else {
            credits
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        },
        year: year(subtitle(item)),
        artwork_url: image(
            item.get("thumbnailRenderer")
                .or_else(|| item.get("thumbnail"))
                .unwrap_or(&Value::Null),
        ),
    })
}

async fn browse(id: &str) -> Result<Value> {
    let response = post("browse", json!({"context": context(), "browseId": id})).await?;
    if let Some(error) = response.get("error") {
        anyhow::bail!("YouTube Music browse: {error}");
    }
    // Metadata-only diagnostics; set TONAL_SOURCE_DUMP=/tmp/tonal-browse.
    if let Some(dir) = std::env::var_os("TONAL_SOURCE_DUMP") {
        let path = std::path::PathBuf::from(dir);
        let safe_id: String = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .collect();
        if let Err(error) = std::fs::create_dir_all(&path).and_then(|()| {
            std::fs::write(path.join(format!("{safe_id}.json")), response.to_string())
        }) {
            log::warn!("YouTube metadata dump: {error}");
        }
    }
    Ok(response)
}

fn header<'a>(response: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| nodes(response, key).into_iter().next())
}

pub(super) async fn artist(r: &ArtistRef) -> Result<ArtistPage> {
    let response = browse(r.id.as_deref().context("YouTube artist has no browse id")?).await?;
    parse_artist(&response, r)
}

fn parse_artist(response: &Value, r: &ArtistRef) -> Result<ArtistPage> {
    let head = header(
        response,
        &[
            "musicImmersiveHeaderRenderer",
            "musicVisualHeaderRenderer",
            "musicResponsiveHeaderRenderer",
            "musicHeaderRenderer",
        ],
    )
    .context("YouTube artist response has no supported header")?;
    let mut artist = r.clone();
    let name = runs(&head["title"]);
    if !name.is_empty() {
        artist.name = name;
    }
    let mut page = ArtistPage {
        artist,
        image_url: image(&head["thumbnail"]),
        ..Default::default()
    };
    for shelf in nodes(&response["contents"], "musicShelfRenderer") {
        let heading = runs(&shelf["title"]).to_lowercase();
        if heading == "songs" || heading == "top songs" {
            page.top = shelf["contents"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| song(&item["musicResponsiveListItemRenderer"]))
                .take(10)
                .collect();
            for track in &mut page.top {
                if track.artist_refs.is_empty() {
                    track.artist_refs = vec![page.artist.clone()];
                    track.artist = page.artist.name.clone();
                }
            }
            break;
        }
    }
    for shelf in nodes(&response["contents"], "musicCarouselShelfRenderer") {
        let heading =
            runs(&shelf["header"]["musicCarouselShelfBasicHeaderRenderer"]["title"]).to_lowercase();
        for item in shelf["contents"].as_array().into_iter().flatten() {
            let item = &item["musicTwoRowItemRenderer"];
            if heading == "albums" {
                if let Some(album) = album_ref(item, &page.artist.name) {
                    page.albums.push(album);
                }
            } else if heading.contains("single") || heading.contains("eps") {
                if let Some(album) = album_ref(item, &page.artist.name) {
                    page.singles.push(album);
                }
            } else if (heading.contains("fans might also like")
                || heading.contains("similar artist")
                || heading.contains("related artist"))
                && let Some(id) = item_id(item, "UC")
            {
                let name = title(item);
                if !name.is_empty() {
                    page.similar.push(ArtistCard {
                        artist: ArtistRef {
                            source: SourceKind::YouTube,
                            name,
                            id: Some(id),
                        },
                        image_url: image(&item["thumbnailRenderer"]),
                    });
                }
            }
        }
    }
    Ok(page)
}

pub(super) async fn album(r: &AlbumRef) -> Result<AlbumPage> {
    let response = browse(r.id.as_deref().context("YouTube album has no browse id")?).await?;
    parse_album(&response, r)
}

fn parse_album(response: &Value, r: &AlbumRef) -> Result<AlbumPage> {
    let head = header(
        response,
        &["musicResponsiveHeaderRenderer", "musicDetailHeaderRenderer"],
    )
    .context("YouTube album response has no supported header")?;
    let mut album = r.clone();
    let name = runs(&head["title"]);
    if !name.is_empty() {
        album.title = name;
    }
    album.year = year(&head["subtitle"]).or(album.year);
    album.artwork_url = image(&head["thumbnail"]).or(album.artwork_url);
    let mut credits = artists(&head["straplineTextOne"]);
    if credits.is_empty() {
        credits = artists(&head["subtitle"]);
    }
    if credits.is_empty() && !album.artist.is_empty() {
        credits.push(ArtistRef {
            source: SourceKind::YouTube,
            name: album.artist.clone(),
            id: None,
        });
    }
    album.artist = credits
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let kind = runs(&head["subtitle"])
        .split('•')
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    let mut tracks = Vec::new();
    // Scope traversal to the album's track shelf, excluding recommended albums.
    for key in ["musicShelfRenderer", "musicPlaylistShelfRenderer"] {
        for shelf in nodes(&response["contents"], key) {
            for item in shelf["contents"].as_array().into_iter().flatten() {
                if let Some(mut track) = song(&item["musicResponsiveListItemRenderer"]) {
                    track.album = Some(album.title.clone());
                    track.album_id = album.id.clone();
                    track.artwork_url = track.artwork_url.or_else(|| album.artwork_url.clone());
                    if track.artist_refs.is_empty() {
                        track.artist_refs = credits.clone();
                        track.artist = album.artist.clone();
                    }
                    if !tracks
                        .iter()
                        .any(|old: &crate::model::Track| old.id == track.id)
                    {
                        tracks.push(track);
                    }
                }
            }
        }
    }
    anyhow::ensure!(
        !tracks.is_empty(),
        "YouTube album response contains no playable tracks"
    );
    Ok(AlbumPage {
        album,
        kind,
        artists: credits,
        tracks,
    })
}

pub(super) async fn find_artist(name: &str) -> Result<Option<ArtistRef>> {
    let response = post(
        "search",
        json!({"context": context(), "query": name, "params": ARTISTS}),
    )
    .await?;
    Ok(
        nodes(&response["contents"], "musicResponsiveListItemRenderer")
            .into_iter()
            .filter_map(|item| {
                Some(ArtistRef {
                    source: SourceKind::YouTube,
                    name: title(item),
                    id: Some(item_id(item, "UC")?),
                })
            })
            .enumerate()
            .max_by_key(|(index, r)| (super::name_score(&r.name, name), std::cmp::Reverse(*index)))
            .map(|(_, r)| r),
    )
}

pub(super) async fn find_album(title: &str, artist: &str) -> Result<Option<AlbumRef>> {
    let response = post(
        "search",
        json!({"context": context(), "query": format!("{title} {artist}"), "params": ALBUMS}),
    )
    .await?;
    Ok(
        nodes(&response["contents"], "musicResponsiveListItemRenderer")
            .into_iter()
            .filter_map(|item| album_ref(item, ""))
            .enumerate()
            .max_by_key(|(index, r)| {
                (
                    2 * super::name_score(&r.title, title) + super::name_score(&r.artist, artist),
                    std::cmp::Reverse(*index),
                )
            })
            .map(|(_, r)| r),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_tracks_inherit_header_without_losing_order_or_duration() {
        let r = AlbumRef {
            source: SourceKind::YouTube,
            id: Some("MPREtest".into()),
            ..Default::default()
        };
        let response = json!({"contents": {"header": {"musicResponsiveHeaderRenderer": {
            "title": {"runs": [{"text": "Discovery"}]},
            "subtitle": {"runs": [{"text": "Album"}, {"text": " • "}, {"text": "2001"}]},
            "straplineTextOne": {"runs": [{"text": "Daft Punk", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCtest"}}}]},
            "thumbnail": {"thumbnails": [{"url": "cover", "width": 544}]}
        }}, "shelf": {"musicShelfRenderer": {"contents": [
            {"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": "one"}, "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "One More Time"}]}}}], "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"runs": [{"text": "5:21"}]}}}]}},
            {"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": "two"}, "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Aerodynamic"}]}}}]}}
        ]}}}});
        let page = parse_album(&response, &r).expect("album");
        assert_eq!(page.kind, "Album");
        assert_eq!(page.album.year.as_deref(), Some("2001"));
        assert_eq!(page.tracks.len(), 2);
        assert_eq!(page.tracks[0].duration_secs, Some(321.0));
        assert_eq!(page.tracks[0].artist_refs[0].id.as_deref(), Some("UCtest"));
        assert_eq!(page.tracks[0].artwork_url.as_deref(), Some("cover"));
        assert_eq!(page.tracks[1].source_id, "two");
        assert_eq!(page.tracks[1].album_id, r.id);
    }

    #[test]
    fn malformed_pages_fail_without_panicking() {
        assert!(parse_album(&Value::Null, &AlbumRef::default()).is_err());
        assert!(parse_artist(&json!({"contents": []}), &ArtistRef::default()).is_err());
    }

    #[test]
    fn individual_credits_include_unlinked_artists() {
        let refs = artists(&json!({"runs": [
            {"text": "First", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCone"}}},
            {"text": " & "}, {"text": "Second"}, {"text": " • "},
            {"text": "Album", "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREone"}}}
        ]}));
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].id.as_deref(), Some("UCone"));
        assert_eq!(refs[1].id, None);
    }
}
