use super::{models, state::State};
use crate::{
    AppWindow,
    library::Library,
    model::{RepeatMode, Track},
    player::PlayerHandle,
};
use futures::{StreamExt, stream};
use parking_lot::Mutex;
use slint::{ComponentHandle, Model};
use std::{sync::Arc, time::Duration};

pub use super::settings::parse_color;

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
            let mut s = self.state.lock();
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
            if this.state.lock().toast == id {
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
    pub fn display(&self, tracks: Vec<Track>, heading: String) {
        let generation = self.state.lock().generation;
        self.display_for(generation, tracks, heading);
    }
    /// Shows `tracks` if the page for `generation` is still the current one.
    pub fn display_for(&self, generation: u64, tracks: Vec<Track>, heading: String) -> bool {
        {
            let mut s = self.state.lock();
            if s.generation != generation {
                return false;
            }
            s.display = tracks.clone();
        }
        let state = self.state.clone();
        let library = self.library.clone();
        let display = tracks.clone();
        self.ui(move |w| {
            if state.lock().generation != generation {
                return;
            }
            models::update(w.get_tracks(), models::rows(&library, &display));
            w.set_heading(heading.into());
            w.set_loading(false);
        });
        self.artwork(tracks, false);
        true
    }
    pub(super) fn artwork(&self, tracks: Vec<Track>, queue: bool) {
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
            let mut s = self.state.lock();
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
    pub fn command(&self, command: &str) {
        match command {
            "toggle" => self.player.toggle_pause(),
            "next" => self.player.next(),
            "previous" => self.player.prev(),
            "like" => {
                let t = self.state.lock().current.clone();
                if let Some(t) = t {
                    self.like(t);
                }
            }
            "shuffle" => {
                let s = self.state.lock();
                self.player.set_shuffle(!s.settings.shuffle);
            }
            "repeat" => {
                let s = self.state.lock();
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
            "queue" | "lyrics" | "eq" => {
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
                let mut s = self.state.lock();
                if s.settings.volume > 0.0 {
                    s.muted_from = Some(s.settings.volume);
                    self.player.set_volume(0.0);
                } else {
                    self.player.set_volume(s.muted_from.take().unwrap_or(80.0));
                }
            }
            "save-settings" => self.save_settings(),
            "update-ytdlp" => self.update_ytdlp(),
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
}
