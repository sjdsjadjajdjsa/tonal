use super::{controller::Controller, models};
use crate::{
    CardRow,
    model::{AlbumPage, AlbumRef, ArtistCard, ArtistPage, ArtistRef, SourceKind, Track},
};
use futures::{StreamExt, stream};
use slint::{Model, ModelRc, SharedString, VecModel};
use std::time::{Duration, Instant};

const CACHE_FOR: Duration = Duration::from_secs(10 * 60);
const CACHE_SIZE: usize = 24;

/// A place in the app the back and forward buttons can return to.
#[derive(Clone, Debug, PartialEq)]
pub enum Nav {
    Page(String),
    Playlist(i64),
    Search(String, i32),
    Artist(ArtistRef),
    Album(AlbumRef),
}

#[derive(Clone)]
pub enum Cached {
    Artist(ArtistPage),
    Album(AlbumPage),
}

fn artist_key(r: &ArtistRef) -> String {
    format!(
        "artist:{:?}:{}:{}",
        r.source,
        r.id.as_deref().unwrap_or(""),
        r.name
    )
}

fn album_key(r: &AlbumRef) -> String {
    format!(
        "album:{:?}:{}:{}:{}",
        r.source,
        r.id.as_deref().unwrap_or(""),
        r.title,
        r.artist
    )
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn local_artist(tracks: Vec<Track>, artist: &ArtistRef) -> ArtistPage {
    let mut top: Vec<Track> = tracks
        .into_iter()
        .filter(|t| {
            t.artists().iter().any(|a| same_name(&a.name, &artist.name))
                || same_name(&t.artist, &artist.name)
        })
        .collect();
    top.sort_by(|a, b| a.album.cmp(&b.album).then(a.title.cmp(&b.title)));
    let mut albums: Vec<AlbumRef> = Vec::new();
    for t in &top {
        if let Some(album) = t.album_ref()
            && !albums.iter().any(|a| same_name(&a.title, &album.title))
        {
            albums.push(AlbumRef {
                artist: artist.name.clone(),
                ..album
            });
        }
    }
    ArtistPage {
        artist: artist.clone(),
        image_url: top.iter().find_map(|t| t.artwork_url.clone()),
        top,
        albums,
        ..Default::default()
    }
}

fn local_album(tracks: Vec<Track>, album: &AlbumRef) -> AlbumPage {
    let titled: Vec<Track> = tracks
        .into_iter()
        .filter(|t| {
            t.album
                .as_deref()
                .is_some_and(|a| same_name(a, &album.title))
        })
        .collect();
    let by_artist = |t: &Track| {
        album.artist.is_empty()
            || t.artists()
                .iter()
                .any(|a| same_name(&a.name, &album.artist))
            || same_name(&t.artist, &album.artist)
    };
    // An album artist that no song credits ("Various Artists") keeps the whole album.
    let mut list: Vec<Track> = if titled.iter().any(by_artist) {
        titled.into_iter().filter(|t| by_artist(t)).collect()
    } else {
        titled
    };
    list.sort_by(|a, b| a.play_url.cmp(&b.play_url));
    let artists = list.first().map(|t| t.artists()).unwrap_or_default();
    AlbumPage {
        album: AlbumRef {
            artwork_url: album
                .artwork_url
                .clone()
                .or_else(|| list.iter().find_map(|t| t.artwork_url.clone())),
            ..album.clone()
        },
        kind: "Album".into(),
        artists,
        tracks: list,
    }
}

fn album_meta(page: &AlbumPage) -> String {
    let mut parts = Vec::new();
    if let Some(year) = page.album.year.as_deref().filter(|y| !y.is_empty()) {
        parts.push(year.to_string());
    }
    let count = page.tracks.len();
    if count > 0 {
        parts.push(if count == 1 {
            "1 song".into()
        } else {
            format!("{count} songs")
        });
        let secs: f64 = page.tracks.iter().filter_map(|t| t.duration_secs).sum();
        if secs >= 60.0 {
            let minutes = (secs / 60.0).round() as u64;
            parts.push(if minutes >= 60 {
                format!("{} h {} min", minutes / 60, minutes % 60)
            } else {
                format!("{minutes} min")
            });
        }
    }
    parts.join(" · ")
}

fn album_card(a: &AlbumRef) -> (String, String) {
    (a.title.clone(), a.year.clone().unwrap_or_default())
}

fn artist_card(a: &ArtistCard) -> (String, String) {
    (a.artist.name.clone(), "Artist".into())
}

fn cards(rows: Vec<(String, String)>) -> Vec<CardRow> {
    rows.into_iter()
        .map(|(title, subtitle)| CardRow {
            title: title.into(),
            subtitle: subtitle.into(),
            art: Default::default(),
        })
        .collect()
}

fn strings(values: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        values
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    ))
}

