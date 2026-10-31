use crate::model::{EqFilter, FilterKind, RepeatMode, Track};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    QueueChanged {
        tracks: Vec<Track>,
        /// Stable queue key of each entry in `tracks`, unchanged by moves and shuffle.
        keys: Vec<u64>,
        current: Option<usize>,
    },
    NowPlaying(Option<Track>),
    Position {
        pos: f64,
        duration: f64,
    },
    Paused(bool),
    Buffering(bool),
    Volume(f64),
    Shuffle(bool),
    Repeat(RepeatMode),
    TrackFinished(Track),
    NearQueueEnd,
    Error(String),
    /// A YouTube track failed to load. The engine holds on it (no skip) until
    /// the app answers with `replace` (a fallback) or `skip_failed`, each naming
    /// the entry by `key` so a late answer never hits another song.
    LoadFailed {
        key: u64,
        track: Track,
        error: String,
    },
    /// The playing track is nearing its end; `track` is the entry that plays
    /// next. The app makes sure it loads and swaps in a fallback before the gap.
    CheckNext {
        key: u64,
        track: Track,
    },
    /// Short format tag of what plays, like "FLAC 24/96" or "Opus 160k".
    Quality(String),
    /// Output devices of the audio backend mpv uses.
    Devices(Vec<AudioDevice>),
}

#[derive(Clone, Debug)]
pub struct PlayerConfig {
    pub volume: f64,
    pub crossfade_secs: f64,
    pub normalize: bool,
    pub gapless: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub equalizer: Equalizer,
    /// mpv audio device name; empty means the system default.
    pub audio_device: String,
    pub replaygain: bool,
    /// No filters, no software volume, exclusive output, no resampling by mpv.
    pub bit_perfect: bool,
}

pub use crate::model::EQ_BANDS;

#[derive(Clone, Debug, PartialEq)]
pub struct Equalizer {
    pub gains: [f64; 6],
    /// Center frequency of each band in Hz.
    pub freqs: [f64; 6],
    pub preamp: f64,
    /// Headphone correction, applied before the bands. Changing it rebuilds the graph.
    pub correction: Vec<EqFilter>,
}

impl Default for Equalizer {
    fn default() -> Self {
        Self {
            gains: [0.0; 6],
            freqs: EQ_BANDS,
            preamp: 0.0,
            correction: Vec::new(),
        }
    }
}

/// Label of band `i` inside the graph, the target of its live commands.
fn band_target(i: usize) -> String {
    match i {
        0 => "lowshelf@b0".into(),
        5 => "highshelf@b5".into(),
        _ => format!("equalizer@b{i}"),
    }
}

impl Equalizer {
    /// mpv filter for the whole equalizer, labelled so each band can be changed live.
    fn graph(&self) -> String {
        let correction = self.correction.iter().enumerate().map(|(i, c)| {
            let (f, g, q) = (c.freq, c.gain, c.q);
            let name = match c.kind {
                FilterKind::Peak => "equalizer",
                FilterKind::LowShelf => "lowshelf",
                FilterKind::HighShelf => "highshelf",
            };
            format!("{name}@h{i}=f={f:.0}:t=q:w={q:.2}:g={g:.1}")
        });
        let bands = self
            .freqs
            .iter()
            .zip(self.gains)
            .enumerate()
            .map(|(i, (f, g))| match i {
                0 | 5 => format!("{}=f={f:.0}:g={g:.1}", band_target(i)),
                _ => format!("{}=f={f:.0}:t=o:w=1:g={g:.1}", band_target(i)),
            });
        let filters: Vec<String> = correction.chain(bands).collect();
        format!(
            "@eq:lavfi=[{},volume@pre=volume={:.1}dB]",
            filters.join(","),
            self.preamp
        )
    }

    /// Live commands for every band's gain and frequency, then the preamp.
    fn commands(&self) -> Vec<Value> {
        let mut out: Vec<Value> = (0..6)
            .flat_map(|i| {
                [
                    json!([
                        "af-command",
                        "eq",
                        "g",
                        format!("{:.1}", self.gains[i]),
                        band_target(i)
                    ]),
                    json!([
                        "af-command",
                        "eq",
                        "f",
                        format!("{:.0}", self.freqs[i]),
                        band_target(i)
                    ]),
                ]
            })
            .collect();
        out.push(json!([
            "af-command",
            "eq",
            "volume",
            format!("{:.1}dB", self.preamp),
            "volume@pre"
        ]));
        out
    }
}

/// The filter chain. ReplayGain is a gain mpv applies itself, so it needs no filter.
fn filters(normalize: bool, equalizer: &Equalizer) -> String {
    let eq = equalizer.graph();
    if normalize {
        format!("dynaudnorm=f=250:g=15,{eq}")
    } else {
        eq
    }
}

mod mpv;
mod quality;
mod queue;
#[cfg(test)]
mod tests;

use anyhow::Result;
pub use mpv::AudioDevice;
use mpv::{Disconnected, Launch, Message, Mpv};
use quality::Quality;
use queue::Queue;
use serde_json::{Value, json};
use tokio::{
    sync::mpsc::{UnboundedReceiver, unbounded_channel},
    time::{Duration, Instant},
};

enum Cmd {
    Play(Vec<Track>, usize),
    Restore(Vec<Track>, usize, f64),
    Enqueue(Vec<Track>, bool),
    Remove(usize),
    Replace(u64, Track),
    SkipFailed(u64),
    Move(usize, usize),
    Clear,
    Jump(usize),
    Next,
    Prev,
    TogglePause,
    Pause(bool),
    Stop,
    Seek(f64, bool),
    Volume(f64),
    Shuffle(bool),
    Repeat(RepeatMode),
    Crossfade(f64),
    Normalize(bool),
    Equalizer(Equalizer),
    Gapless(bool),
    YtdlOptions,
    RefreshPrefetch,
    AudioDevice(String),
    ReplayGain(bool),
    BitPerfect(bool),
    Devices,
    Shutdown,
}

