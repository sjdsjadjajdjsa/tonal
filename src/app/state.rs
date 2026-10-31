use crate::{
    integrations::lyrics::Lyrics,
    library::Session,
    model::{Settings, Track},
};
use std::time::Instant;

pub struct State {
    pub settings: Settings,
    pub display: Vec<Track>,
    pub queue: Vec<Track>,
    /// The engine's stable key for each `queue` entry.
    pub queue_keys: Vec<u64>,
    pub current: Option<Track>,
    pub index: Option<usize>,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub playlist: Option<i64>,
    pub selected: Option<usize>,
    pub generation: u64,
    pub lyrics: Option<Lyrics>,
    pub lyric_index: Option<usize>,
    pub played: f64,
    pub started: i64,
    pub scrobbled: bool,
    pub saved: Instant,
    pub restore_position: Option<f64>,
    pub muted_from: Option<f64>,
    pub toast: u64,
    pub warned_youtube_runtime: bool,
    /// Load failures in a row with nothing playing between them.
    pub stall: u32,
    /// Track ids already given their one fallback attempt.
    pub fallback_tried: std::collections::HashSet<String>,
    pub nav: Vec<super::browse::Nav>,
    pub nav_at: usize,
    pub artist_page: Option<crate::model::ArtistPage>,
    pub album_page: Option<crate::model::AlbumPage>,
    pub browse_cache: Vec<(String, Instant, super::browse::Cached)>,
    pub collection: super::collection::Collection,
}
impl State {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            display: Vec::new(),
            queue: Vec::new(),
            queue_keys: Vec::new(),
            current: None,
            index: None,
            position: 0.0,
            duration: 0.0,
            paused: true,
            playlist: None,
            selected: None,
            generation: 0,
            lyrics: None,
            lyric_index: None,
            played: 0.0,
            started: 0,
            scrobbled: false,
            saved: Instant::now(),
            restore_position: None,
            muted_from: None,
            toast: 0,
            warned_youtube_runtime: false,
            stall: 0,
            fallback_tried: Default::default(),
            nav: Vec::new(),
            nav_at: 0,
            artist_page: None,
            album_page: None,
            browse_cache: Vec::new(),
            collection: Default::default(),
        }
    }
    pub fn session(&self) -> Session {
        Session {
            tracks: self.queue.clone(),
            current: self.index,
            position: self.position,
        }
    }
}
