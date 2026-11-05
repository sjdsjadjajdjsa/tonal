//! Quick search (Cmd+K or Ctrl+K): one box that finds pages, playlists, songs in
//! your library and actions, and runs the one you pick.

use super::{browse::Nav, controller::Controller, equalizer::PRESETS};
use crate::{PaletteItem, model::Track};
use slint::{ModelRc, VecModel};
use std::cell::RefCell;

/// Items shown at once.
const SHOWN: usize = 9;
/// Library songs among them.
const SONGS: usize = 5;

#[derive(Clone)]
enum Pick {
    Page(&'static str),
    Playlist(i64),
    Song(Box<Track>),
    Command(&'static str),
    Preset(&'static str),
    Search(String),
}

struct Entry {
    title: String,
    detail: String,
    kind: &'static str,
    pick: Pick,
}

thread_local! {
    static PICKS: RefCell<Vec<Pick>> = const { RefCell::new(Vec::new()) };
}

const PAGES: [&str; 10] = [
    "Home",
    "Search",
    "Liked songs",
    "Local files",
    "Downloads",
    "On repeat",
    "Rediscover",
    "Recently added",
    "Themes",
    "Settings",
];

const COMMANDS: [(&str, &str); 9] = [
    ("Equalizer", "eq"),
    ("Queue", "queue"),
    ("Lyrics", "lyrics"),
    ("Now playing", "now"),
    ("Mini player", "mini"),
    ("Shuffle", "shuffle"),
    ("Sleep in 30 minutes", "sleep:30"),
    ("Sleep at the end of this song", "sleep:end"),
    ("Turn the sleep timer off", "sleep:off"),
];

/// How well `text` answers `query`: 3 for the start, 2 for the start of a word,
/// 1 anywhere in it, None when it isn't there.
fn score(text: &str, query: &str) -> Option<u8> {
    let text = text.to_lowercase();
    let at = text.find(query)?;
    Some(if at == 0 {
        3
    } else if text[..at].ends_with([' ', '-', '(']) {
        2
    } else {
        1
    })
}

impl Controller {
    pub(super) fn palette_search(&self, query: &str) {
        let query = query.trim().to_lowercase();
        let mut entries: Vec<(u8, Entry)> = Vec::new();
        let mut add = |title: String, detail: &str, kind: &'static str, pick: Pick| {
            let rank = if query.is_empty() {
                Some(1)
            } else {
                score(&title, &query)
            };
            if let Some(rank) = rank {
                entries.push((
                    rank,
                    Entry {
                        title,
                        detail: detail.into(),
                        kind,
                        pick,
                    },
                ));
            }
        };
        for page in PAGES {
            add(page.into(), "Page", "page", Pick::Page(page));
        }
        if !query.is_empty() {
            for (name, command) in COMMANDS {
                add(name.into(), "Action", "action", Pick::Command(command));
            }
            for (name, _) in PRESETS {
                add(
                    format!("Equalizer: {name}"),
                    "Preset",
                    "eq",
                    Pick::Preset(name),
                );
            }
        }
        if let Ok(playlists) = self.library.playlists() {
            for p in playlists {
                add(p.name, "Playlist", "playlist", Pick::Playlist(p.id));
            }
        }
        entries.sort_by_key(|(rank, _)| std::cmp::Reverse(*rank));
        let mut list: Vec<Entry> = entries.into_iter().map(|(_, e)| e).collect();
        if query.is_empty() {
            list.truncate(SHOWN);
        } else {
            list.truncate(SHOWN - SONGS.min(SHOWN - 1) - 1);
            if query.chars().count() >= 2
                && let Ok(songs) = self.library.find(&query, SONGS)
            {
                list.extend(songs.into_iter().map(|t| Entry {
                    title: t.title.clone(),
                    detail: t.artist.clone(),
                    kind: "song",
                    pick: Pick::Song(Box::new(t)),
                }));
            }
            list.push(Entry {
                title: format!("Search for \u{201c}{query}\u{201d}"),
                detail: "Search".into(),
                kind: "search",
                pick: Pick::Search(query.clone()),
            });
        }
        let rows: Vec<PaletteItem> = list
            .iter()
            .map(|e| PaletteItem {
                title: e.title.as_str().into(),
                detail: e.detail.as_str().into(),
                kind: e.kind.into(),
            })
            .collect();
        PICKS.with(|p| *p.borrow_mut() = list.into_iter().map(|e| e.pick).collect());
        if let Some(window) = self.window.upgrade() {
            window.set_palette_items(ModelRc::new(VecModel::from(rows)));
        }
    }

    pub(super) fn palette_pick(&self, index: usize) {
        let Some(pick) = PICKS.with(|p| p.borrow().get(index).cloned()) else {
            return;
        };
        match pick {
            Pick::Page(page) => {
                self.ui(move |w| w.set_page(page.into()));
                self.go(Nav::Page(page.into()));
            }
            Pick::Playlist(id) => self.go(Nav::Playlist(id)),
            Pick::Song(track) => {
                let mut tracks = vec![*track];
                self.playable(&mut tracks);
                self.player.play_tracks(tracks, 0);
            }
            Pick::Command(command) => self.command(command),
            Pick::Preset(name) => self.eq_choose(name),
            Pick::Search(query) => {
                let shown = query.clone();
                let source = self.window.upgrade().map_or(0, |w| {
                    w.set_query(shown.into());
                    w.set_page("Search".into());
                    w.get_source()
                });
                self.go(Nav::Search(query, source));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_of_words_rank_above_middles() {
        assert_eq!(score("Liked songs", "lik"), Some(3));
        assert_eq!(score("Recently added", "add"), Some(2));
        assert_eq!(score("Downloads", "load"), Some(1));
        assert_eq!(score("Queue", "zz"), None);
    }
}