#[derive(Clone)]
pub struct PlayerHandle {
    tx: UnboundedSender<Cmd>,
}

pub fn spawn(config: PlayerConfig, events: UnboundedSender<PlayerEvent>) -> Result<PlayerHandle> {
    tokio::runtime::Handle::try_current()?;
    let (tx, rx) = unbounded_channel();
    tokio::spawn(Engine::new(config, events).run(rx));
    Ok(PlayerHandle { tx })
}

impl PlayerHandle {
    pub fn play_tracks(&self, tracks: Vec<Track>, start: usize) {
        let _ = self.tx.send(Cmd::Play(tracks, start));
    }
    pub fn restore_tracks(&self, tracks: Vec<Track>, start: usize, position: f64) {
        let _ = self.tx.send(Cmd::Restore(tracks, start, position));
    }
    pub fn enqueue(&self, tracks: Vec<Track>) {
        let _ = self.tx.send(Cmd::Enqueue(tracks, false));
    }
    pub fn play_next(&self, tracks: Vec<Track>) {
        let _ = self.tx.send(Cmd::Enqueue(tracks, true));
    }
    pub fn remove(&self, index: usize) {
        let _ = self.tx.send(Cmd::Remove(index));
    }
    /// Swaps `track` into entry `key`: plays it while it is the held, current
    /// one, otherwise repairs the slot without interrupting playback.
    pub fn replace(&self, key: u64, track: Track) {
        let _ = self.tx.send(Cmd::Replace(key, track));
    }
    /// Answers a `LoadFailed` for entry `key` the app could not recover: moves on.
    /// Ignored when `key` is no longer the held, current entry.
    pub fn skip_failed(&self, key: u64) {
        let _ = self.tx.send(Cmd::SkipFailed(key));
    }
    pub fn move_track(&self, from: usize, to: usize) {
        let _ = self.tx.send(Cmd::Move(from, to));
    }
    pub fn clear(&self) {
        let _ = self.tx.send(Cmd::Clear);
    }
    pub fn jump(&self, index: usize) {
        let _ = self.tx.send(Cmd::Jump(index));
    }
    pub fn next(&self) {
        let _ = self.tx.send(Cmd::Next);
    }
    pub fn prev(&self) {
        let _ = self.tx.send(Cmd::Prev);
    }
    pub fn toggle_pause(&self) {
        let _ = self.tx.send(Cmd::TogglePause);
    }
    pub fn set_paused(&self, paused: bool) {
        let _ = self.tx.send(Cmd::Pause(paused));
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
    pub fn seek(&self, secs: f64) {
        let _ = self.tx.send(Cmd::Seek(secs, false));
    }
    pub fn seek_relative(&self, secs: f64) {
        let _ = self.tx.send(Cmd::Seek(secs, true));
    }
    pub fn set_volume(&self, volume: f64) {
        let _ = self.tx.send(Cmd::Volume(volume));
    }
    pub fn set_shuffle(&self, on: bool) {
        let _ = self.tx.send(Cmd::Shuffle(on));
    }
    pub fn set_repeat(&self, mode: RepeatMode) {
        let _ = self.tx.send(Cmd::Repeat(mode));
    }
    pub fn set_crossfade(&self, secs: f64) {
        let _ = self.tx.send(Cmd::Crossfade(secs));
    }
    pub fn set_normalize(&self, on: bool) {
        let _ = self.tx.send(Cmd::Normalize(on));
    }
    pub fn set_equalizer(&self, equalizer: Equalizer) {
        let _ = self.tx.send(Cmd::Equalizer(equalizer));
    }
    pub fn refresh_ytdl_options(&self) {
        let _ = self.tx.send(Cmd::YtdlOptions);
    }
    /// Re-syncs mpv's gapless prefetch with the queue, e.g. after a stream URL
    /// was cached for the next entry.
    pub fn refresh_prefetch(&self) {
        let _ = self.tx.send(Cmd::RefreshPrefetch);
    }
    pub fn set_gapless(&self, on: bool) {
        let _ = self.tx.send(Cmd::Gapless(on));
    }
    /// mpv device name, empty for the system default. Applies live.
    pub fn set_audio_device(&self, name: &str) {
        let _ = self.tx.send(Cmd::AudioDevice(name.to_string()));
    }
    pub fn set_replaygain(&self, on: bool) {
        let _ = self.tx.send(Cmd::ReplayGain(on));
    }
    /// Restarts mpv at the same spot, since exclusive output can't change live.
    pub fn set_bit_perfect(&self, on: bool) {
        let _ = self.tx.send(Cmd::BitPerfect(on));
    }
    /// Emits `PlayerEvent::Devices`, even before anything has played.
    pub fn refresh_devices(&self) {
        let _ = self.tx.send(Cmd::Devices);
    }
    pub fn shutdown(&self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }
}

struct Fade {
    next: u64,
    duration: f64,
    elapsed: f64,
    last_tick: Instant,
    ready: bool,
    position: f64,
    track_duration: f64,
}

struct Engine {
    config: PlayerConfig,
    queue: Queue,
    events: UnboundedSender<PlayerEvent>,
    ipc_tx: UnboundedSender<Message>,
    ipc_rx: UnboundedReceiver<Message>,
    active: Option<Mpv>,
    standby: Option<Mpv>,
    fade: Option<Fade>,
    pending: Option<u64>,
    pending_entry: Option<i64>,
    played: f64,
    last_playhead: Option<f64>,
    gapless_eof: bool,
    gapless_position: bool,
    near_key: Option<u64>,
    pos: f64,
    duration: f64,
    paused: bool,
    cache_paused: bool,
    start_at: Option<f64>,
    core_idle: bool,
    buffering: bool,
    last_position: Instant,
    failures: usize,
    restarts: Vec<Instant>,
    quality: Quality,
    devices: Vec<AudioDevice>,
    /// The playing file has ReplayGain tags, so they replace dynaudnorm.
    rg_tagged: bool,
    /// Queue key of the failed YouTube entry waiting for the app's answer.
    held: Option<u64>,
    /// Current queue key whose next entry was already sent for a pre-check.
    precheck_key: Option<u64>,
}

impl Engine {
    fn new(mut config: PlayerConfig, events: UnboundedSender<PlayerEvent>) -> Self {
        config.volume = finite(config.volume).clamp(0.0, 100.0);
        config.crossfade_secs = finite(config.crossfade_secs).max(0.0);
        let (ipc_tx, ipc_rx) = unbounded_channel();
        let mut queue = Queue::default();
        queue.shuffle = config.shuffle;
        Self {
            config,
            queue,
            events,
            ipc_tx,
            ipc_rx,
            active: None,
            standby: None,
            fade: None,
            pending: None,
            pending_entry: None,
            played: 0.0,
            last_playhead: None,
            gapless_eof: false,
            gapless_position: false,
            near_key: None,
            pos: 0.0,
            duration: 0.0,
            paused: true,
            cache_paused: false,
            start_at: None,
            core_idle: true,
            buffering: false,
            last_position: Instant::now() - Duration::from_secs(1),
            failures: 0,
            restarts: Vec::new(),
            quality: Quality::default(),
            devices: Vec::new(),
            rg_tagged: false,
            held: None,
            precheck_key: None,
        }
    }

