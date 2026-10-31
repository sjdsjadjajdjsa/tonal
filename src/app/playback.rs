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
            PlayerEvent::CheckNext { key, track } => {
                let this = self.clone();
                tokio::spawn(async move {
                    this.precheck(key, track).await;
                });
            }
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
        crate::sources::resolve::forget(&track.play_url);
        let tried = self.state.lock().fallback_tried.contains(&track.id);
        let this = self.clone();
        tokio::spawn(async move {
            if tried {
                this.player.skip_failed(key);
                this.status(format!("Couldn't play {}", track.title));
                return;
            }
            let mut race = this.start_race(&track);
            let winner = race.recv().await;
            if winner.is_none() {
                this.player.skip_failed(key);
            }
            match winner {
                Some(found) => this.apply_fallback(key, &track.id, found),
                None => this.status(format!("Couldn't play {}", track.title)),
            }
        });
    }

    /// The engine says `track` plays next. Makes sure it can: a song that will
    /// not load gets its fallback swapped in before the gap instead of after it.
    async fn precheck(&self, key: u64, track: Track) {
        if std::path::Path::new(&track.play_url).is_file()
            || !crate::sources::is_streamed_url(&track.play_url)
            || crate::sources::resolve::lookup(&track.play_url).is_some()
        {
            return;
        }
        match crate::sources::resolve::resolve(&track.play_url).await {
            Ok(_) => {}
            Err(why) => {
                log::debug!("Pre-check failed for {}: {why}", track.title);
                if self.state.lock().fallback_tried.contains(&track.id) {
                    return;
                }
                let mut race = self.start_race(&track);
                if let Some(found) = race.recv().await {
                    self.apply_fallback(key, &track.id, found);
                }
            }
        }
    }

    /// Searches every source for `track` at once and returns the channel the
    /// first playable copy arrives on. YouTube's own retry keeps running after
    /// the race is over, so any access strategy it finds is remembered either way.
    fn start_race(&self, track: &Track) -> tokio::sync::mpsc::UnboundedReceiver<Track> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let unblock_id = format!("unblock:{}", track.id);
        let retry_youtube = {
            let mut s = self.state.lock();
            let retry = crate::sources::is_youtube_url(&track.play_url)
                && !s.fallback_tried.contains(&unblock_id);
            s.fallback_tried.insert(unblock_id);
            retry
        };
        if retry_youtube {
            let tx = tx.clone();
            let url = track.play_url.clone();
            let original = track.clone();
            tokio::spawn(async move {
                match crate::sources::unblock_youtube(&url).await {
                    Ok((how, direct)) => {
                        log::info!("YouTube retry for {} ({how})", original.title);
                        if let Some(direct) = direct {
                            crate::sources::resolve::remember(&url, &direct);
                        }
                        let _ = tx.send(original);
                    }
                    Err(why) => log::info!("YouTube stays blocked for {}: {why}", original.title),
                }
            });
        }
        for source in [SourceKind::SoundCloud, SourceKind::Audius] {
            let tx = tx.clone();
            let title = track.title.clone();
            let artist = track.artist.clone();
            let duration = track.duration_secs;
            tokio::spawn(async move {
                let matches =
                    match crate::sources::match_tracks(source, &title, &artist, duration).await {
                        Ok(matches) => matches,
                        Err(e) => {
                            log::debug!("{} fallback search: {e:#}", source.label());
                            return;
                        }
                    };
                for found in matches.into_iter().take(3) {
                    if source != SourceKind::SoundCloud {
                        let _ = tx.send(found);
                        return;
                    }
                    // SoundCloud lists DRM-only uploads yt-dlp cannot play, so
                    // check the best few before offering one; the check also
                    // caches the stream URL so the swap starts instantly.
                    match crate::sources::resolve::resolve(&found.play_url).await {
                        Ok(_) => {
                            let _ = tx.send(found);
                            return;
                        }
                        Err(why) => log::debug!("Skipping {}: {why}", found.title),
                    }
                }
            });
        }
        rx
    }

    /// Swaps a fallback into queue entry `key`: the engine plays it while the
    /// entry is still the held, current one, and otherwise repairs the slot.
    /// The original song marks its one fallback attempt as used only now that a
    /// copy was actually found, so a transient search failure does not spend it.
    fn apply_fallback(&self, key: u64, original_id: &str, found: Track) {
        self.state
            .lock()
            .fallback_tried
            .insert(original_id.to_owned());
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
pub(super) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
