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
}
impl State {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            display: Vec::new(),
            queue: Vec::new(),
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
