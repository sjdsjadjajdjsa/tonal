use super::{controller::Controller, models};
use crate::{
    model::{SourceKind, Track},
    player::PlayerEvent,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

impl Controller {
    pub fn playable(&self, tracks: &mut [Track]) {
        for t in tracks {
            if let Some(path) = self.library.download_path(&t.id).filter(|p| p.is_file()) {
                if t.play_url != path.to_string_lossy() {
                    self.result(self.library.upsert_tracks(std::slice::from_ref(t)));
                }
                t.play_url = path.to_string_lossy().into_owned();
            }
        }
    }
    pub fn play(&self, index: usize) {
        let mut tracks = {
            let mut s = self.state.lock();
            s.restore_position = None;
            s.display.clone()
        };
        self.playable(&mut tracks);
        if index < tracks.len() {
            self.player.play_tracks(tracks, index);
        }
    }
    pub(super) fn radio(&self, track: Track) {
        let this = self.clone();
        tokio::spawn(async move {
            if let Some(mut tracks) = this.result(crate::sources::radio(&track, 20).await) {
                {
                    let s = this.state.lock();
                    tracks.retain(|t| !s.queue.iter().any(|q| q.id == t.id));
                }
                this.playable(&mut tracks);
                this.player.enqueue(tracks);
            }
        });
    }
    pub fn seek(&self, position: f64) {
        self.player.seek(position);
        let paused = self.state.lock().paused;
        self.media.set_playback(!paused, position);
    }
    pub fn event(&self, event: PlayerEvent) {
        match event {
            PlayerEvent::QueueChanged {
                tracks,
                keys,
                current,
            } => {
                {
                    let mut s = self.state.lock();
                    s.queue = tracks.clone();
                    s.queue_keys = keys;
                    s.index = current;
                }
                let library = self.library.clone();
                let queue = tracks.clone();
                self.ui(move |w| {
                    models::update(w.get_queue(), models::rows(&library, &queue));
                    w.set_queue_current(current.map(|i| i as i32).unwrap_or(-1));
                });
                self.artwork(tracks, true);
            }
            PlayerEvent::NowPlaying(track) => {
                if let Some(t) = &track {
                    log::info!("Now playing: {} — {}", t.artist, t.title);
                    if t.source == SourceKind::YouTube {
                        self.check_youtube_runtime();
                    }
                }
                let initial_position = {
                    let mut s = self.state.lock();
                    s.current = track.clone();
                    s.position = s.restore_position.take().unwrap_or(0.0);
                    s.played = 0.0;
                    s.started = now();
                    s.scrobbled = false;
                    s.lyrics = None;
                    s.lyric_index = None;
                    s.position
                };
                let initial_duration = track.as_ref().and_then(|t| t.duration_secs).unwrap_or(1.0);
                self.media.set_track(track.as_ref());
                self.media
                    .set_playback(!self.state.lock().paused, initial_position);
                let liked = track.as_ref().is_some_and(|t| self.library.is_liked(&t.id));
                let title = track.as_ref().map(|t| t.title.clone()).unwrap_or_default();
                let artist = track.as_ref().map(|t| t.artist.clone()).unwrap_or_default();
                let now_id = track.as_ref().map(|t| t.id.clone()).unwrap_or_default();
                let has_track = track.is_some();
                let now_artists: Vec<slint::SharedString> = track
                    .as_ref()
                    .map(|t| t.artists().into_iter().map(|a| a.name.into()).collect())
                    .unwrap_or_default();
                let has_album = track.as_ref().is_some_and(|t| t.album_ref().is_some());
                self.ui(move |w| {
                    w.set_now_id(now_id.into());
                    if !has_track {
                        super::waves::set_playing(&w, false);
                        w.set_now_quality(Default::default());
                    }
                    w.set_position(initial_position as f32);
                    w.set_elapsed(models::time(initial_position).into());
                    w.set_duration(initial_duration as f32);
                    w.set_total(models::time(initial_duration).into());
                    w.set_now_title(title.into());
                    w.set_now_artist(artist.into());
                    w.set_now_artists(slint::ModelRc::new(slint::VecModel::from(now_artists)));
                    w.set_now_album(has_album);
                    w.set_now_liked(liked);
                    w.set_now_art(slint::Image::default());
                    w.set_lyric_index(-1);
                    models::update(w.get_lyrics(), models::lyrics(None, None));
                });
                if let Some(t) = track {
                    let canonical = if self.library.download_path(&t.id).is_some() {
                        self.library
                            .downloads()
                            .ok()
                            .and_then(|tracks| tracks.into_iter().find(|track| track.id == t.id))
                            .unwrap_or_else(|| t.clone())
                    } else {
                        t.clone()
                    };
                    self.result(self.library.record_play(&canonical));
                    self.result(self.library.upsert_tracks(std::slice::from_ref(&canonical)));
                    let this = self.clone();
                    let artwork = t.clone();
                    tokio::spawn(async move {
                        if let Some(url) = artwork.artwork_url
                            && let Ok(pixels) = crate::artwork::load(&url, 128).await
                        {
                            let check = this.state.clone();
                            let id = artwork.id;
                            this.ui(move |w| {
                                if check.lock().current.as_ref().is_some_and(|t| t.id == id) {
                                    w.set_now_art(slint::Image::from_rgba8(pixels));
                                }
                            });
                        }
                    });
                    let this = self.clone();
                    tokio::spawn(async move {
                        let scrobbler = this.scrobbler.lock().clone();
                        this.result(scrobbler.now_playing(&t).await);
                        let lyrics = match crate::integrations::lyrics::fetch(&t).await {
                            Ok(l) => l,
                            Err(e) => {
                                log::debug!("Lyrics: {e}");
                                None
                            }
                        };
                        {
                            let mut s = this.state.lock();
                            if s.current.as_ref().map(|t| &t.id) != Some(&t.id) {
                                return;
                            }
                            s.lyrics = lyrics.clone();
                        }
                        let rows = models::lyrics(lyrics.as_ref(), None);
                        this.ui(move |w| models::update(w.get_lyrics(), rows));
                    });
                }
            }
            PlayerEvent::Position { pos, duration } => {
                let (line, scrobble, session) = {
                    let mut s = self.state.lock();
                    if !s.paused {
                        let delta = pos - s.position;
                        if (0.0..=2.0).contains(&delta) {
                            s.played += delta;
                        }
                    }
                    s.position = pos;
                    s.duration = duration;
                    let active = s
                        .lyrics
                        .as_ref()
                        .and_then(|l| l.synced.iter().rposition(|(time, _)| *time <= pos));
                    let line = if active != s.lyric_index {
                        s.lyric_index = active;
                        Some((models::lyrics(s.lyrics.as_ref(), active), active))
                    } else {
                        None
                    };
                    let scrobble = if !s.scrobbled
                        && crate::integrations::scrobble::should_scrobble(s.played, duration)
                    {
                        s.scrobbled = true;
                        s.current
                            .clone()
                            .map(|t| (t, s.started, s.settings.clone()))
                    } else {
                        None
                    };
                    let session = if !s.paused && s.saved.elapsed() >= Duration::from_secs(30) {
                        s.saved = Instant::now();
                        Some(s.session())
                    } else {
                        None
                    };
                    (line, scrobble, session)
                };
                if let Some(session) = session {
                    self.result(self.library.save_session(&session));
                }
                if let Some((t, started, settings)) = scrobble {
                    let this = self.clone();
                    tokio::spawn(async move {
                        this.result(
                            crate::integrations::scrobble::Scrobbler::new(&settings)
                                .scrobble(&t, started)
                                .await,
                        );
                    });
                }
                self.ui(move |w| {
                    let elapsed = models::time(pos);
                    if w.get_elapsed().as_str() != elapsed {
                        w.set_elapsed(elapsed.into());
                        w.set_position(pos as f32);
                    }
                    if w.get_duration() != duration.max(1.0) as f32 {
                        w.set_duration(duration.max(1.0) as f32);
                    }
                    let total = models::time(duration);
                    if w.get_total().as_str() != total {
                        w.set_total(total.into());
                    }
                    if let Some((rows, index)) = line {
                        models::update(w.get_lyrics(), rows);
                        w.set_lyric_index(index.map(|i| i as i32).unwrap_or(-1));
                    }
                });
            }
            PlayerEvent::Paused(paused) => {
                let pos = {
                    let mut s = self.state.lock();
                    s.paused = paused;
                    s.position
                };
                self.media.set_playback(!paused, pos);
                self.ui(move |w| {
                    if w.get_paused() != paused {
                        w.set_paused(paused);
                    }
                    super::waves::set_playing(&w, !paused);
                });
            }
            PlayerEvent::Volume(volume) => {
                {
                    self.state.lock().settings.volume = volume;
                }
                self.media.set_volume(volume);
                self.ui(move |w| {
                    if w.get_volume() != volume as f32 {
                        w.set_volume(volume as f32);
                    }
                });
            }
            PlayerEvent::Shuffle(shuffle) => {
                self.state.lock().settings.shuffle = shuffle;
                self.ui(move |w| w.set_shuffle(shuffle));
            }
            PlayerEvent::Repeat(repeat) => {
                self.state.lock().settings.repeat = repeat;
                self.ui(move |w| w.set_repeat(format!("{repeat:?}").into()));
            }
            PlayerEvent::Buffering(buffering) => self.ui(move |w| {
                w.set_buffering(buffering);
            }),
            PlayerEvent::NearQueueEnd => {
                let seed = {
                    let s = self.state.lock();
                    if s.settings.autoplay && s.restore_position.is_none() {
                        s.current.clone()
                    } else {
                        None
                    }
                };
                if let Some(t) = seed {
                    self.radio(t);
                }
            }
            PlayerEvent::TrackFinished(track) => {
                log::debug!("Finished {}", track.id);
            }
            PlayerEvent::Quality(tag) => self.ui(move |w| w.set_now_quality(tag.into())),
            PlayerEvent::Devices(devices) => {
                if models::set_devices(devices) {
                    let settings = self.state.lock().settings.clone();
                    self.ui(move |w| models::update(w.get_settings(), models::settings(&settings)));
                }
            }
            PlayerEvent::Error(error) => match error
                .strip_prefix("mpv could not load ")
                .and_then(|rest| rest.split_once(": "))
            {
                Some((title, _)) => {
                    self.status(format!("Couldn't play {title}"));
                }
                None => self.status(error),
            },
            PlayerEvent::LoadFailed { key, track, error } => self.recover(key, track, error),
        }
    }
    /// True while queue entry `key` is still the current song. The entry is found
    /// by its stable key, so moving it or toggling shuffle does not lose it.
    fn is_current(&self, key: u64) -> bool {
        let s = self.state.lock();
        let index = s.queue_keys.iter().position(|k| *k == key);
        index.is_some() && s.index == index
    }
    /// A song failed to load. The queue holds on it while every source is
    /// searched at once; the first playable copy plays in its place, and the
    /// song is skipped only once every source has come up empty.
    fn recover(&self, key: u64, track: Track, error: String) {
        log::warn!("Load failed for {}: {error}", track.title);
        let tried = self.state.lock().fallback_tried.contains(&track.id);
        let this = self.clone();
        tokio::spawn(async move {
            if tried {
                if this.is_current(key) {
                    this.status(format!("Couldn't play {}", track.title));
                }
                this.player.skip_failed(key);
                return;
            }
            match this.find_copy(&track).await {
                Some(found) => this.apply_fallback(key, &track, found),
                None => {
                    if this.is_current(key) {
                        this.status(format!("Couldn't play {}", track.title));
                    }
                    this.player.skip_failed(key);
                }
            }
        });
    }

    /// Finds a playable copy of `track`. YouTube's own retry (new access
    /// strategy) and the SoundCloud and Audius searches all start at once.
    /// YouTube is preferred: a copy found elsewhere waits up to
    /// `YOUTUBE_PREFERENCE` for the retry before it plays instead.
    async fn find_copy(&self, track: &Track) -> Option<Track> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(bool, Option<Track>)>();
        let mut youtube_running = self.start_youtube_retry(track, &tx);
        self.start_searches(track, &tx);
        drop(tx);
        let mut other: Option<Track> = None;
        loop {
            let next = if other.is_some() {
                match tokio::time::timeout(Self::YOUTUBE_PREFERENCE, rx.recv()).await {
                    Ok(next) => next,
                    Err(_) => return other,
                }
            } else {
                rx.recv().await
            };
            match next {
                None => return other,
                Some((true, Some(found))) => return Some(found),
                Some((true, None)) => youtube_running = false,
                Some((false, found)) => {
                    if other.is_none() {
                        other = found;
                    }
                }
            }
            if !youtube_running && other.is_some() {
                return other;
            }
        }
    }

    /// How long a copy found on another source waits for YouTube's retry.
    const YOUTUBE_PREFERENCE: std::time::Duration = std::time::Duration::from_secs(12);

    /// Once per song, looks for a YouTube access strategy that loads it again.
    /// Sends exactly one `(true, _)` answer; false when no retry was started.
    fn start_youtube_retry(&self, track: &Track, tx: &CopySender) -> bool {
        let id = format!("unblock:{}", track.id);
        {
            let mut s = self.state.lock();
            if !crate::sources::is_youtube_url(&track.play_url) || !s.fallback_tried.insert(id) {
                return false;
            }
        }
        let tx = tx.clone();
        let original = track.clone();
        tokio::spawn(async move {
            let found = match crate::sources::unblock_youtube(&original.play_url).await {
                Ok(how) => {
                    log::info!("YouTube retry for {} ({how})", original.title);
                    Some(original)
                }
                Err(why) => {
                    log::info!("YouTube stays blocked for {}: {why}", original.title);
                    None
                }
            };
            let _ = tx.send((true, found));
        });
        true
    }

    /// Searches SoundCloud and Audius for the same song; each sends `(false,
    /// Some(copy))` for its best playable match.
    fn start_searches(&self, track: &Track, tx: &CopySender) {
        for source in [SourceKind::SoundCloud, SourceKind::Audius] {
            let tx = tx.clone();
            let title = track.title.clone();
            let artist = track.artist.clone();
            let duration = track.duration_secs;
            tokio::spawn(async move {
                if let Some(found) = search_copy(source, &title, &artist, duration).await {
                    let _ = tx.send((false, Some(found)));
                }
            });
        }
    }

    /// Swaps a copy into queue entry `key`: the engine plays it while the entry
    /// is still the held, current one, and otherwise repairs the slot. A copy
    /// from another source spends the song's one fallback; a YouTube retry does
    /// not, so if it fails again SoundCloud and Audius still get their turn.
    fn apply_fallback(&self, key: u64, original: &Track, found: Track) {
        if found.id != original.id {
            self.state.lock().fallback_tried.insert(original.id.clone());
        }
        let label = found.source.label();
        let mut tracks = vec![found];
        self.playable(&mut tracks);
        let found = tracks.remove(0);
        let current = self.is_current(key);
        self.player.replace(key, found);
        if current {
            self.status(format!("Playing from {label}"));
        }
    }
    pub fn save_session(&self) {
        let s = self.state.lock();
        self.result(self.library.save_session(&s.session()));
        self.result(self.library.save_settings(&s.settings));
    }
}
type CopySender = tokio::sync::mpsc::UnboundedSender<(bool, Option<Track>)>;

/// Best playable match for a song on `source`. SoundCloud lists DRM-only
/// uploads yt-dlp cannot play, so its top few are checked before one is used.
async fn search_copy(
    source: SourceKind,
    title: &str,
    artist: &str,
    duration: Option<f64>,
) -> Option<Track> {
    let matches = match crate::sources::match_tracks(source, title, artist, duration).await {
        Ok(matches) => matches,
        Err(e) => {
            log::debug!("{} fallback search: {e:#}", source.label());
            return None;
        }
    };
    for found in matches.into_iter().take(3) {
        if source != SourceKind::SoundCloud {
            return Some(found);
        }
        match crate::sources::playable_check(&found.play_url).await {
            Ok(()) => return Some(found),
            Err(why) => log::debug!("Skipping {}: {why}", found.title),
        }
    }
    None
}

pub(super) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
