use super::{controller::Controller, models};
use crate::model::{SourceKind, Track};
use slint::Model;

impl Controller {
    pub fn refresh_playlists(&self) {
        if let Some(rows) = self.result(models::playlists(&self.library)) {
            self.ui(move |w| models::update(w.get_playlists(), rows));
        }
    }
    pub fn playlist(&self, id: i64) {
        {
            let mut s = self.state.lock();
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
            let pinned = self.state.lock().settings.playlist_themes.contains_key(&id);
            self.ui(move |w| {
                w.set_page("Playlist".into());
                w.set_loading(false);
                w.set_playlist_view(true);
                w.set_active_playlist(id as i32);
                w.set_playlist_themed(pinned);
            });
            self.display(tracks, name);
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
            let s = self.state.lock();
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
                    let len = self.state.lock().display.len();
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
                            this.refresh_page("Downloads");
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
        let folder = self.state.lock().settings.download_dir.clone();
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
    pub(super) fn rescan(&self) {
        let this = self.clone();
        let dirs = self.state.lock().settings.music_dirs.clone();
        self.status("Scanning music folders…");
        tokio::spawn(async move {
            match tokio::task::spawn_blocking(move || crate::sources::local::scan(&dirs)).await {
                Ok(tracks) => {
                    if this
                        .result(this.library.replace_local_tracks(&tracks))
                        .is_some()
                    {
                        this.forget_local_pages();
                        this.go(super::browse::Nav::Page("Local files".into()));
                        this.status(format!("Scanned {} tracks", tracks.len()));
                    }
                }
                Err(e) => this.status(e),
            }
        });
    }
    pub fn dialog(&self, kind: &str, value: &str) {
        let id = self.state.lock().playlist;
        match kind {
            "create" if !value.trim().is_empty() => {
                if let Some(id) = self.result(self.library.create_playlist(value.trim(), None)) {
                    self.refresh_playlists();
                    self.go(super::browse::Nav::Playlist(id));
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
                    self.forget_playlist(id);
                    self.go(super::browse::Nav::Page("Home".into()));
                }
            }
            "import" => self.import_public(value.to_owned()),
            "theme-save" if !value.trim().is_empty() => self.theme_save(value.trim()),
            "theme-code" if !value.trim().is_empty() => self.theme_code(value),
            _ => {
                self.status("Enter a name first");
                return;
            }
        }
        self.ui(|w| w.set_dialog("".into()));
    }
    pub fn add_playlist(&self, id: i64) {
        let track = {
            let s = self.state.lock();
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
}