impl Controller {
    /// Opens a place and records it in the history.
    pub fn go(&self, nav: Nav) {
        {
            let mut s = self.state.lock();
            if s.nav.get(s.nav_at) != Some(&nav) {
                let keep = if s.nav.is_empty() { 0 } else { s.nav_at + 1 };
                s.nav.truncate(keep);
                s.nav.push(nav.clone());
                if s.nav.len() > 100 {
                    s.nav.remove(0);
                }
                s.nav_at = s.nav.len() - 1;
            }
        }
        self.open(nav);
    }

    /// Runs a search typed on the search page: edits the current search entry instead of
    /// adding one per keystroke, and drops any forward history it no longer leads to.
    pub fn replace_nav(&self, nav: Nav) {
        let replaced = {
            let mut s = self.state.lock();
            let at = s.nav_at;
            let on_search = matches!(s.nav.get(at), Some(Nav::Search(..)))
                || s.nav.get(at) == Some(&Nav::Page("Search".into()));
            if on_search {
                s.nav[at] = nav.clone();
                s.nav.truncate(at + 1);
            }
            on_search
        };
        if replaced {
            self.open(nav);
        } else {
            self.go(nav);
        }
    }

    /// Reloads a page only while it is the one on screen.
    pub fn refresh_page(&self, page: &str) {
        let current = {
            let s = self.state.lock();
            s.nav.get(s.nav_at) == Some(&Nav::Page(page.into()))
        };
        if current {
            self.load_page(page);
        }
    }

    pub fn forget_playlist(&self, id: i64) {
        let mut s = self.state.lock();
        let at = s.nav_at.min(s.nav.len());
        let before = s.nav[..at]
            .iter()
            .filter(|n| **n == Nav::Playlist(id))
            .count();
        s.nav.retain(|n| *n != Nav::Playlist(id));
        s.nav_at = at.saturating_sub(before).min(s.nav.len().saturating_sub(1));
    }

    pub fn forget_local_pages(&self) {
        self.state
            .lock()
            .browse_cache
            .retain(|(key, _, _)| !key.contains(":Local:"));
    }

    pub fn nav(&self, step: i32) {
        let target = {
            let mut s = self.state.lock();
            let next = s.nav_at as i64 + step as i64;
            if next < 0 || next >= s.nav.len() as i64 {
                return;
            }
            s.nav_at = next as usize;
            s.nav[s.nav_at].clone()
        };
        self.open(target);
    }

    fn sync_nav(&self) {
        let (back, forward) = {
            let s = self.state.lock();
            (s.nav_at > 0, s.nav_at + 1 < s.nav.len())
        };
        self.ui(move |w| {
            w.set_can_back(back);
            w.set_can_forward(forward);
        });
    }

    fn open(&self, nav: Nav) {
        self.sync_nav();
        match nav {
            Nav::Page(page) => self.load_page(&page),
            Nav::Playlist(id) => self.playlist(id),
            Nav::Search(query, source) => {
                let q = query.clone();
                self.ui(move |w| {
                    w.set_page("Search".into());
                    w.set_query(q.into());
                    w.set_source(source);
                });
                if query.trim().is_empty() {
                    self.load_page("Search");
                } else {
                    self.search(query, source, false);
                }
            }
            Nav::Artist(artist) => self.show_artist(artist),
            Nav::Album(album) => self.show_album(album),
        }
    }

