//! The now playing view and the album color of the current song.
use super::{browse::Cached, controller::Controller};
use crate::model::{SourceKind, Track};

/// Pixel size the large artwork is decoded at.
const ART: u32 = 640;

/// The song whose large cover is on screen, so a resize redraws it without a blank frame.
static SHOWN: parking_lot::Mutex<Option<String>> = parking_lot::Mutex::new(None);

impl Controller {
    /// Runs when the song changes and when the now playing view opens or closes.
    pub fn now_changed(&self, open: bool, art_size: f32) {
        let track = self.state.lock().current.clone();
        self.album_color(track.as_ref());
        let id = track.as_ref().filter(|_| open).map(|t| t.id.clone());
        if *SHOWN.lock() != id {
            *SHOWN.lock() = None;
            self.ui(|w| w.set_now_big_art(Default::default()));
        }
        let Some(track) = track.filter(|_| open) else {
            return;
        };
        let year = self.known_year(&track);
        let album = track.album.clone().unwrap_or_default();
        let id = track.id.clone();
        let state = self.state.clone();
        self.ui(move |w| {
            if state.lock().current.as_ref().is_some_and(|t| t.id == id) {
                w.set_now_album_title(album.into());
                w.set_now_year(year.unwrap_or_default().into());
            }
        });
        let this = self.clone();
        tokio::spawn(async move {
            if track.source == SourceKind::Local && track.album.is_some() {
                let path = track.play_url.clone();
                if let Ok(Some(year)) =
                    tokio::task::spawn_blocking(move || super::collection::tags(&path).year).await
                {
                    let id = track.id.clone();
                    let state = this.state.clone();
                    this.ui(move |w| {
                        if state.lock().current.as_ref().is_some_and(|t| t.id == id) {
                            w.set_now_year(year.into());
                        }
                    });
                }
            }
            let Some(url) = track.artwork_url.clone() else {
                return;
            };
            // Rounded in the pixels: the software renderer can't clip to rounded corners.
            let radius = 14.0 / art_size.max(56.0);
            if let Ok(pixels) = crate::artwork::load_large(&url, ART, radius).await {
                let state = this.state.clone();
                this.ui(move |w| {
                    if w.get_now_open()
                        && state
                            .lock()
                            .current
                            .as_ref()
                            .is_some_and(|t| t.id == track.id)
                    {
                        *SHOWN.lock() = Some(track.id);
                        w.set_now_big_art(slint::Image::from_rgba8(pixels));
                    }
                });
            }
        });
    }

    fn album_color(&self, track: Option<&Track>) {
        let Some((id, url)) = track.and_then(|t| Some((t.id.clone(), t.artwork_url.clone()?)))
        else {
            self.ui(|w| w.set_album_tint(false));
            return;
        };
        let this = self.clone();
        tokio::spawn(async move {
            let color = crate::artwork::color(&url).await;
            log::debug!("Album color {color:?} for {url}");
            let state = this.state.clone();
            this.ui(move |w| {
                if !state.lock().current.as_ref().is_some_and(|t| t.id == id) {
                    return;
                }
                match color {
                    Some([r, g, b]) => {
                        w.set_album_color(slint::Color::from_rgb_u8(r, g, b));
                        w.set_album_tint(true);
                    }
                    None => w.set_album_tint(false),
                }
            });
        });
    }

    /// The album's year, when a page already seen this session listed it.
    fn known_year(&self, track: &Track) -> Option<String> {
        let album = track.album_ref()?;
        let same = |a: &crate::model::AlbumRef| {
            a.title.trim().eq_ignore_ascii_case(album.title.trim())
                && (album.id.is_none() || a.id == album.id || a.id.is_none())
        };
        let s = self.state.lock();
        s.browse_cache
            .iter()
            .find_map(|(_, _, page)| match page {
                Cached::Album(p) if same(&p.album) => p.album.year.clone(),
                Cached::Artist(p) => p
                    .albums
                    .iter()
                    .chain(&p.singles)
                    .find(|a| same(a))
                    .and_then(|a| a.year.clone()),
                _ => None,
            })
            .filter(|y| !y.is_empty())
    }
}