    async fn run(mut self, mut commands: UnboundedReceiver<Cmd>) {
        loop {
            let ticking = self.fade.as_ref().is_some_and(|fade| fade.ready) && !self.paused;
            let deadline = self.fade.as_ref().map_or_else(Instant::now, |fade| {
                fade.last_tick + Duration::from_millis(100)
            });
            let result = tokio::select! {
                command = commands.recv() => match command {
                    Some(Cmd::Shutdown) | None => break,
                    Some(command) => self.command(command).await,
                },
                Some(message) = self.ipc_rx.recv() => self.message(message).await,
                _ = tokio::time::sleep_until(deadline), if ticking => self.tick_fade().await,
            };
            if let Err(error) = self.handle_result(result).await {
                self.emit(PlayerEvent::Error(format!("{error:#}")));
                let _ = self.stop_playback().await;
            }
        }
        if let Some(mpv) = &mut self.active {
            mpv.shutdown().await;
        }
        if let Some(mpv) = &mut self.standby {
            mpv.shutdown().await;
        }
    }

    // Both reader EOF and command transport errors converge here. Old instance
    // messages are ignored after replacement, so each death is handled only once.
    async fn handle_result(&mut self, mut result: Result<()>) -> Result<()> {
        while let Err(error) = result {
            let Some(disconnected) = error.downcast_ref::<Disconnected>() else {
                return Err(error);
            };
            result = self.restart(disconnected.instance).await;
        }
        Ok(())
    }

    async fn restart(&mut self, instance: u64) -> Result<()> {
        if !self.active.as_ref().is_some_and(|mpv| mpv.id == instance)
            && !self.standby.as_ref().is_some_and(|mpv| mpv.id == instance)
        {
            return Ok(());
        }
        self.reload(true).await
    }

