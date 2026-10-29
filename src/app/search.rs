use super::controller::Controller;
use crate::model::SourceKind;

impl Controller {
    pub(super) fn check_youtube_runtime(&self) {
        if self.state.lock().warned_youtube_runtime || crate::sources::js_runtime().is_some() {
            return;
        }
        {
            let mut state = self.state.lock();
            if state.warned_youtube_runtime {
                return;
            }
            state.warned_youtube_runtime = true;
        }
        self.status("Install Deno or Node.js for full YouTube support");
    }

    pub fn search(&self, query: String, source: i32, debug_play: bool) {
        if !matches!(source, 1..=3) {
            self.check_youtube_runtime();
        }
        let generation = {
            let mut s = self.state.lock();
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
            if this.state.lock().generation != generation {
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
}
