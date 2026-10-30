//! Local album grid and listening stats.
use super::{controller::Controller, models};
use crate::{
    CardRow,
    library::Top,
    model::{AlbumRef, ArtistRef, SourceKind, Track},
};
use futures::{StreamExt, stream};
use parking_lot::Mutex;
use slint::Model;
use std::{collections::HashMap, sync::OnceLock, time::SystemTime};

/// What the album grid and the stats cards link to, by card index.
#[derive(Default)]
pub struct Collection {
    /// Local files shows the album grid instead of the song list.
    pub local_albums: bool,
    pub albums: Vec<LocalAlbum>,
    pub sort: Sort,
    pub top_artists: Vec<Top>,
    pub top_albums: Vec<Top>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Sort {
    #[default]
    Artist,
    Year,
    Added,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalAlbum {
    pub title: String,
    pub artist: String,
    pub year: Option<String>,
    pub artwork_url: Option<String>,
    pub added: Option<SystemTime>,
}

#[derive(Clone, Debug, Default)]
pub struct Tags {
    pub album_artist: Option<String>,
    pub year: Option<String>,
    pub added: Option<SystemTime>,
}

/// Album artist, year and file date of a local file, read once per file version.
pub fn tags(path: &str) -> Tags {
    type Cache = HashMap<String, (Option<SystemTime>, Tags)>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let meta = std::fs::metadata(path).ok();
    let modified = meta.as_ref().and_then(|m| m.modified().ok());
    if let Some((at, tags)) = cache.lock().get(path)
        && *at == modified
    {
        return tags.clone();
    }
    use lofty::prelude::{Accessor, ItemKey, TaggedFileExt};
    let file = lofty::probe::Probe::open(path).and_then(|probe| {
        probe
            .options(
                lofty::config::ParseOptions::new()
                    .read_properties(false)
                    .read_cover_art(false),
            )
            .read()
    });
    let tag = file
        .as_ref()
        .ok()
        .and_then(|f| f.primary_tag().or_else(|| f.first_tag()));
    let tags = Tags {
        album_artist: tag
            .and_then(|t| t.get_string(ItemKey::AlbumArtist))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        year: tag
            .and_then(|t| t.date())
            .map(|d| d.year)
            .filter(|y| *y > 0)
            .map(|y| y.to_string()),
        added: meta.and_then(|m| m.created().ok()).or(modified),
    };
    cache
        .lock()
        .insert(path.to_string(), (modified, tags.clone()));
    tags
}

/// Groups local songs by album and album artist (the song artist when untagged).
pub fn group(tracks: &[Track], tags: impl Fn(&Track) -> Tags) -> Vec<LocalAlbum> {
    let mut albums: Vec<LocalAlbum> = Vec::new();
    for t in tracks {
        let Some(title) = t.album.as_deref().map(str::trim).filter(|a| !a.is_empty()) else {
            continue;
        };
        let info = tags(t);
        let artist = info
            .album_artist
            .unwrap_or_else(|| t.artist.trim().to_string());
        let same = |a: &LocalAlbum| {
            a.title.eq_ignore_ascii_case(title) && a.artist.eq_ignore_ascii_case(&artist)
        };
        match albums.iter_mut().find(|a| same(a)) {
            Some(album) => {
                album.year = album.year.take().or(info.year);
                album.artwork_url = album.artwork_url.take().or_else(|| t.artwork_url.clone());
                album.added = album.added.max(info.added);
            }
            None => albums.push(LocalAlbum {
                title: title.to_string(),
                artist,
                year: info.year,
                artwork_url: t.artwork_url.clone(),
                added: info.added,
            }),
        }
    }
    albums
}

pub fn sort(albums: &mut [LocalAlbum], sort: Sort) {
    let name = |a: &LocalAlbum| (a.artist.to_lowercase(), a.title.to_lowercase());
    match sort {
        Sort::Artist => albums.sort_by(|a, b| {
            let (aa, at) = name(a);
            let (ba, bt) = name(b);
            aa.cmp(&ba).then(a.year.cmp(&b.year)).then(at.cmp(&bt))
        }),
        Sort::Year => albums.sort_by(|a, b| {
            b.year
                .is_some()
                .cmp(&a.year.is_some())
                .then(b.year.cmp(&a.year))
                .then(name(a).cmp(&name(b)))
        }),
        Sort::Added => albums.sort_by(|a, b| b.added.cmp(&a.added).then(name(a).cmp(&name(b)))),
    }
}

fn album_subtitle(a: &LocalAlbum) -> String {
    match a.year.as_deref() {
        Some(year) => format!("{} · {year}", a.artist),
        None => a.artist.clone(),
    }
}

fn plays(n: u64) -> String {
    if n == 1 {
        "1 play".into()
    } else {
        format!("{n} plays")
    }
}

fn card(title: String, subtitle: String) -> CardRow {
    CardRow {
        title: title.into(),
        subtitle: subtitle.into(),
        art: Default::default(),
    }
}

/// Keeps covers that are already loaded when the same cards come back in another order.
fn keep_art(model: &slint::ModelRc<CardRow>, rows: &mut [CardRow]) -> Vec<usize> {
    let mut missing = Vec::new();
    for (i, row) in rows.iter_mut().enumerate() {
        let old = (0..model.row_count())
            .filter_map(|j| model.row_data(j))
            .find(|o| o.title == row.title && o.subtitle == row.subtitle && o.art.size().width > 0);
        match old {
            Some(o) => row.art = o.art,
            None => missing.push(i),
        }
    }
    missing
}

/// Milliseconds at the start of the UTC calendar month containing `ms`.
fn month_start(ms: i64) -> i64 {
    const DAY: i64 = 86_400_000;
    // Day of the month from the days since 1970 (Howard Hinnant's civil_from_days).
    let z = ms.div_euclid(DAY) + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    (ms.div_euclid(DAY) - (day - 1)) * DAY
}

const CARD_ART: u32 = 240;
const STATS_LIMIT: usize = 10;

impl Controller {
    /// Empties the card models so their covers don't stay in memory off screen.
    pub(super) fn clear_collection(&self) {
        self.ui(|w| {
            for model in [
                w.get_local_albums(),
                w.get_top_artists(),
                w.get_top_albums(),
            ] {
                models::update(model, Vec::new());
            }
        });
    }

    pub fn local_view(&self, albums: bool) {
        self.ui(move |w| w.set_local_view(albums as i32));
        if albums {
            self.load_local_albums();
        } else {
            self.clear_collection();
        }
    }

    pub fn local_sort(&self, by: i32) {
        let by = match by {
            1 => Sort::Year,
            2 => Sort::Added,
            _ => Sort::Artist,
        };
        let (generation, albums) = {
            let mut s = self.state.lock();
            s.collection.sort = by;
            sort(&mut s.collection.albums, by);
            (s.generation, s.collection.albums.clone())
        };
        self.show_albums(generation, albums);
    }

    pub fn load_local_albums(&self) {
        let generation = self.state.lock().generation;
        let this = self.clone();
        tokio::spawn(async move {
            let library = this.library.clone();
            let by = this.state.lock().collection.sort;
            let albums = tokio::task::spawn_blocking(move || {
                library.local_tracks().map(|tracks| {
                    let mut albums = group(&tracks, |t| tags(&t.play_url));
                    sort(&mut albums, by);
                    albums
                })
            })
            .await;
            let Some(albums) = this.result(albums.map_err(anyhow::Error::from).and_then(|r| r))
            else {
                return;
            };
            {
                let mut s = this.state.lock();
                if s.generation != generation {
                    return;
                }
                s.collection.albums = albums.clone();
            }
            this.show_albums(generation, albums);
        });
    }

    fn show_albums(&self, generation: u64, albums: Vec<LocalAlbum>) {
        let state = self.state.clone();
        let this = self.clone();
        self.ui(move |w| {
            if state.lock().generation != generation {
                return;
            }
            let model = w.get_local_albums();
            let mut rows: Vec<CardRow> = albums
                .iter()
                .map(|a| card(a.title.clone(), album_subtitle(a)))
                .collect();
            let missing = keep_art(&model, &mut rows);
            models::update(model, rows);
            let jobs: Vec<_> = missing
                .into_iter()
                .filter_map(|i| Some((i, albums[i].artwork_url.clone()?)))
                .collect();
            this.load_cards(generation, "local-albums", jobs, 0.06);
        });
    }

    /// Fills in card covers, checking each row is still the same card when it arrives.
    fn load_cards(
        &self,
        generation: u64,
        list: &'static str,
        jobs: Vec<(usize, String)>,
        radius: f32,
    ) {
        if jobs.is_empty() {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            stream::iter(jobs)
                .for_each_concurrent(4, |(index, url)| {
                    let this = this.clone();
                    async move {
                        let Ok(pixels) = crate::artwork::load_large(&url, CARD_ART, radius).await
                        else {
                            return;
                        };
                        let state = this.state.clone();
                        this.ui(move |w| {
                            if state.lock().generation != generation {
                                return;
                            }
                            let model = match list {
                                "local-albums" => w.get_local_albums(),
                                "top-artists" => w.get_top_artists(),
                                _ => w.get_top_albums(),
                            };
                            if let Some(mut row) = model.row_data(index) {
                                row.art = slint::Image::from_rgba8(pixels);
                                model.set_row_data(index, row);
                            }
                        });
                    }
                })
                .await;
        });
    }

    /// Top artists and albums, `period` 0 for this calendar month and 1 for all time.
    pub fn load_stats(&self, period: i32) {
        let since = (period == 0).then(|| {
            month_start(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
            )
        });
        let (Some(artists), Some(albums)) = (
            self.result(self.library.top_artists(since, STATS_LIMIT)),
            self.result(self.library.top_albums(since, STATS_LIMIT)),
        ) else {
            return;
        };
        let generation = {
            let mut s = self.state.lock();
            s.collection.top_artists = artists.clone();
            s.collection.top_albums = albums.clone();
            s.generation
        };
        let artist_rows: Vec<_> = artists
            .iter()
            .map(|t| (t.title.clone(), plays(t.plays)))
            .collect();
        let album_rows: Vec<_> = albums
            .iter()
            .map(|t| {
                (
                    t.title.clone(),
                    format!("{} · {}", t.artist, plays(t.plays)),
                )
            })
            .collect();
        let this = self.clone();
        let state = self.state.clone();
        self.ui(move |w| {
            if state.lock().generation != generation {
                return;
            }
            for (list, model, rows, entries, radius) in [
                (
                    "top-artists",
                    w.get_top_artists(),
                    artist_rows,
                    &artists,
                    0.5,
                ),
                ("top-albums", w.get_top_albums(), album_rows, &albums, 0.06),
            ] {
                let mut rows: Vec<CardRow> = rows.into_iter().map(|(t, s)| card(t, s)).collect();
                let missing = keep_art(&model, &mut rows);
                models::update(model, rows);
                let jobs = missing
                    .into_iter()
                    .filter_map(|i| Some((i, entries[i].track.artwork_url.clone()?)))
                    .collect();
                this.load_cards(generation, list, jobs, radius);
            }
        });
    }

    /// Where a card from the album grid or the stats leads.
    pub(super) fn collection_target(&self, kind: &str, index: usize) -> Option<super::browse::Nav> {
        use super::browse::Nav;
        let s = self.state.lock();
        let c = &s.collection;
        match kind {
            "local-album" => c.albums.get(index).map(|a| {
                Nav::Album(AlbumRef {
                    source: SourceKind::Local,
                    title: a.title.clone(),
                    artist: a.artist.clone(),
                    year: a.year.clone(),
                    artwork_url: a.artwork_url.clone(),
                    id: None,
                })
            }),
            "top-artist" => c.top_artists.get(index).map(|t| {
                Nav::Artist(t.track.artists().into_iter().next().unwrap_or(ArtistRef {
                    source: t.track.source,
                    name: t.title.clone(),
                    id: None,
                }))
            }),
            "top-album" => c.top_albums.get(index).and_then(|t| {
                t.track.album_ref().map(|a| {
                    Nav::Album(AlbumRef {
                        artwork_url: t.track.artwork_url.clone(),
                        ..a
                    })
                })
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn track(title: &str, artist: &str, album: Option<&str>) -> Track {
        Track {
            id: format!("local:{title}"),
            source: SourceKind::Local,
            title: title.into(),
            artist: artist.into(),
            album: album.map(Into::into),
            play_url: format!("/music/{title}.flac"),
            ..Default::default()
        }
    }

    #[test]
    fn month_starts_on_the_first() {
        const DAY: i64 = 86_400_000;
        // 2026-10-29 13:00 UTC → 2026-10-01 00:00 UTC.
        assert_eq!(month_start(1_793_278_800_000), 1_790_812_800_000);
        // 2024-03-01 00:00 is its own start; 2024-02-29 belongs to February.
        assert_eq!(month_start(1_709_251_200_000), 1_709_251_200_000);
        assert_eq!(month_start(1_709_251_200_000 - DAY), 1_706_745_600_000);
    }

    #[test]
    fn albums_group_by_title_and_album_artist() {
        let tracks = vec![
            track("a", "Daft Punk", Some("Discovery")),
            track("b", "Daft Punk feat. Romanthony", Some("discovery")),
            track("c", "Justice", Some("Cross")),
            track("d", "Nobody", Some("Greatest Hits")),
            track("e", "Somebody", Some("Greatest Hits")),
            track("f", "Loose", None),
        ];
        let mut albums = group(&tracks, |t| Tags {
            album_artist: matches!(t.title.as_str(), "a" | "b").then(|| "Daft Punk".into()),
            year: match t.title.as_str() {
                "b" => Some("2001".into()),
                "c" => Some("2007".into()),
                _ => None,
            },
            added: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(t.title.as_bytes()[0] as u64)),
        });
        assert_eq!(albums.len(), 4);
        assert_eq!(albums[0].year.as_deref(), Some("2001"));
        let order = |albums: &[LocalAlbum]| {
            albums
                .iter()
                .map(|a| format!("{}/{}", a.artist, a.title))
                .collect::<Vec<_>>()
        };
        sort(&mut albums, Sort::Artist);
        assert_eq!(
            order(&albums),
            [
                "Daft Punk/Discovery",
                "Justice/Cross",
                "Nobody/Greatest Hits",
                "Somebody/Greatest Hits"
            ]
        );
        sort(&mut albums, Sort::Year);
        assert_eq!(
            order(&albums)[..2],
            ["Justice/Cross", "Daft Punk/Discovery"]
        );
        sort(&mut albums, Sort::Added);
        assert_eq!(order(&albums)[0], "Somebody/Greatest Hits");
        assert_eq!(album_subtitle(&albums[3]), "Daft Punk · 2001");
    }
}
