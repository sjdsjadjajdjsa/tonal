use super::{models, state::State};
use crate::{
    AppWindow,
    library::Library,
    model::{RepeatMode, SourceKind, Track},
    player::{PlayerEvent, PlayerHandle},
};
use futures::{StreamExt, stream};
use slint::{ComponentHandle, Model};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct Controller {
    pub state: Arc<Mutex<State>>,
    pub library: Library,
    pub player: PlayerHandle,
    pub window: slint::Weak<AppWindow>,
    pub media: crate::media_controls::MediaControlsHandle,
    pub scrobbler: Arc<Mutex<crate::integrations::scrobble::Scrobbler>>,
}
impl Controller {
    pub fn ui(&self, f: impl FnOnce(AppWindow) + Send + 'static) {
        let _ = self.window.upgrade_in_event_loop(f);
    }
    pub fn status(&self, message: impl ToString) {
        let text = message.to_string();
        log::info!("{text}");
        let id = {
            let mut s = self.state.lock().unwrap();
            s.toast += 1;
            s.toast
        };
        self.ui(move |w| {
            w.set_status(text.into());
            w.set_loading(false);
        });
        let this = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(3500)).await;
            if this.state.lock().unwrap().toast == id {
                this.ui(|w| w.set_status(Default::default()));
            }
        });
    }
    pub fn result<T>(&self, result: anyhow::Result<T>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                self.status(e);
                None
            }
        }
    }
    pub fn refresh_playlists(&self) {
        if let Some(rows) = self.result(models::playlists(&self.library)) {
            self.ui(move |w| models::update(w.get_playlists(), rows));
        }
    }
    pub fn display(&self, tracks: Vec<Track>, heading: String) {
        let generation = {
            let mut s = self.state.lock().unwrap();
            s.display = tracks.clone();
            s.generation
        };
        let state = self.state.clone();
        let library = self.library.clone();
        let display = tracks.clone();
        self.ui(move |w| {
            if state.lock().unwrap().generation != generation {
                return;
            }
            models::update(w.get_tracks(), models::rows(&library, &display));
            w.set_heading(heading.into());
            w.set_loading(false);
        });
        self.artwork(tracks, false);
    }
    fn artwork(&self, tracks: Vec<Track>, queue: bool) {
        let this = self.clone();
        tokio::spawn(async move {
            stream::iter(tracks.into_iter().enumerate())
                .for_each_concurrent(4, |(index, t)| {
                    let this = this.clone();
                    async move {
                        let Some(url) = t.artwork_url else { return };
                        if let Ok(pixels) = crate::artwork::load(&url, 96).await {
                            this.ui(move |w| {
                                let model = if queue { w.get_queue() } else { w.get_tracks() };
                                if let Some(mut row) =
                                    model.row_data(index).filter(|r| r.id.as_str() == t.id)
                                {
                                    row.art = slint::Image::from_rgba8(pixels);
                                    model.set_row_data(index, row);
                                }
                            });
                        }
                    }
                })
                .await;
        });
    }
    pub fn load_page(&self, page: &str) {
        {
            let mut s = self.state.lock().unwrap();
            s.playlist = None;
            s.generation += 1;
        }
        let p = page.to_owned();
        self.ui(move |w| {
            w.set_page(p.clone().into());
            w.set_loading(false);
            w.set_playlist_view(false);
            w.set_heading(p.into());
        });
        let tracks = match page {
            "Home" => self.library.recent(100),
            "Liked songs" => self.library.liked(),
            "Local files" => self.library.local_tracks(),
            "Downloads" => self.library.downloads(),
            "Search" | "Settings" => return,
            _ => return,
        };
        if let Some(tracks) = self.result(tracks) {
            if page == "Home" {
                self.ui(|w| w.set_home_tab(0));
            }
            self.display(tracks, page.into());
        }
    }
    pub fn playlist(&self, id: i64) {
        {
            let mut s = self.state.lock().unwrap();
            s.playlist = Some(id);
            s.generation += 1;
        }
        if let Some(tracks) = self.result(self.library.playlist_tracks(id)) {
            let name = self
                .library
                .playlists()
                .unwrap_or_default()
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| p.name)
                .unwrap_or_default();
            self.ui(move |w| {
                w.set_page("Playlist".into());
                w.set_loading(false);
                w.set_playlist_view(true);
                w.set_active_playlist(id as i32);
            });
            self.display(tracks, name);
        }
    }
    pub fn search(&self, query: String, source: i32, debug_play: bool) {
        let generation = {
            let mut s = self.state.lock().unwrap();
            s.generation += 1;
            s.generation
        };
        self.ui(|w| {
            w.set_loading(true);
            w.set_page("Search".into());
            w.set_playlist_view(false);
        });
        let this = self.clone();
        tokio::spawn(async move {
            let kind = match source {
                1 => SourceKind::SoundCloud,
                2 => SourceKind::Audius,
                3 => SourceKind::Local,
                _ => SourceKind::YouTube,
            };
            let result = if kind == SourceKind::Local {
                let query = query.to_lowercase();
                this.library.local_tracks().map(|tracks| {
                    tracks
                        .into_iter()
                        .filter(|t| {
                            format!(
                                "{} {} {}",
                                t.title,
                                t.artist,
                                t.album.as_deref().unwrap_or_default()
                            )
                            .to_lowercase()
                            .contains(&query)
                        })
                        .collect()
                })
            } else {
                crate::sources::search(kind, &query, 50).await
            };
            if this.state.lock().unwrap().generation != generation {
                return;
            }
            if let Some(tracks) = this.result(result) {
                let count = tracks.len();
                this.display(tracks, format!("Results for “{query}”"));
                if debug_play && count > 0 {
                    this.play(0);
                }
            }
        });
    }
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
            let mut s = self.state.lock().unwrap();
            s.restore_position = None;
            s.display.clone()
        };
        self.playable(&mut tracks);
        if index < tracks.len() {
            self.player.play_tracks(tracks, index);
        }
    }
    pub fn like(&self, track: Track) {
        let liked = !self.library.is_liked(&track.id);
        if self.result(self.library.set_liked(&track, liked)).is_none() {
            return;
        }
        let current = self
            .state
            .lock()
            .unwrap()
            .current
            .as_ref()
            .is_some_and(|t| t.id == track.id);
        self.ui(move |w| {
            for model in [w.get_tracks(), w.get_queue()] {
                for i in 0..model.row_count() {
                    if let Some(mut row) = model.row_data(i).filter(|r| r.id.as_str() == track.id) {
                        row.liked = liked;
                        model.set_row_data(i, row);
                    }
                }
            }
            if current {
                w.set_now_liked(liked);
            }
        });
    }
    pub fn track_action(&self, index: usize, action: &str) {
        let (track, playlist) = {
            let s = self.state.lock().unwrap();
            (s.display.get(index).cloned(), s.playlist)
        };
        let Some(track) = track else {
            return;
        };
        match action {
            "Play" => self.play(index),
            "Play next" | "Add to queue" => {
                let mut tracks = vec![track];
                self.playable(&mut tracks);
                if action == "Play next" {
                    self.player.play_next(tracks);
                } else {
                    self.player.enqueue(tracks);
                }
            }
            "Like / Unlike" => self.like(track),
            "Download" => self.download(track),
            "Start radio" => self.radio(track),
            "Remove from playlist" => {
                if let Some(id) = playlist
                    && self
                        .result(self.library.remove_from_playlist(id, index))
                        .is_some()
                {
                    self.playlist(id);
                }
            }
            "Move up in playlist" | "Move down in playlist" => {
                if let Some(id) = playlist {
                    let len = self.state.lock().unwrap().display.len();
                    let target = if action == "Move up in playlist" {
                        index.saturating_sub(1)
                    } else {
                        (index + 1).min(len.saturating_sub(1))
                    };
                    if self
                        .result(self.library.move_in_playlist(id, index, target))
                        .is_some()
                    {
                        self.playlist(id);
                    }
                }
            }
            "Remove download" => {
                if let Some(path) = self.library.download_path(&track.id) {
                    let this = self.clone();
                    tokio::spawn(async move {
                        if (track.source == SourceKind::Local
                            || !path.exists()
                            || this
                                .result(tokio::fs::remove_file(&path).await.map_err(Into::into))
                                .is_some())
                            && this
                                .result(this.library.remove_download(&track.id))
                                .is_some()
                        {
                            this.load_page("Downloads");
                            this.status("Download removed");
                        }
                    });
                }
            }
            _ => {}
        }
    }
    fn download(&self, track: Track) {
        let this = self.clone();
        let folder = self.state.lock().unwrap().settings.download_dir.clone();
        tokio::spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let progress = this.clone();
            let title = track.title.clone();
            let task = tokio::spawn(async move {
                let mut last = -1;
                while let Some(value) = rx.recv().await {
                    let percent = (value * 100.0) as i32;
                    if percent != last {
                        last = percent;
                        progress.status(format!("Downloading {title}: {percent}%"));
                    }
                }
            });
            if let Some(path) =
                this.result(crate::sources::download::download(&track, &folder, tx).await)
                && this
                    .result(this.library.set_download(&track, &path))
                    .is_some()
            {
                this.status(format!("Downloaded {}", track.title));
            }
            let _ = task.await;
        });
    }
    fn radio(&self, track: Track) {
        let this = self.clone();
        tokio::spawn(async move {
            if let Some(mut tracks) = this.result(crate::sources::radio(&track, 20).await) {
                {
                    let s = this.state.lock().unwrap();
                    tracks.retain(|t| !s.queue.iter().any(|q| q.id == t.id));
                }
                this.playable(&mut tracks);
                this.player.enqueue(tracks);
            }
        });
    }
    pub fn command(&self, command: &str) {
        match command {
            "toggle" => self.player.toggle_pause(),
            "next" => self.player.next(),
            "previous" => self.player.prev(),
            "like" => {
                let t = self.state.lock().unwrap().current.clone();
                if let Some(t) = t {
                    self.like(t);
                }
            }
            "shuffle" => {
                let s = self.state.lock().unwrap();
                self.player.set_shuffle(!s.settings.shuffle);
            }
            "repeat" => {
                let s = self.state.lock().unwrap();
                self.player.set_repeat(match s.settings.repeat {
                    RepeatMode::Off => RepeatMode::All,
                    RepeatMode::All => RepeatMode::One,
                    RepeatMode::One => RepeatMode::Off,
                });
            }
            "shuffle-play" => {
                self.player.set_shuffle(true);
                self.play(0);
            }
            "queue" | "lyrics" => {
                let panel = command.to_owned();
                self.ui(move |w| {
                    if w.get_panel() == panel {
                        w.set_panel("".into());
                    } else {
                        w.set_panel(panel.into());
                    }
                });
            }
            "mini" => self.ui(|w| {
                let mini = !w.get_mini();
                w.set_mini(mini);
                w.window().set_size(slint::LogicalSize::new(
                    if mini { 380.0 } else { 1240.0 },
                    if mini { 170.0 } else { 780.0 },
                ));
            }),
            "recent" => {
                if let Some(t) = self.result(self.library.recent(100)) {
                    self.display(t, "Home".into());
                }
            }
            "most-played" => {
                if let Some(t) = self.result(self.library.most_played(100)) {
                    self.display(t, "Home".into());
                }
            }
            "mute" => {
                let mut s = self.state.lock().unwrap();
                if s.settings.volume > 0.0 {
                    s.muted_from = Some(s.settings.volume);
                    self.player.set_volume(0.0);
                } else {
                    self.player.set_volume(s.muted_from.take().unwrap_or(80.0));
                }
            }
            "save-settings" => self.save_settings(),
            "rescan" => self.rescan(),
            "spotify-login" => self.spotify_login(),
            "lastfm-login" => self.lastfm_login(),
            "clear-queue" => {
                self.player.stop();
                self.player.clear();
            }
            _ => {}
        }
    }
    pub fn seek(&self, position: f64) {
        self.player.seek(position);
        let paused = self.state.lock().unwrap().paused;
        self.media.set_playback(!paused, position);
    }
    pub fn setting(&self, key: &str, value: &str) {
        self.apply_setting(key, value);
        if key == "visualizer" {
            let (on, paused) = {
                let s = self.state.lock().unwrap();
                (s.settings.visualizer, s.paused)
            };
            self.ui(move |w| {
                w.set_waves(on && crate::visualizer::available());
                super::waves::set_playing(&w, !paused);
            });
        }
        self.save_settings();
    }
    fn apply_setting(&self, key: &str, value: &str) {
        let mut state = self.state.lock().unwrap();
        let s = &mut state.settings;
        match key {
            "music_dirs" => {
                s.music_dirs = value
                    .split(';')
                    .filter(|v| !v.trim().is_empty())
                    .map(|v| v.trim().into())
                    .collect()
            }
            "download_dir" => s.download_dir = value.into(),
            "crossfade_secs" => {
                if let Ok(v) = value.parse::<f64>() {
                    s.crossfade_secs = v.clamp(0.0, 30.0);
                }
            }
            "normalize" => {
                if let Ok(v) = value.parse() {
                    s.normalize = v;
                }
            }
            "gapless" => {
                if let Ok(v) = value.parse() {
                    s.gapless = v;
                }
            }
            "autoplay" => {
                if let Ok(v) = value.parse() {
                    s.autoplay = v;
                }
            }
            "scrobble_enabled" => {
                if let Ok(v) = value.parse() {
                    s.scrobble_enabled = v;
                }
            }
            "visualizer" => {
                if let Ok(v) = value.parse() {
                    s.visualizer = v;
                }
            }
            "youtube_cookies" => s.youtube_cookies = value.trim().to_lowercase(),
            "accent" => s.accent = value.into(),
            "spotify_client_id" => s.spotify_client_id = value.into(),
            "lastfm_api_key" => s.lastfm_api_key = value.into(),
            "lastfm_api_secret" => s.lastfm_api_secret = value.into(),
            "listenbrainz_token" => s.listenbrainz_token = value.into(),
            _ => {}
        }
    }
    pub fn save_settings(&self) {
        let settings = self.state.lock().unwrap().settings.clone();
        if self.result(self.library.save_settings(&settings)).is_some() {
            self.scrobbler.lock().unwrap().update_settings(&settings);
            self.player.set_crossfade(settings.crossfade_secs);
            self.player.set_normalize(settings.normalize);
            self.player.set_gapless(settings.gapless);
            crate::sources::set_cookies_browser(&settings.youtube_cookies);
            self.player.refresh_ytdl_options();
            let color = parse_color(&settings.accent);
            let rows = models::settings(&settings);
            self.ui(move |w| {
                if let Some(color) = color {
                    w.global::<crate::Theme>().set_accent(color);
                }
                models::update(w.get_settings(), rows);
            });
        }
    }
    fn rescan(&self) {
        let this = self.clone();
        let dirs = self.state.lock().unwrap().settings.music_dirs.clone();
        self.status("Scanning music folders…");
        tokio::spawn(async move {
            match tokio::task::spawn_blocking(move || crate::sources::local::scan(&dirs)).await {
                Ok(tracks) => {
                    if this
                        .result(this.library.replace_local_tracks(&tracks))
                        .is_some()
                    {
                        this.load_page("Local files");
                        this.status(format!("Scanned {} tracks", tracks.len()));
                    }
                }
                Err(e) => this.status(e),
            }
        });
    }
    pub fn dialog(&self, kind: &str, value: &str) {
        let id = self.state.lock().unwrap().playlist;
        match kind {
            "create" if !value.trim().is_empty() => {
                if let Some(id) = self.result(self.library.create_playlist(value.trim(), None)) {
                    self.refresh_playlists();
                    self.playlist(id);
                }
            }
            "rename" if !value.trim().is_empty() => {
                if let Some(id) = id
                    && self
                        .result(self.library.rename_playlist(id, value.trim()))
                        .is_some()
                {
                    self.refresh_playlists();
                    self.playlist(id);
                }
            }
            "delete" if value == "DELETE" => {
                if let Some(id) = id
                    && self.result(self.library.delete_playlist(id)).is_some()
                {
                    self.refresh_playlists();
                    self.load_page("Home");
                }
            }
            "import" => self.import_public(value.to_owned()),
            _ => {
                self.status("Enter a name first");
                return;
            }
        }
        self.ui(|w| w.set_dialog("".into()));
    }
    pub fn add_playlist(&self, id: i64) {
        let track = {
            let s = self.state.lock().unwrap();
            s.selected.and_then(|i| s.display.get(i).cloned())
        };
        if let Some(t) = track
            && self
                .result(self.library.add_to_playlist(id, &[t]))
                .is_some()
        {
            self.refresh_playlists();
            self.status("Added to playlist");
        }
        self.ui(|w| w.set_dialog("".into()));
    }
    async fn import_tracks(
        &self,
        name: String,
        origin: Option<String>,
        tracks: Vec<crate::integrations::spotify::SpotifyTrack>,
    ) -> anyhow::Result<()> {
        let total = tracks.len();
        let mut done = 0;
        let mut matched = Vec::new();
        let mut failures = 0;
        let tasks = stream::iter(tracks.into_iter().enumerate().map(|(index, t)| async move {
            let result = crate::sources::match_track(&t.title, &t.artist, t.duration_secs).await;
            (index, t, result)
        }))
        .buffer_unordered(4);
        futures::pin_mut!(tasks);
        while let Some((index, metadata, result)) = tasks.next().await {
            done += 1;
            match result {
                Ok(Some(mut t)) => {
                    if t.album.is_none() {
                        t.album = metadata.album;
                    }
                    if t.artwork_url.is_none() {
                        t.artwork_url = metadata.artwork_url;
                    }
                    matched.push((index, t));
                }
                Ok(None) | Err(_) => failures += 1,
            }
            self.status(format!(
                "Matched {}/{total} · {done}/{total} processed",
                matched.len()
            ));
        }
        matched.sort_by_key(|(i, _)| *i);
        let tracks = matched.into_iter().map(|(_, t)| t).collect::<Vec<_>>();
        let id = self.library.create_playlist(&name, origin.as_deref())?;
        self.library.add_to_playlist(id, &tracks)?;
        self.refresh_playlists();
        self.playlist(id);
        self.status(format!(
            "Imported {} tracks · {failures} unmatched",
            tracks.len()
        ));
        Ok(())
    }
    fn import_public(&self, url: String) {
        let this = self.clone();
        self.status("Fetching Spotify playlist…");
        tokio::spawn(async move {
            if let Some((name, tracks)) =
                this.result(crate::integrations::spotify::fetch_public(&url).await)
            {
                this.result(this.import_tracks(name, Some(url), tracks).await);
            }
        });
    }
    fn spotify_login(&self) {
        let this = self.clone();
        let s = self.state.lock().unwrap().settings.clone();
        self.status("Opening Spotify login…");
        tokio::spawn(async move {
            let result: anyhow::Result<()> = async {
                let mut auth = if let Some(token) = s.spotify_refresh_token.as_deref() {
                    match crate::integrations::spotify::refresh(&s.spotify_client_id, token).await {
                        Ok(auth) => auth,
                        Err(_) => crate::integrations::spotify::login(&s.spotify_client_id).await?,
                    }
                } else {
                    crate::integrations::spotify::login(&s.spotify_client_id).await?
                };
                {
                    let mut state = this.state.lock().unwrap();
                    state.settings.spotify_refresh_token = Some(auth.refresh_token.clone());
                    this.library.save_settings(&state.settings)?;
                }
                let liked = crate::integrations::spotify::liked_tracks(&auth).await?;
                this.import_tracks("Spotify · Liked songs".into(), None, liked)
                    .await?;
                for list in crate::integrations::spotify::my_playlists(&auth).await? {
                    if auth.expires_at <= now() + 60 {
                        auth = crate::integrations::spotify::refresh(
                            &s.spotify_client_id,
                            &auth.refresh_token,
                        )
                        .await?;
                        let mut state = this.state.lock().unwrap();
                        state.settings.spotify_refresh_token = Some(auth.refresh_token.clone());
                        this.library.save_settings(&state.settings)?;
                    }
                    this.status(format!(
                        "Importing {} · {} tracks{}",
                        list.name,
                        list.track_count,
                        if list.image_url.is_some() {
                            " · with artwork"
                        } else {
                            ""
                        }
                    ));
                    let tracks =
                        crate::integrations::spotify::playlist_tracks(&auth, &list.id).await?;
                    this.import_tracks(
                        list.name,
                        Some(format!("https://open.spotify.com/playlist/{}", list.id)),
                        tracks,
                    )
                    .await?;
                }
                Ok(())
            }
            .await;
            this.result(result);
        });
    }
    fn lastfm_login(&self) {
        let this = self.clone();
        let s = self.state.lock().unwrap().settings.clone();
        self.status("Approve Tonal in your browser…");
        tokio::spawn(async move {
            if let Some(session) = this.result(
                crate::integrations::scrobble::lastfm_login(
                    &s.lastfm_api_key,
                    &s.lastfm_api_secret,
                )
                .await,
            ) {
                {
                    this.state.lock().unwrap().settings.lastfm_session = Some(session);
                }
                this.save_settings();
                this.status("Connected to Last.fm");
            }
        });
    }
    pub fn event(&self, event: PlayerEvent) {
        match event {
            PlayerEvent::QueueChanged { tracks, current } => {
                {
                    let mut s = self.state.lock().unwrap();
                    s.queue = tracks.clone();
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
                }
                let initial_position = {
                    let mut s = self.state.lock().unwrap();
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
                    .set_playback(!self.state.lock().unwrap().paused, initial_position);
                let liked = track.as_ref().is_some_and(|t| self.library.is_liked(&t.id));
                let title = track.as_ref().map(|t| t.title.clone()).unwrap_or_default();
                let artist = track.as_ref().map(|t| t.artist.clone()).unwrap_or_default();
                let now_id = track.as_ref().map(|t| t.id.clone()).unwrap_or_default();
                let has_track = track.is_some();
                self.ui(move |w| {
                    w.set_now_id(now_id.into());
                    if !has_track {
                        super::waves::set_playing(&w, false);
                    }
                    w.set_position(initial_position as f32);
                    w.set_elapsed(models::time(initial_position).into());
                    w.set_duration(initial_duration as f32);
                    w.set_total(models::time(initial_duration).into());
                    w.set_now_title(title.into());
                    w.set_now_artist(artist.into());
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
                                if check
                                    .lock()
                                    .unwrap()
                                    .current
                                    .as_ref()
                                    .is_some_and(|t| t.id == id)
                                {
                                    w.set_now_art(slint::Image::from_rgba8(pixels));
                                }
                            });
                        }
                    });
                    let this = self.clone();
                    tokio::spawn(async move {
                        let scrobbler = this.scrobbler.lock().unwrap().clone();
                        this.result(scrobbler.now_playing(&t).await);
                        let lyrics = match crate::integrations::lyrics::fetch(&t).await {
                            Ok(l) => l,
                            Err(e) => {
                                log::debug!("Lyrics: {e}");
                                None
                            }
                        };
                        {
                            let mut s = this.state.lock().unwrap();
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
                    let mut s = self.state.lock().unwrap();
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
                    let mut s = self.state.lock().unwrap();
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
                    self.state.lock().unwrap().settings.volume = volume;
                }
                self.media.set_volume(volume);
                self.ui(move |w| {
                    if w.get_volume() != volume as f32 {
                        w.set_volume(volume as f32);
                    }
                });
            }
            PlayerEvent::Shuffle(shuffle) => {
                self.state.lock().unwrap().settings.shuffle = shuffle;
                self.ui(move |w| w.set_shuffle(shuffle));
            }
            PlayerEvent::Repeat(repeat) => {
                self.state.lock().unwrap().settings.repeat = repeat;
                self.ui(move |w| w.set_repeat(format!("{repeat:?}").into()));
            }
            PlayerEvent::Buffering(buffering) => self.ui(move |w| {
                w.set_buffering(buffering);
            }),
            PlayerEvent::NearQueueEnd => {
                let seed = {
                    let s = self.state.lock().unwrap();
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
            PlayerEvent::Error(error) => match error
                .strip_prefix("mpv could not load ")
                .and_then(|rest| rest.split_once(": "))
            {
                Some((title, _)) => self.status(format!("Couldn't play {title}")),
                None => self.status(error),
            },
        }
    }
    pub fn save_session(&self) {
        let s = self.state.lock().unwrap();
        self.result(self.library.save_session(&s.session()));
        self.result(self.library.save_settings(&s.settings));
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn parse_color(value: &str) -> Option<slint::Color> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(slint::Color::from_rgb_u8(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
    ))
}