    fn cached(&self, key: &str) -> Option<Cached> {
        let s = self.state.lock();
        s.browse_cache
            .iter()
            .find(|(k, at, _)| k == key && at.elapsed() < CACHE_FOR)
            .map(|(_, _, page)| page.clone())
    }

    fn remember(&self, key: String, page: Cached) {
        let mut s = self.state.lock();
        s.browse_cache.retain(|(k, _, _)| *k != key);
        if s.browse_cache.len() >= CACHE_SIZE {
            s.browse_cache.remove(0);
        }
        s.browse_cache.push((key, Instant::now(), page));
    }

    fn begin_page(&self, page: &'static str, heading: String) -> u64 {
        let generation = {
            let mut s = self.state.lock();
            s.generation += 1;
            s.playlist = None;
            s.display.clear();
            s.generation
        };
        self.clear_collection();
        self.ui(move |w| {
            w.set_page(page.into());
            w.set_heading(heading.clone().into());
            w.set_playlist_view(false);
            w.set_browse_loading(true);
            w.set_loading(true);
            models::update(w.get_tracks(), Vec::new());
            if page == "Artist" {
                w.set_artist_name(heading.into());
                w.set_artist_image(Default::default());
                w.set_artist_source(Default::default());
                for model in [w.get_albums(), w.get_singles(), w.get_similar()] {
                    models::update(model, Vec::new());
                }
            } else {
                w.set_album_title(heading.into());
                w.set_album_kind("Album".into());
                w.set_album_meta(Default::default());
                w.set_album_artists(strings(Vec::new()));
                w.set_album_cover(Default::default());
            }
        });
        generation
    }

    fn current(&self, generation: u64) -> bool {
        self.state.lock().generation == generation
    }

    /// Runs `f` on the UI thread only if the page it belongs to is still on screen.
    fn ui_for(&self, generation: u64, f: impl FnOnce(crate::AppWindow) + Send + 'static) {
        let state = self.state.clone();
        self.ui(move |w| {
            if state.lock().generation == generation {
                f(w);
            }
        });
    }

    fn fail(&self, generation: u64, error: anyhow::Error) {
        if self.current(generation) {
            self.ui_for(generation, |w| {
                w.set_browse_loading(false);
                w.set_loading(false);
            });
            self.status(error);
        }
    }

