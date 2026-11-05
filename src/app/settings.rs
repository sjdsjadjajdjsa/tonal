use super::{controller::Controller, models};
use slint::ComponentHandle;

impl Controller {
    pub fn setting(&self, key: &str, value: &str) {
        self.apply_setting(key, value);
        if key == "album_colors" {
            let on = self.state.lock().settings.album_colors;
            self.ui(move |w| w.set_album_colors(on));
        }
        if key == "beat_flash" {
            let k = models::flash(&self.state.lock().settings.beat_flash);
            self.ui(move |w| w.global::<crate::WaveLook>().set_flash(k));
        }
        if key == "bit_perfect" {
            self.eq_refresh();
        }
        if key == "offline_liked" {
            self.sync_offline();
        }
        if key == "motion" {
            let on = self.state.lock().settings.motion;
            self.ui(move |w| w.global::<crate::Motion>().set_on(on));
        }
        if key == "discord" {
            self.discord_update();
        }
        if key == "visualizer" {
            let (on, paused) = {
                let s = self.state.lock();
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
        let mut state = self.state.lock();
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
                if let Some(v) = seconds(value) {
                    s.crossfade_secs = v;
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
            "sounds" => {
                if let Ok(v) = value.parse() {
                    s.sounds = v;
                }
            }
            "offline_liked" => {
                if let Ok(v) = value.parse() {
                    s.offline_liked = v;
                }
            }
            "motion" => {
                if let Ok(v) = value.parse() {
                    s.motion = v;
                }
            }
            "discord" => {
                if let Ok(v) = value.parse() {
                    s.discord = v;
                }
            }
            "eq_per_output" => {
                if let Ok(v) = value.parse() {
                    s.eq_per_output = v;
                }
            }
            "album_colors" => {
                if let Ok(v) = value.parse() {
                    s.album_colors = v;
                }
            }
            "replaygain" => {
                if let Ok(v) = value.parse() {
                    s.replaygain = v;
                }
            }
            "bit_perfect" => {
                if let Ok(v) = value.parse() {
                    s.bit_perfect = v;
                }
            }
            "audio_device" => {
                if let Some(name) = models::device_named(value) {
                    s.audio_device = name;
                }
            }
            "youtube_cookies" => s.youtube_cookies = value.trim().to_lowercase(),
            "beat_flash" if models::FLASH.iter().any(|(name, _)| *name == value) => {
                s.beat_flash = value.into()
            }
            "accent" if accent_ok(value) => s.accent = value.trim().into(),
            "spotify_client_id" => s.spotify_client_id = value.into(),
            "lastfm_api_key" => s.lastfm_api_key = value.into(),
            "lastfm_api_secret" => s.lastfm_api_secret = value.into(),
            "listenbrainz_token" => s.listenbrainz_token = value.into(),
            _ => {}
        }
    }
    pub fn save_settings(&self) {
        let settings = self.state.lock().settings.clone();
        if self.result(self.library.save_settings(&settings)).is_some() {
            self.scrobbler.lock().update_settings(&settings);
            self.player.set_crossfade(settings.crossfade_secs);
            self.player.set_normalize(settings.normalize);
            self.player.set_gapless(settings.gapless);
            // Each is a no-op in the engine unless it changed.
            self.player.set_audio_device(&settings.audio_device);
            self.player.set_replaygain(settings.replaygain);
            self.player.set_bit_perfect(settings.bit_perfect);
            crate::sources::set_cookies_browser(&settings.youtube_cookies);
            self.player.refresh_ytdl_options();
            let color = super::themes::accent(&settings);
            self.ui(move |w| {
                w.global::<crate::Theme>().set_accent(color);
                models::update(w.get_settings(), models::settings(&settings));
            });
        }
    }
    /// Looks for a newer Tonal; `quiet` only speaks up when there is one.
    pub(super) fn update_check(&self, quiet: bool) {
        use crate::integrations::update::{self, State};
        if matches!(update::state(), State::Checking | State::Installing) {
            return;
        }
        update::set_state(State::Checking);
        self.refresh_settings();
        let this = self.clone();
        tokio::spawn(async move {
            let state = match update::check().await {
                Ok(state) => state,
                Err(e) => {
                    log::warn!("Update check: {e:#}");
                    if !quiet {
                        this.status("Couldn't reach GitHub to check for updates");
                    }
                    State::Failed
                }
            };
            match &state {
                State::Available { version, .. } => this.status(format!(
                    "Tonal {version} is out · Settings, Updates to install it"
                )),
                State::Current if !quiet => this.status("Tonal is up to date"),
                _ => {}
            }
            update::set_state(state);
            this.refresh_settings();
        });
    }

    /// Settings, Updates: checks, then installs what it found.
    pub(super) fn update_tonal(&self) {
        use crate::integrations::update::{self, State};
        let (version, url) = match update::state() {
            State::Available { version, url } => (version, url),
            State::Checking | State::Installing => return,
            _ => return self.update_check(false),
        };
        let Some(url) = url else {
            let _ = open::that(update::PAGE);
            return;
        };
        update::set_state(State::Installing);
        self.refresh_settings();
        self.status(format!("Installing Tonal {version}…"));
        let this = self.clone();
        tokio::spawn(async move {
            match update::install(&url).await {
                Ok(()) => this.ui(|_| {
                    let _ = slint::quit_event_loop();
                }),
                Err(e) => {
                    log::warn!("Update: {e:#}");
                    this.status(format!("Couldn't install Tonal {version}: {e}"));
                    update::set_state(State::Available {
                        version,
                        url: Some(url),
                    });
                    this.refresh_settings();
                }
            }
        });
    }

    fn refresh_settings(&self) {
        let settings = self.state.lock().settings.clone();
        self.ui(move |w| models::update(w.get_settings(), models::settings(&settings)));
    }

    pub(super) fn update_ytdlp(&self) {
        self.status("Updating yt-dlp…");
        let this = self.clone();
        tokio::spawn(async move {
            if let Some(message) = this.result(crate::sources::update_ytdlp().await) {
                this.status(message);
            }
        });
    }
}

fn seconds(value: &str) -> Option<f64> {
    let v = value.trim().replace(',', ".").parse::<f64>().ok()?;
    v.is_finite().then(|| v.clamp(0.0, 30.0))
}

fn accent_ok(value: &str) -> bool {
    value.trim().is_empty() || parse_color(value.trim()).is_some()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfade_is_clamped_and_rejects_junk() {
        assert_eq!(seconds("45"), Some(30.0));
        assert_eq!(seconds(" 2,5 "), Some(2.5));
        assert_eq!(seconds("-3"), Some(0.0));
        assert_eq!(seconds("abc"), None);
        assert_eq!(seconds("NaN"), None);
        assert_eq!(seconds("inf"), None);
    }

    #[test]
    fn accent_must_be_a_hex_colour() {
        assert!(accent_ok("#ff8800"));
        assert!(accent_ok(" #FF8800 "));
        assert!(accent_ok(""));
        assert!(!accent_ok("red"));
        assert!(!accent_ok("#fff"));
    }
}