    /// Replaces mpv with a fresh one at the same track, position and pause state.
    /// `crashed` counts against the restart budget and tells the user; a planned
    /// respawn (bit-perfect on or off) closes the old output first and stays quiet.
    async fn reload(&mut self, mut crashed: bool) -> Result<()> {
        // The reader may already have seen B start while the engine is still
        // waiting for A's EOF/playlist handshake (or handling a failed command).
        if let Some(mpv) = &self.active
            && let (Some(entry), position) = mpv.playback()
            && Some(entry) == self.pending_entry
            && let Some(index) = self.pending.and_then(|key| self.queue.index_of(key))
        {
            self.queue.current = Some(index);
            self.announce();
            self.pos = position.unwrap_or(0.0);
        }
        let position = self.start_at.take().unwrap_or(self.pos);
        let paused = self.paused;
        // Abandon both sides of a fade and all gapless handshakes before dropping
        // children. No stale event may advance the queue during recovery.
        self.fade = None;
        self.pending = None;
        self.pending_entry = None;
        self.gapless_eof = false;
        self.gapless_position = false;
        self.start_at = None;
        let old = [self.active.take(), self.standby.take()];
        if !crashed {
            // An exclusive device is only free once the old process is gone.
            for mut mpv in old.into_iter().flatten() {
                mpv.shutdown().await;
            }
        }
        self.quality.forget();
        let Some(track) = self.queue.track() else {
            return Ok(());
        };
        loop {
            if crashed {
                let now = Instant::now();
                self.restarts
                    .retain(|time| now.duration_since(*time) < Duration::from_secs(60));
                if self.restarts.len() >= 3 {
                    self.stop_playback().await?;
                    self.emit(PlayerEvent::Error("mpv keeps crashing".into()));
                    return Ok(());
                }
                self.restarts.push(now);
            }
            self.cache_paused = false;
            self.core_idle = true;
            self.update_buffering();
            let result: Result<()> = async {
                self.active =
                    Some(Mpv::spawn(self.ipc_tx.clone(), &self.launch(self.volume())).await?);
                self.active_send(json!(["set_property", "pause", paused]))
                    .await?;
                self.active
                    .as_mut()
                    .expect("replacement mpv")
                    .load(&crate::sources::mpv_url(&track.play_url), Some(position))
                    .await?;
                self.last_playhead = None;
                self.sync_prefetch().await
            }
            .await;
            match result {
                Ok(()) => {
                    if crashed {
                        self.emit(PlayerEvent::Error("Playback engine restarted".into()));
                    }
                    return Ok(());
                }
                // This attempt owns even a child that died before spawn returned.
                // Do not route it through the stale-instance guard in restart.
                Err(error) if error.is::<Disconnected>() => {
                    self.active = None;
                    self.standby = None;
                    self.pending = None;
                    self.pending_entry = None;
                    crashed = true;
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn emit(&self, event: PlayerEvent) {
        let _ = self.events.send(event);
    }
    fn queue_changed(&self) {
        self.emit(PlayerEvent::QueueChanged {
            tracks: self.queue.tracks(),
            keys: self.queue.keys(),
            current: self.queue.current,
        });
    }
    fn near_end(&mut self) {
        if self.config.repeat == RepeatMode::Off
            && self
                .queue
                .current
                .is_some_and(|i| i + 1 == self.queue.len())
            && self.queue.key() != self.near_key
        {
            self.near_key = self.queue.key();
            self.emit(PlayerEvent::NearQueueEnd);
        }
    }
    fn announce(&mut self) {
        self.pos = 0.0;
        self.played = 0.0;
        self.last_playhead = None;
        self.precheck_key = None;
        self.duration = self
            .queue
            .track()
            .and_then(|track| track.duration_secs)
            .unwrap_or(0.0);
        self.queue_changed();
        self.emit(PlayerEvent::NowPlaying(self.queue.track()));
        self.near_end();
    }

    async fn active_send(&mut self, command: Value) -> Result<()> {
        if let Some(mpv) = &mut self.active {
            mpv.send(command).await?;
        }
        Ok(())
    }
    async fn cancel_fade(&mut self) -> Result<()> {
        if self.fade.take().is_some() {
            if let Some(mpv) = &mut self.standby {
                mpv.send(json!(["stop"])).await?;
            }
            self.active_send(json!(["set_property", "volume", self.volume()]))
                .await?;
        }
        Ok(())
    }
    /// Ends the hold for `key`. True when `key` was held and is still current, so
    /// the app's answer applies; a stale answer is dropped.
    fn release(&mut self, key: u64) -> bool {
        let held = self.held == Some(key);
        if held {
            self.held = None;
        }
        held && self.queue.key() == Some(key)
    }
    async fn start_current(&mut self) -> Result<()> {
        self.held = None;
        self.announce();
        self.pos = self.start_at.unwrap_or(0.0);
        self.cancel_fade().await?;
        self.pending = None;
        self.gapless_eof = false;
        self.gapless_position = false;
        let Some(track) = self.queue.track() else {
            self.start_at = None;
            return self.stop_playback().await;
        };
        if self.active.is_none() {
            self.active = Some(Mpv::spawn(self.ipc_tx.clone(), &self.launch(self.volume())).await?);
        }
        self.cache_paused = false;
        self.emit(PlayerEvent::Paused(self.paused));
        self.active_send(json!(["set_property", "pause", self.paused]))
            .await?;
        self.active
            .as_mut()
            .expect("active mpv")
            .load(
                &crate::sources::mpv_url(&track.play_url),
                self.start_at.take(),
            )
            .await?;
        self.active_send(json!(["set_property", "volume", self.volume()]))
            .await?;
        self.sync_prefetch().await
    }
    async fn stop_playback(&mut self) -> Result<()> {
        self.held = None;
        self.cancel_fade().await?;
        self.pending = None;
        self.quality = Quality::default();
        self.queue.current = None;
        self.paused = true;
        self.pos = 0.0;
        self.duration = 0.0;
        self.buffering = false;
        self.queue_changed();
        self.emit(PlayerEvent::NowPlaying(None));
        self.emit(PlayerEvent::Paused(true));
        self.emit(PlayerEvent::Buffering(false));
        if let Some(mpv) = &mut self.active {
            mpv.entry_id = None;
        }
        self.active_send(json!(["stop"])).await
    }
    async fn sync_prefetch(&mut self) -> Result<()> {
        // A gapless transition in flight must finish first; it re-syncs itself
        // once the next entry is current, so the queue is not duplicated.
        if self.gapless_eof && !self.gapless_position {
            return Ok(());
        }
        self.pending = None;
        self.pending_entry = None;
        self.gapless_eof = false;
        self.gapless_position = false;
        if self.active.is_none() || self.queue.current.is_none() {
            return Ok(());
        }
        self.active_send(json!(["playlist-clear"])).await?;
        if self.config.gapless
            && !self.crossfading()
            && let Some(index) = self.queue.next(self.config.repeat, true)
        {
            let track = self.queue.at(index).expect("queue index");
            let url = crate::sources::mpv_url(&track.play_url);
            self.pending = self.queue.key_at(index);
            let mpv = self.active.as_mut().expect("active mpv");
            mpv.request(json!(["loadfile", url, "append"])).await?;
            let playlist = mpv.request(json!(["get_property", "playlist"])).await?;
            self.pending_entry = playlist
                .as_array()
                .and_then(|entries| entries.last())
                .and_then(|entry| entry["id"].as_i64());
        }
        Ok(())
    }
    async fn seek(&mut self, secs: f64, relative: bool) -> Result<()> {
        let secs = finite(secs);
        let target = if relative { self.pos + secs } else { secs }.max(0.0);
        let mode = if relative {
            "relative+exact"
        } else {
            "absolute+exact"
        };
        let result = async {
            self.cancel_fade().await?;
            self.active_send(json!(["seek", if relative { secs } else { target }, mode]))
                .await
        }
        .await;
        if result
            .as_ref()
            .is_err_and(|error| error.is::<Disconnected>())
        {
            self.pos = target;
        }
        result
    }
    async fn pause(&mut self, paused: bool) -> Result<()> {
        self.paused = paused;
        if let Some(fade) = &mut self.fade {
            fade.last_tick = Instant::now();
        }
        self.active_send(json!(["set_property", "pause", paused]))
            .await?;
        if self.fade.is_some()
            && let Some(mpv) = &mut self.standby
        {
            mpv.send(json!(["set_property", "pause", paused])).await?;
        }
        self.emit(PlayerEvent::Paused(paused));
        self.update_buffering();
        Ok(())
    }
    fn update_buffering(&mut self) {
        let buffering =
            !self.paused && self.queue.current.is_some() && (self.cache_paused || self.core_idle);
        if buffering != self.buffering {
            self.buffering = buffering;
            self.emit(PlayerEvent::Buffering(buffering));
        }
    }
    async fn command(&mut self, command: Cmd) -> Result<()> {
        match command {
            Cmd::Play(tracks, start) => {
                self.failures = 0;
                self.near_key = None;
                self.queue.replace(tracks, start);
                self.paused = false;
                self.start_current().await?;
            }
            Cmd::Restore(tracks, start, position) => {
                self.failures = 0;
                self.near_key = None;
                self.queue.replace(tracks, start);
                self.paused = true;
                self.start_at = Some(position).filter(|p| p.is_finite() && *p > 0.0);
                self.start_current().await?;
            }
            Cmd::Enqueue(tracks, next) => {
                self.cancel_fade().await?;
                self.queue.insert(tracks, next);
                self.queue_changed();
                self.sync_prefetch().await?;
            }
            Cmd::Remove(index) => {
                self.cancel_fade().await?;
                let removed_current = self.queue.remove(index);
                if removed_current {
                    self.start_current().await?;
                } else {
                    self.queue_changed();
                    self.sync_prefetch().await?;
                }
            }
            Cmd::Replace(key, track) => {
                if self.release(key)
                    && let Some(index) = self.queue.index_of(key)
                {
                    self.queue.set(index, track);
                    self.paused = false;
                    self.start_current().await?;
                } else if self.queue.key() != Some(key)
                    && let Some(index) = self.queue.index_of(key)
                {
                    self.queue.set(index, track);
                    self.queue_changed();
                    self.sync_prefetch().await?;
                }
            }
            Cmd::SkipFailed(key) => {
                if self.release(key) {
                    self.paused = false;
                    if self.failures >= self.queue.len().min(5) {
                        self.stop_playback().await?;
                    } else {
                        self.queue.current = self.queue.next(self.config.repeat, false);
                        self.start_current().await?;
                    }
                }
            }
            Cmd::Move(from, to) => {
                self.cancel_fade().await?;
                self.queue.move_track(from, to);
                self.queue_changed();
                self.sync_prefetch().await?;
            }
            Cmd::Clear => {
                self.queue.replace(Vec::new(), 0);
                self.stop_playback().await?;
            }
            Cmd::Jump(index) => {
                if index < self.queue.len() {
                    self.queue.current = Some(index);
                    self.failures = 0;
                    self.start_current().await?;
                }
            }
            Cmd::Next => {
                self.queue.current = self.queue.next(self.config.repeat, false);
                self.failures = 0;
                self.start_current().await?;
            }
            Cmd::Prev => {
                if self.pos > 3.0 {
                    self.seek(0.0, false).await?;
                } else {
                    self.queue.current = self.queue.previous(self.config.repeat);
                    self.start_current().await?;
                }
            }
            Cmd::TogglePause => self.pause(!self.paused).await?,
            Cmd::Pause(paused) => self.pause(paused).await?,
            Cmd::Stop => self.stop_playback().await?,
            Cmd::Seek(secs, relative) => self.seek(secs, relative).await?,
            Cmd::Volume(volume) => {
                self.config.volume = finite(volume).clamp(0.0, 100.0);
                if self.fade.is_some() {
                    self.apply_fade_volumes().await?;
                } else {
                    self.active_send(json!(["set_property", "volume", self.volume()]))
                        .await?;
                }
                self.emit(PlayerEvent::Volume(self.config.volume));
            }
            Cmd::Shuffle(on) => {
                self.cancel_fade().await?;
                self.queue.set_shuffle(on);
                self.config.shuffle = on;
                self.queue_changed();
                self.emit(PlayerEvent::Shuffle(on));
                self.sync_prefetch().await?;
            }
            Cmd::Repeat(mode) => {
                self.cancel_fade().await?;
                self.config.repeat = mode;
                self.emit(PlayerEvent::Repeat(mode));
                self.queue_changed();
                self.sync_prefetch().await?;
            }
            Cmd::Crossfade(secs) => {
                self.cancel_fade().await?;
                self.config.crossfade_secs = finite(secs).max(0.0);
                self.sync_prefetch().await?;
            }
            Cmd::Normalize(on) => {
                self.config.normalize = on;
                let filter = self.af();
                self.active_send(json!(["set_property", "af", filter.clone()]))
                    .await?;
                if let Some(mpv) = &mut self.standby {
                    mpv.send(json!(["set_property", "af", filter])).await?;
                }
            }
            Cmd::Equalizer(equalizer) => {
                if equalizer != self.config.equalizer {
                    let before = std::mem::replace(&mut self.config.equalizer, equalizer);
                    let rebuild = before.correction != self.config.equalizer.correction;
                    let commands = if self.config.bit_perfect {
                        Vec::new()
                    } else if rebuild {
                        vec![json!(["set_property", "af", self.af()])]
                    } else {
                        let old = before.commands();
                        self.config
                            .equalizer
                            .commands()
                            .into_iter()
                            .zip(old)
                            .filter(|(new, old)| new != old)
                            .map(|(new, _)| new)
                            .collect()
                    };
                    for command in commands {
                        self.active_send(command.clone()).await?;
                        if let Some(mpv) = &mut self.standby {
                            mpv.send(command).await?;
                        }
                    }
                }
            }
            Cmd::Gapless(on) => {
                self.config.gapless = on;
                self.sync_prefetch().await?;
            }
            Cmd::AudioDevice(name) => {
                if name != self.config.audio_device {
                    self.config.audio_device = name;
                    let device = match self.config.audio_device.as_str() {
                        "" => "auto".to_string(),
                        name => name.to_string(),
                    };
                    self.active_send(json!(["set_property", "audio-device", device.clone()]))
                        .await?;
                    if let Some(mpv) = &mut self.standby {
                        mpv.send(json!(["set_property", "audio-device", device]))
                            .await?;
                    }
                }
            }
            Cmd::ReplayGain(on) => {
                if on != self.config.replaygain {
                    self.config.replaygain = on;
                    if !self.config.bit_perfect {
                        let mode = if on { "album" } else { "no" };
                        self.active_send(json!(["set_property", "replaygain", mode]))
                            .await?;
                        if let Some(mpv) = &mut self.standby {
                            mpv.send(json!(["set_property", "replaygain", mode]))
                                .await?;
                        }
                        if self.config.normalize && self.rg_tagged {
                            self.active_send(json!(["set_property", "af", self.af()]))
                                .await?;
                        }
                    }
                }
            }
            Cmd::BitPerfect(on) => {
                if on != self.config.bit_perfect {
                    self.config.bit_perfect = on;
                    self.reload(false).await?;
                }
            }
            Cmd::Devices => {
                let events = self.events.clone();
                tokio::spawn(async move {
                    let _ = events.send(PlayerEvent::Devices(mpv::audio_devices().await));
                });
            }
            Cmd::YtdlOptions => {
                let options = crate::sources::ytdl_raw_options();
                self.active_send(json!(["set_property", "ytdl-raw-options", options.clone()]))
                    .await?;
                if let Some(mpv) = &mut self.standby {
                    mpv.send(json!(["set_property", "ytdl-raw-options", options]))
                        .await?;
                }
            }
            Cmd::RefreshPrefetch => {
                self.sync_prefetch().await?;
            }
            Cmd::Shutdown => {}
        }
        self.near_end();
        Ok(())
    }
    /// the app fallback, anything else is reported and skipped.
    async fn failed_load(&mut self, file_error: &str) -> Result<()> {
        self.failures += 1;
        self.pending = None;
        if let Some(track) = self.queue.track() {
            crate::sources::resolve::forget(&track.play_url);
        }
        if let (Some(key), Some(track)) = (self.queue.key(), self.queue.track())
            && crate::sources::is_youtube_url(&track.play_url)
        {
            self.pending_entry = None;
            if let Some(mpv) = &mut self.active {
                mpv.entry_id = None;
            }
            self.held = Some(key);
            self.active_send(json!(["stop"])).await?;
            self.buffering = true;
            self.emit(PlayerEvent::Buffering(true));
            self.emit(PlayerEvent::LoadFailed {
                key,
                track,
                error: file_error.to_string(),
            });
            return Ok(());
        }
        self.emit(PlayerEvent::Error(format!(
            "mpv could not load {}: {}",
            self.queue.track().map_or_else(String::new, |t| t.title),
            file_error
        )));
        if self.failures >= self.queue.len().min(5) {
            self.stop_playback().await?;
        } else {
            self.queue.current = self.queue.next(self.config.repeat, false);
            self.start_current().await?;
        }
        Ok(())
    }

    async fn message(&mut self, message: Message) -> Result<()> {
        let active = self
            .active
            .as_ref()
            .is_some_and(|mpv| mpv.id == message.instance);
        let value = message.value;
        if value["event"] == "ipc-error" {
            return self.restart(message.instance).await;
        }
        if !active {
            if self
                .standby
                .as_ref()
                .is_some_and(|mpv| mpv.id == message.instance)
                && self.fade.is_some()
            {
                match value["event"].as_str() {
                    Some("file-loaded") => {
                        if let Some(fade) = &mut self.fade {
                            fade.ready = true;
                            fade.last_tick = Instant::now();
                        }
                    }
                    Some("property-change") => {
                        if let Some(fade) = &mut self.fade {
                            match value["name"].as_str() {
                                Some("time-pos") => {
                                    fade.position = value["data"].as_f64().unwrap_or(fade.position)
                                }
                                Some("duration") => {
                                    fade.track_duration =
                                        value["data"].as_f64().unwrap_or(fade.track_duration)
                                }
                                _ => {}
                            }
                        }
                    }
                    Some("end-file") if value["reason"] == "error" => {
                        self.emit(PlayerEvent::Error(format!(
                            "crossfade load failed: {}",
                            value["file_error"]
                        )));
                        self.cancel_fade().await?;
                    }

                    _ => {}
                }
            }
            return Ok(());
        }
        let entry = self.active.as_ref().and_then(|mpv| mpv.entry_id);
        if value["event"] == "end-file"
            && value["reason"] == "error"
            && self
                .pending_entry
                .is_some_and(|id| value["playlist_entry_id"].as_i64() == Some(id))
        {
            // A prefetched entry failed before becoming the current one. During
            // the transition, finish the handshake so the failure is handled for
            // the song that actually failed; earlier, while the current song
            // still plays, drop the dead prefetch and let the app fix the entry
            // before the gap.
            if self.gapless_eof && !self.gapless_position {
                self.queue.current = self
                    .pending
                    .take()
                    .and_then(|key| self.queue.index_of(key));
                if let Some(mpv) = &mut self.active {
                    mpv.entry_id = self.pending_entry.take();
                }
                self.announce();
                self.failed_load(value["file_error"].as_str().unwrap_or("load failed"))
                    .await?;
            } else {
                let key = self.pending.take();
                self.pending_entry = None;
                self.active_send(json!(["playlist-clear"])).await?;
                let track = key
                    .and_then(|key| self.queue.index_of(key))
                    .and_then(|index| self.queue.at(index).cloned());
                if let (Some(key), Some(track)) = (key, track) {
                    crate::sources::resolve::forget(&track.play_url);
                    self.emit(PlayerEvent::LoadFailed {
                        key,
                        track,
                        error: value["file_error"].to_string(),
                    });
                }
            }
            return Ok(());
        }
        if value["event"] == "end-file"
            && (entry.is_none() || value["playlist_entry_id"].as_i64() != entry)
        {
            return Ok(());
        }
        if (value["event"] == "file-loaded"
            || (value["event"] == "property-change"
                && matches!(
                    value["name"].as_str(),
                    Some("time-pos" | "duration" | "pause")
                )))
            && (entry.is_none() || value["playback_entry_id"].as_i64() != entry)
        {
            return Ok(());
        }
        match value["event"].as_str() {
            Some("property-change") => match value["name"].as_str() {
                Some("time-pos") => {
                    if let Some(pos) = value["data"].as_f64() {
                        if !self.paused {
                            if let Some(previous) = self.last_playhead {
                                let delta = pos - previous;
                                if delta > 0.0 && delta <= 1.0 {
                                    self.played += delta;
                                }
                            }
                            if self.played > 2.0 {
                                self.failures = 0;
                            }
                        }
                        self.last_playhead = Some(pos);
                        self.pos = pos;
                        if !self.paused
                            && self.queue.current.is_some()
                            && self.last_position.elapsed() >= Duration::from_millis(250)
                        {
                            self.last_position = Instant::now();
                            self.emit(PlayerEvent::Position {
                                pos,
                                duration: self.duration,
                            });
                        }
                        if !self.paused
                            && self.duration > 60.0
                            && pos > self.duration - 45.0
                            && let Some(current) = self.queue.key()
                            && self.precheck_key != Some(current)
                            && let Some(index) = self.queue.next(self.config.repeat, true)
                            && let Some(key) = self.queue.key_at(index)
                            && key != current
                            && let Some(track) = self.queue.at(index).cloned()
                        {
                            self.precheck_key = Some(current);
                            self.emit(PlayerEvent::CheckNext { key, track });
                        }
                        self.maybe_fade().await?;
                    }
                }
                Some("duration") => {
                    if let Some(duration) = value["data"].as_f64() {
                        self.duration = duration;
                    }
                }
                Some("pause") => {
                    if let Some(paused) = value["data"].as_bool() {
                        if paused != self.paused {
                            self.paused = paused;
                            if let Some(fade) = &mut self.fade {
                                fade.last_tick = Instant::now();
                            }
                            self.emit(PlayerEvent::Paused(paused));
                        }
                        self.update_buffering();
                    }
                }
                Some("paused-for-cache") => {
                    self.cache_paused = value["data"].as_bool().unwrap_or(false);
                    self.update_buffering();
                }
                Some("core-idle") => {
                    self.core_idle = value["data"].as_bool().unwrap_or(true);
                    self.update_buffering();
                }
                Some("volume") => {
                    // Bit-perfect pins mpv at 100; the user's level waits for its return.
                    if self.fade.is_none()
                        && !self.config.bit_perfect
                        && let Some(volume) = value["data"].as_f64()
                        && (volume - self.config.volume).abs() > 0.01
                    {
                        self.config.volume = volume.clamp(0.0, 100.0);
                        self.emit(PlayerEvent::Volume(self.config.volume));
                    }
                }
                Some("playlist")
                    if self.pending_entry.is_some()
                        && value["data"].as_array().is_some_and(|entries| {
                            entries.iter().any(|entry| {
                                entry["current"] == true
                                    && entry["id"].as_i64() == self.pending_entry
                            })
                        }) =>
                {
                    self.gapless_position = true;
                    self.complete_gapless().await?;
                }
                Some("metadata") => self.replaygain_tags(&value["data"]).await?,
                Some("audio-device-list") => {
                    let devices = mpv::devices_from_list(&value["data"]);
                    if !devices.is_empty() && devices != self.devices {
                        self.devices = devices.clone();
                        self.emit(PlayerEvent::Devices(devices));
                    }
                }
                Some(name) => {
                    if self.queue.current.is_some()
                        && let Some(tag) = self.quality.update(name, &value["data"])
                    {
                        self.emit(PlayerEvent::Quality(tag));
                    }
                }
                None => {}
            },
            Some("start-file") => self.quality.reset(),
            Some("file-loaded") => {}
            Some("end-file") if value["reason"] == "eof" => {
                if self.fade.is_some() {
                    self.finish_fade().await?;
                } else if self.queue.current.is_some() {
                    if let Some(track) = self.queue.track() {
                        self.emit(PlayerEvent::TrackFinished(track));
                    }
                    if self.pending.is_some() {
                        self.gapless_eof = true;
                        self.complete_gapless().await?;
                    } else {
                        self.queue.current = self.queue.next(self.config.repeat, true);
                        self.start_current().await?;
                    }
                }
            }
            Some("end-file") if value["reason"] == "error" => {
                self.failed_load(value["file_error"].as_str().unwrap_or("load failed"))
                    .await?;
            }

            _ => {
                if let Some(error) = value["error"].as_str()
                    && error != "success"
                {
                    self.emit(PlayerEvent::Error(format!("mpv: {error}")));
                }
            }
        }
        Ok(())
    }

    async fn complete_gapless(&mut self) -> Result<()> {
        if self.gapless_eof && self.gapless_position {
            self.queue.current = self.pending.take().and_then(|key| self.queue.index_of(key));
            if let Some(mpv) = &mut self.active {
                mpv.entry_id = self.pending_entry.take();
            }
            self.announce();
            self.sync_prefetch().await?;
        }
        Ok(())
    }
    /// The filter chain for the playing file. Bit-perfect has none. When a file has
    /// ReplayGain tags and ReplayGain is on, its static gain replaces dynaudnorm:
    /// the mastered dynamics stay intact and loud and quiet songs still match.
    /// Untagged files keep the dynamic normaliser.
    fn af(&self) -> String {
        if self.config.bit_perfect {
            return String::new();
        }
        let normalize = self.config.normalize && !(self.config.replaygain && self.rg_tagged);
        filters(normalize, &self.config.equalizer)
    }
    fn launch(&self, volume: f64) -> Launch {
        Launch {
            volume,
            af: self.af(),
            device: self.config.audio_device.clone(),
            replaygain: self.config.replaygain,
            bit_perfect: self.config.bit_perfect,
        }
    }
    /// What mpv's volume is set to. Bit-perfect leaves the level to the device.
    fn volume(&self) -> f64 {
        if self.config.bit_perfect {
            100.0
        } else {
            self.config.volume
        }
    }
    /// Two streams can't share an exclusive device, and mixing isn't bit-perfect.
    fn crossfading(&self) -> bool {
        self.config.crossfade_secs > 0.0 && !self.config.bit_perfect
    }
    async fn replaygain_tags(&mut self, metadata: &Value) -> Result<()> {
        if metadata.is_null() {
            return Ok(());
        }
        let tagged = quality::has_replaygain(metadata);
        if tagged != self.rg_tagged {
            let before = self.af();
            self.rg_tagged = tagged;
            let after = self.af();
            if after != before {
                self.active_send(json!(["set_property", "af", after]))
                    .await?;
            }
        }
        Ok(())
    }
    /// After a crossfade the new active mpv reported its format while it was the
    /// standby, so ask again.
    async fn refresh_quality(&mut self) -> Result<()> {
        self.quality.forget();
        for name in [
            "audio-codec-name",
            "file-format",
            "audio-params",
            "current-tracks/audio",
            "audio-bitrate",
            "metadata",
        ] {
            let Some(mpv) = &mut self.active else {
                return Ok(());
            };
            let data = match mpv.request(json!(["get_property", name])).await {
                Ok(data) => data,
                Err(error) if error.is::<Disconnected>() => return Err(error),
                Err(_) => continue,
            };
            if name == "metadata" {
                self.replaygain_tags(&data).await?;
            } else if let Some(tag) = self.quality.update(name, &data) {
                self.emit(PlayerEvent::Quality(tag));
            }
        }
        Ok(())
    }

    async fn maybe_fade(&mut self) -> Result<()> {
        let remaining = self.duration - self.pos;
        if !self.crossfading()
            || self.fade.is_some()
            || self.paused
            || self.duration <= 0.0
            || remaining <= 0.0
            || remaining > self.config.crossfade_secs
        {
            return Ok(());
        }
        let Some(index) = self.queue.next(self.config.repeat, true) else {
            return Ok(());
        };
        let Some(track) = self.queue.at(index) else {
            return Ok(());
        };
        let url = crate::sources::mpv_url(&track.play_url);
        let track_duration = track.duration_secs.unwrap_or(0.0);
        let next = self.queue.key_at(index).expect("queue index");
        if self.standby.is_none() {
            self.standby = Some(Mpv::spawn(self.ipc_tx.clone(), &self.launch(0.0)).await?);
        }
        let mpv = self.standby.as_mut().expect("standby mpv");
        mpv.send(json!(["set_property", "volume", 0])).await?;
        mpv.load(&url, None).await?;
        mpv.send(json!(["set_property", "pause", false])).await?;
        self.fade = Some(Fade {
            next,
            duration: remaining,
            elapsed: 0.0,
            last_tick: Instant::now(),
            ready: false,
            position: 0.0,
            track_duration,
        });
        Ok(())
    }
    async fn apply_fade_volumes(&mut self) -> Result<()> {
        let Some(fade) = &self.fade else {
            return Ok(());
        };
        let fraction = (fade.elapsed / fade.duration).clamp(0.0, 1.0);
        self.active_send(json!([
            "set_property",
            "volume",
            self.volume() * (1.0 - fraction)
        ]))
        .await?;
        let volume = self.volume() * fraction;
        if let Some(mpv) = &mut self.standby {
            mpv.send(json!(["set_property", "volume", volume])).await?;
        }
        Ok(())
    }
    async fn tick_fade(&mut self) -> Result<()> {
        if let Some(fade) = &mut self.fade {
            let now = Instant::now();
            fade.elapsed += (now - fade.last_tick).as_secs_f64();
            fade.last_tick = now;
        }
        self.apply_fade_volumes().await?;
        if self
            .fade
            .as_ref()
            .is_some_and(|fade| fade.elapsed >= fade.duration)
        {
            self.finish_fade().await?;
        }
        Ok(())
    }
    async fn finish_fade(&mut self) -> Result<()> {
        let Some(fade) = self.fade.take() else {
            return Ok(());
        };
        if let Some(track) = self.queue.track() {
            self.emit(PlayerEvent::TrackFinished(track));
        }
        self.active_send(json!(["stop"])).await?;
        std::mem::swap(&mut self.active, &mut self.standby);
        self.queue.current = self.queue.index_of(fade.next);
        self.announce();
        self.pos = fade.position;
        self.duration = fade.track_duration;
        self.active_send(json!(["set_property", "volume", self.volume()]))
            .await?;
        self.refresh_quality().await?;
        self.sync_prefetch().await
    }
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}