    fn show_artist(&self, artist: ArtistRef) {
        let generation = self.begin_page("Artist", artist.name.clone());
        let key = artist_key(&artist);
        if let Some(Cached::Artist(page)) = self.cached(&key) {
            self.fill_artist(generation, page);
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            let result = if artist.source == SourceKind::Local {
                this.library
                    .local_tracks()
                    .map(|tracks| local_artist(tracks, &artist))
            } else {
                crate::sources::artist(&artist).await
            };
            match result {
                Ok(page) => {
                    this.remember(key, Cached::Artist(page.clone()));
                    this.fill_artist(generation, page);
                }
                Err(e) => this.fail(generation, e),
            }
        });
    }

    fn fill_artist(&self, generation: u64, page: ArtistPage) {
        let mut top = page.top.clone();
        self.playable(&mut top);
        {
            let mut s = self.state.lock();
            if s.generation != generation {
                return;
            }
            s.artist_page = Some(page.clone());
        }
        if !self.display_for(generation, top, page.artist.name.clone()) {
            return;
        }
        let name = page.artist.name.clone();
        let source = page.artist.source.label().to_string();
        let albums: Vec<_> = page.albums.iter().map(album_card).collect();
        let singles: Vec<_> = page.singles.iter().map(album_card).collect();
        let similar: Vec<_> = page.similar.iter().map(artist_card).collect();
        self.ui_for(generation, move |w| {
            w.set_artist_name(name.into());
            w.set_artist_source(source.into());
            models::update(w.get_albums(), cards(albums));
            models::update(w.get_singles(), cards(singles));
            models::update(w.get_similar(), cards(similar));
            w.set_browse_loading(false);
            w.set_loading(false);
        });
        let this = self.clone();
        tokio::spawn(async move {
            if let Some(url) = page.image_url.as_deref()
                && let Ok(pixels) = crate::artwork::load_shaped(url, 336, 0.5).await
            {
                this.ui_for(generation, move |w| {
                    w.set_artist_image(slint::Image::from_rgba8(pixels))
                });
            }
            let jobs = page
                .albums
                .iter()
                .enumerate()
                .map(|(i, a)| ("albums", i, a.artwork_url.clone(), 0.06))
                .chain(
                    page.singles
                        .iter()
                        .enumerate()
                        .map(|(i, a)| ("singles", i, a.artwork_url.clone(), 0.06)),
                )
                .chain(
                    page.similar
                        .iter()
                        .enumerate()
                        .map(|(i, a)| ("similar", i, a.image_url.clone(), 0.5)),
                )
                .filter_map(|(list, i, url, radius)| url.map(|url| (list, i, url, radius)))
                .collect::<Vec<_>>();
            stream::iter(jobs)
                .for_each_concurrent(4, |(list, index, url, radius)| {
                    let this = this.clone();
                    async move {
                        let Ok(pixels) = crate::artwork::load_shaped(&url, 320, radius).await
                        else {
                            return;
                        };
                        this.ui_for(generation, move |w| {
                            let model = match list {
                                "albums" => w.get_albums(),
                                "singles" => w.get_singles(),
                                _ => w.get_similar(),
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

    fn show_album(&self, album: AlbumRef) {
        let generation = self.begin_page("Album", album.title.clone());
        let key = album_key(&album);
        if let Some(Cached::Album(page)) = self.cached(&key) {
            self.fill_album(generation, page);
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            let result = if album.source == SourceKind::Local {
                this.library
                    .local_tracks()
                    .map(|tracks| local_album(tracks, &album))
            } else {
                crate::sources::album(&album).await
            };
            match result {
                Ok(page) => {
                    this.remember(key, Cached::Album(page.clone()));
                    this.fill_album(generation, page);
                }
                Err(e) => this.fail(generation, e),
            }
        });
    }

    fn fill_album(&self, generation: u64, page: AlbumPage) {
        let mut tracks = page.tracks.clone();
        self.playable(&mut tracks);
        {
            let mut s = self.state.lock();
            if s.generation != generation {
                return;
            }
            s.album_page = Some(page.clone());
        }
        if !self.display_for(generation, tracks, page.album.title.clone()) {
            return;
        }
        let title = page.album.title.clone();
        let kind = if page.kind.is_empty() {
            "Album".to_string()
        } else {
            page.kind.clone()
        };
        let meta = album_meta(&page);
        let artists: Vec<String> = page.artists.iter().map(|a| a.name.clone()).collect();
        self.ui_for(generation, move |w| {
            w.set_album_title(title.into());
            w.set_album_kind(kind.into());
            w.set_album_meta(meta.into());
            w.set_album_artists(strings(artists));
            w.set_browse_loading(false);
            w.set_loading(false);
        });
        if let Some(url) = page.album.artwork_url.clone() {
            let this = self.clone();
            tokio::spawn(async move {
                if let Ok(pixels) = crate::artwork::load_shaped(&url, 336, 0.083).await {
                    this.ui_for(generation, move |w| {
                        w.set_album_cover(slint::Image::from_rgba8(pixels))
                    });
                }
            });
        }
    }

    fn track_in(&self, list: &str, index: i32) -> Option<Track> {
        let s = self.state.lock();
        match list {
            "now" => s.current.clone(),
            "queue" => usize::try_from(index)
                .ok()
                .and_then(|i| s.queue.get(i).cloned()),
            _ => usize::try_from(index)
                .ok()
                .and_then(|i| s.display.get(i).cloned()),
        }
    }

    pub fn open_artist(&self, list: &str, index: i32, which: i32) {
        let artist = if list == "album" {
            self.state
                .lock()
                .album_page
                .as_ref()
                .and_then(|p| p.artists.get(which.max(0) as usize).cloned())
        } else {
            self.track_in(list, index)
                .and_then(|t| t.artists().into_iter().nth(which.max(0) as usize))
        };
        if let Some(artist) = artist.filter(|a| !a.name.trim().is_empty()) {
            self.go(Nav::Artist(artist));
        }
    }

    pub fn open_album(&self, list: &str, index: i32) {
        if let Some(album) = self.track_in(list, index).and_then(|t| t.album_ref()) {
            self.go(Nav::Album(album));
        }
    }

    pub fn open_card(&self, kind: &str, index: i32) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if let Some(nav) = self.collection_target(kind, index) {
            self.go(nav);
            return;
        }
        let target = {
            let s = self.state.lock();
            let page = s.artist_page.as_ref();
            match kind {
                "album" => page
                    .and_then(|p| p.albums.get(index))
                    .cloned()
                    .map(Nav::Album),
                "single" => page
                    .and_then(|p| p.singles.get(index))
                    .cloned()
                    .map(Nav::Album),
                _ => page
                    .and_then(|p| p.similar.get(index))
                    .map(|c| Nav::Artist(c.artist.clone())),
            }
        };
        if let Some(nav) = target {
            self.go(nav);
        }
    }

    pub fn artist_action(&self, action: &str) {
        match action {
            "play" => self.play(0),
            "shuffle" => {
                self.player.set_shuffle(true);
                self.play(0);
            }
            _ => {
                let seed = self.state.lock().display.first().cloned();
                if let Some(seed) = seed {
                    let mut tracks = vec![seed.clone()];
                    self.playable(&mut tracks);
                    self.player.play_tracks(tracks, 0);
                    self.theme_playing_from(None);
                    self.radio(seed);
                }
            }
        }
    }

    pub fn album_action(&self, action: &str) {
        match action {
            "play" => self.play(0),
            "shuffle" => {
                self.player.set_shuffle(true);
                self.play(0);
            }
            _ => {
                let page = self.state.lock().album_page.clone();
                let Some(page) = page else { return };
                let origin = page.album.id.as_ref().map(|id| format!("album:{id}"));
                let saved = self
                    .library
                    .create_playlist(&page.album.title, origin.as_deref())
                    .and_then(|id| self.library.add_to_playlist(id, &page.tracks));
                if self.result(saved).is_some() {
                    self.refresh_playlists();
                    self.status(format!("Saved {} to your playlists", page.album.title));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, artist: &str, album: &str) -> Track {
        Track {
            id: format!("local:{title}"),
            source: SourceKind::Local,
            title: title.into(),
            artist: artist.into(),
            album: Some(album.into()),
            play_url: format!("/music/{album}/{title}.flac"),
            ..Default::default()
        }
    }

    #[test]
    fn local_pages_group_by_artist_and_album() {
        let tracks = vec![
            track("b", "Daft Punk", "Discovery"),
            track("a", "Daft Punk", "Discovery"),
            track("x", "Justice", "Cross"),
            track("c", "daft punk", "Homework"),
        ];
        let artist = ArtistRef {
            source: SourceKind::Local,
            name: "Daft Punk".into(),
            id: None,
        };
        let page = local_artist(tracks.clone(), &artist);
        assert_eq!(page.top.len(), 3);
        assert_eq!(
            page.albums
                .iter()
                .map(|a| a.title.as_str())
                .collect::<Vec<_>>(),
            ["Discovery", "Homework"]
        );
        let album = local_album(tracks, &page.albums[0]);
        assert_eq!(
            album
                .tracks
                .iter()
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(album_meta(&album), "2 songs");
    }
}
