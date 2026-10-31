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
            "accent" => s.accent = value.into(),
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
