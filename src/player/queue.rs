use crate::model::{RepeatMode, Track};
use rand::seq::SliceRandom;

#[derive(Clone)]
struct Entry {
    key: u64,
    track: Track,
}

#[derive(Default)]
pub(super) struct Queue {
    entries: Vec<Entry>,
    original: Vec<u64>,
    serial: u64,
    pub current: Option<usize>,
    pub shuffle: bool,
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
        self.entries.clear();
        self.original.clear();
        self.current = None;
        self.insert(tracks, false);
        self.current = if self.entries.is_empty() {
            None
        } else {
            Some(start.min(self.entries.len() - 1))
        };
        if self.shuffle {
            self.shuffle_rest();
        }
    }

    pub fn insert(&mut self, tracks: Vec<Track>, next: bool) {
        let index = if next {
            self.current.map_or(0, |i| i + 1)
        } else {
            self.entries.len()
        };
        let anchor = if next { self.key() } else { None };
        let original_index = anchor
            .and_then(|key| self.original.iter().position(|k| *k == key))
            .map_or(self.original.len(), |i| i + 1);
        let mut entries = Vec::new();
        for track in tracks {
            self.serial += 1;
            entries.push(Entry {
                key: self.serial,
                track,
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
            if self.entries.is_empty() {
                None
            } else {
                Some(index.min(self.entries.len() - 1))
            }
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

    fn shuffle_rest(&mut self) {
        let upcoming = self.current.map_or(0, |index| index + 1);
        self.entries[upcoming..].shuffle(&mut rand::rng());
    }

    pub fn next(&self, repeat: RepeatMode, natural: bool) -> Option<usize> {
        let current = self.current?;
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
        let current = self.current?;
        if current > 0 {
            Some(current - 1)
        } else if repeat == RepeatMode::All {
            self.len().checked_sub(1)
        } else {
            Some(0)
        }
    }
}
