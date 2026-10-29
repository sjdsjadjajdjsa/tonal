use crate::model::{RepeatMode, Track};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    QueueChanged {
        tracks: Vec<Track>,
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
}

#[derive(Clone, Debug)]
pub struct PlayerConfig {
    pub volume: f64,
    pub crossfade_secs: f64,
    pub normalize: bool,
    pub gapless: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

mod mpv;
mod queue;
#[cfg(test)]
mod tests;

use anyhow::Result;
use mpv::{Disconnected, Message, Mpv};
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
    Gapless(bool),
    YtdlOptions,
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
    pub fn refresh_ytdl_options(&self) {
        let _ = self.tx.send(Cmd::YtdlOptions);
    }
    pub fn set_gapless(&self, on: bool) {
        let _ = self.tx.send(Cmd::Gapless(on));
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
        self.active = None;
        self.standby = None;
        let Some(track) = self.queue.track() else {
            return Ok(());
        };
        loop {
            let now = Instant::now();
            self.restarts
                .retain(|time| now.duration_since(*time) < Duration::from_secs(60));
            if self.restarts.len() >= 3 {
                self.stop_playback().await?;
                self.emit(PlayerEvent::Error("mpv keeps crashing".into()));
                return Ok(());
            }
            self.restarts.push(now);
            self.cache_paused = false;
            self.core_idle = true;
            self.update_buffering();
            let result: Result<()> = async {
                self.active = Some(
                    Mpv::spawn(
                        self.ipc_tx.clone(),
                        self.config.volume,
                        self.config.normalize,
                    )
                    .await?,
                );
                self.active_send(json!(["set_property", "pause", paused]))
                    .await?;
                self.active
                    .as_mut()
                    .expect("replacement mpv")
                    .load(&track.play_url, Some(position))
                    .await?;
                self.last_playhead = None;
                self.sync_prefetch().await
            }
            .await;
            match result {
                Ok(()) => {
                    self.emit(PlayerEvent::Error("Playback engine restarted".into()));
                    return Ok(());
                }
                // This attempt owns even a child that died before spawn returned.
                // Do not route it through the stale-instance guard in restart.
                Err(error) if error.is::<Disconnected>() => {
                    self.active = None;
                    self.standby = None;
                    self.pending = None;
                    self.pending_entry = None;
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
            self.active_send(json!(["set_property", "volume", self.config.volume]))
                .await?;
        }
        Ok(())
    }
    async fn start_current(&mut self) -> Result<()> {
        // Commit the target track/position before any transport operation, so a
        // failed cancellation, spawn, pause or load recovers the requested track.
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
            self.active = Some(
                Mpv::spawn(
                    self.ipc_tx.clone(),
                    self.config.volume,
                    self.config.normalize,
                )
                .await?,
            );
        }
        self.cache_paused = false;
        self.emit(PlayerEvent::Paused(self.paused));
        // Apply pause before loading: a paused replacement must never start audibly.
        self.active_send(json!(["set_property", "pause", self.paused]))
            .await?;
        self.active
            .as_mut()
            .expect("active mpv")
            .load(&track.play_url, self.start_at.take())
            .await?;
        self.active_send(json!(["set_property", "volume", self.config.volume]))
            .await?;
        self.sync_prefetch().await
    }
    async fn stop_playback(&mut self) -> Result<()> {
        self.cancel_fade().await?;
        self.pending = None;
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
        self.pending = None;
        self.pending_entry = None;
        self.gapless_eof = false;
        self.gapless_position = false;
        if self.active.is_none() || self.queue.current.is_none() {
            return Ok(());
        }
        self.active_send(json!(["playlist-clear"])).await?;
        if self.config.gapless
            && self.config.crossfade_secs == 0.0
            && let Some(index) = self.queue.next(self.config.repeat, true)
        {
            let track = self.queue.at(index).expect("queue index");
            let url = track.play_url.clone();
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
                    self.active_send(json!(["set_property", "volume", self.config.volume]))
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
                let filter = if on { "dynaudnorm=f=250:g=15" } else { "" };
                self.active_send(json!(["set_property", "af", filter]))
                    .await?;
                if let Some(mpv) = &mut self.standby {
                    mpv.send(json!(["set_property", "af", filter])).await?;
                }
            }
            Cmd::Gapless(on) => {
                self.config.gapless = on;
                self.sync_prefetch().await?;
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
            Cmd::Shutdown => {}
        }
        self.near_end();
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
                    if self.fade.is_none()
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
                _ => {}
            },
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
                self.emit(PlayerEvent::Error(format!(
                    "mpv could not load {}: {}",
                    self.queue.track().map_or_else(String::new, |t| t.title),
                    value["file_error"]
                )));
                self.failures += 1;
                self.pending = None;
                if self.failures >= self.queue.len() {
                    self.stop_playback().await?;
                } else {
                    self.queue.current = self.queue.next(self.config.repeat, false);
                    self.start_current().await?;
                }
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
    async fn maybe_fade(&mut self) -> Result<()> {
        let remaining = self.duration - self.pos;
        if self.config.crossfade_secs <= 0.0
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
        let url = track.play_url.clone();
        let track_duration = track.duration_secs.unwrap_or(0.0);
        let next = self.queue.key_at(index).expect("queue index");
        if self.standby.is_none() {
            self.standby = Some(Mpv::spawn(self.ipc_tx.clone(), 0.0, self.config.normalize).await?);
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
            self.config.volume * (1.0 - fraction)
        ]))
        .await?;
        if let Some(mpv) = &mut self.standby {
            mpv.send(json!([
                "set_property",
                "volume",
                self.config.volume * fraction
            ]))
            .await?;
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
        self.active_send(json!(["set_property", "volume", self.config.volume]))
            .await?;
        self.sync_prefetch().await
    }
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}
