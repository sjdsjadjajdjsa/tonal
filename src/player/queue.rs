use crate::model::{RepeatMode, Track};
use rand::seq::SliceRandom;
use std::collections::VecDeque;

/// Songs remembered as just heard, so shuffle leaves them for last.
const RECENT: usize = 50;
/// Shuffle keeps this many songs between two by the same artist when it can.
const ARTIST_GAP: usize = 2;

#[derive(Clone)]
struct Entry {
    key: u64,
    track: Track,
    /// Added with "Play next" or "Add to queue", so it plays before the rest.
    queued: bool,
}

/// Where new songs go in the queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Right after the song playing now.
    Next,
    /// After the song playing now and the songs added before, ahead of the rest.
    Queue,
    /// At the very end, as radio does.
    End,
}

#[derive(Default)]
pub(super) struct Queue {
    entries: Vec<Entry>,
    original: Vec<u64>,
    serial: u64,
    pub current: Option<usize>,
    pub shuffle: bool,
    /// Ids of the songs heard last, newest at the back.
    recent: VecDeque<String>,
}

impl Queue {
    pub fn tracks(&self) -> Vec<Track> {
        self.entries.iter().map(|e| e.track.clone()).collect()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn track(&self) -> Option<Track> {
        self.current
            .and_then(|i| self.entries.get(i))
            .map(|e| e.track.clone())
    }
    pub fn key(&self) -> Option<u64> {
        self.current.and_then(|i| self.key_at(i))
    }
    pub fn keys(&self) -> Vec<u64> {
        self.entries.iter().map(|e| e.key).collect()
    }
    pub fn key_at(&self, index: usize) -> Option<u64> {
        self.entries.get(index).map(|e| e.key)
    }
    pub fn index_of(&self, key: u64) -> Option<usize> {
        self.entries.iter().position(|e| e.key == key)
    }
    pub fn at(&self, index: usize) -> Option<&Track> {
        self.entries.get(index).map(|e| &e.track)
    }

    pub fn replace(&mut self, tracks: Vec<Track>, start: usize) {
        self.restore(tracks, start);
        if self.shuffle
            && let Some(start) = self.current
        {
            self.entries.swap(0, start);
            self.current = Some(0);
            self.shuffle_rest();
        }
    }

    pub fn restore(&mut self, tracks: Vec<Track>, start: usize) {
        self.entries.clear();
        self.original.clear();
        self.current = None;
        self.splice(0, tracks, false);
        self.current = if self.entries.is_empty() {
            None
        } else {
            Some(start.min(self.entries.len() - 1))
        };
    }

    pub fn insert(&mut self, tracks: Vec<Track>, place: Place) {
        let after = self.current.map_or(self.entries.len(), |i| i + 1);
        let index = match place {
            Place::Next => after,
            Place::Queue => after + self.queued_from(after),
            Place::End => self.entries.len(),
        };
        self.splice(index, tracks, place != Place::End);
    }

    /// How many songs added by hand wait in a row from `index`.
    fn queued_from(&self, index: usize) -> usize {
        self.entries
            .get(index..)
            .map_or(0, |rest| rest.iter().take_while(|e| e.queued).count())
    }

    fn splice(&mut self, index: usize, tracks: Vec<Track>, queued: bool) {
        let index = index.min(self.entries.len());
        let original_index = index
            .checked_sub(1)
            .and_then(|i| self.key_at(i))
            .and_then(|key| self.original.iter().position(|k| *k == key))
            .map_or(0, |i| i + 1);
        let mut entries = Vec::new();
        for track in tracks {
            self.serial += 1;
            entries.push(Entry {
                key: self.serial,
                track,
                queued,
            });
        }
        self.original.splice(
            original_index..original_index,
            entries.iter().map(|e| e.key),
        );
        self.entries.splice(index..index, entries);
    }

    /// Swaps the track at `index`, keeping its place, key and shuffle order.
    pub fn set(&mut self, index: usize, track: Track) -> bool {
        match self.entries.get_mut(index) {
            Some(entry) => {
                entry.track = track;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        let old_key = self.key();
        let removed = self.entries.remove(index).key;
        self.original.retain(|key| *key != removed);
        let removed_current = old_key == Some(removed);
        self.current = if removed_current {
            (index < self.entries.len()).then_some(index)
        } else {
            old_key.and_then(|key| self.index_of(key))
        };
        removed_current
    }

    pub fn move_track(&mut self, from: usize, to: usize) {
        if from >= self.len() || to >= self.len() || from == to {
            return;
        }
        let current = self.key();
        let entry = self.entries.remove(from);
        let moved = entry.key;
        self.entries.insert(to, entry);
        self.original.retain(|key| *key != moved);
        let after = to.checked_sub(1).and_then(|i| self.key_at(i));
        let index = after
            .and_then(|key| self.original.iter().position(|k| *k == key))
            .map_or(0, |i| i + 1);
        self.original.insert(index, moved);
        self.current = current.and_then(|key| self.index_of(key));
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if self.shuffle == on {
            return;
        }
        let current = self.key();
        self.shuffle = on;
        if on {
            self.shuffle_rest();
        } else {
            self.entries
                .sort_by_key(|entry| self.original.iter().position(|key| *key == entry.key));
            self.current = current.and_then(|key| self.index_of(key));
        }
    }

    /// Remembers that `id` just played.
    pub fn heard(&mut self, id: &str) {
        self.recent.retain(|r| r != id);
        self.recent.push_back(id.to_owned());
        while self.recent.len() > RECENT {
            self.recent.pop_front();
        }
    }

    /// Replaces what counts as just heard, oldest first.
    pub fn set_recent(&mut self, ids: Vec<String>) {
        self.recent = ids.into_iter().rev().take(RECENT).rev().collect();
    }

    fn shuffle_rest(&mut self) {
        let upcoming = self.current.map_or(0, |index| index + 1);
        let upcoming = upcoming + self.queued_from(upcoming);
        let mut rest: Vec<Entry> = self.entries.drain(upcoming..).collect();
        rest.shuffle(&mut rand::rng());
        let (fresh, heard): (Vec<_>, Vec<_>) = rest
            .into_iter()
            .partition(|e| !self.recent.contains(&e.track.id));
        let mut last: Vec<String> = self
            .entries
            .iter()
            .rev()
            .take(ARTIST_GAP)
            .map(|e| artist_of(&e.track))
            .collect();
        last.reverse();
        let fresh = spread(fresh, &mut last);
        let heard = spread(heard, &mut last);
        self.entries.extend(fresh);
        self.entries.extend(heard);
    }

    pub fn next(&self, repeat: RepeatMode, natural: bool) -> Option<usize> {
        let Some(current) = self.current else {
            return (!self.entries.is_empty()).then_some(0);
        };
        if natural && repeat == RepeatMode::One {
            return Some(current);
        }
        if current + 1 < self.len() {
            Some(current + 1)
        } else if repeat == RepeatMode::All && !self.entries.is_empty() {
            Some(0)
        } else {
            None
        }
    }

    pub fn previous(&self, repeat: RepeatMode) -> Option<usize> {
        let Some(current) = self.current else {
            return self.len().checked_sub(1);
        };
        if current > 0 {
            Some(current - 1)
        } else if repeat == RepeatMode::All {
            self.len().checked_sub(1)
        } else {
            Some(0)
        }
    }
}

/// Who a song is by, for keeping the same artist apart.
fn artist_of(track: &Track) -> String {
    track
        .artists()
        .into_iter()
        .next()
        .map(|a| a.name)
        .unwrap_or_default()
        .trim()
        .to_lowercase()
}

/// Keeps the shuffled order but moves songs along so the same artist doesn't come
/// back within `ARTIST_GAP` songs, unless only that artist is left. `last` holds the
/// artists just before, and is left holding the last ones placed.
fn spread(pool: Vec<Entry>, last: &mut Vec<String>) -> Vec<Entry> {
    let mut pool: Vec<(String, Entry)> =
        pool.into_iter().map(|e| (artist_of(&e.track), e)).collect();
    let mut out = Vec::with_capacity(pool.len());
    while !pool.is_empty() {
        let pick = pool
            .iter()
            .position(|(artist, _)| !last.contains(artist))
            .unwrap_or(0);
        let (artist, entry) = pool.remove(pick);
        last.push(artist);
        if last.len() > ARTIST_GAP {
            last.remove(0);
        }
        out.push(entry);
    }
    out
}
