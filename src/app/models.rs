use crate::{
    LyricRow, PlaylistRow, SettingRow, TrackRow,
    library::Library,
    model::{Settings, Track},
};
use slint::{Model, ModelRc, VecModel};

pub fn time(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}
pub fn rows(library: &Library, tracks: &[Track]) -> Vec<TrackRow> {
    tracks
        .iter()
        .map(|t| TrackRow {
            id: t.id.clone().into(),
            title: t.title.clone().into(),
            artist: t.artist.clone().into(),
            album: t.album.clone().unwrap_or_default().into(),
            duration: t
                .duration_secs
                .filter(|d| *d > 0.0)
                .map(time)
                .unwrap_or_default()
                .into(),
            source: t.source.label().into(),
            liked: library.is_liked(&t.id),
            artists: ModelRc::new(VecModel::from(
                t.artists()
                    .into_iter()
                    .map(|a| slint::SharedString::from(a.name))
                    .collect::<Vec<_>>(),
            )),
            ..Default::default()
        })
        .collect()
}
pub fn update<T: Clone + PartialEq + 'static>(model: ModelRc<T>, rows: Vec<T>) {
    if let Some(model) = model.as_any().downcast_ref::<VecModel<T>>() {
        while model.row_count() > rows.len() {
            model.remove(model.row_count() - 1);
        }
        for (i, row) in rows.into_iter().enumerate() {
            if i >= model.row_count() {
                model.push(row);
            } else if model.row_data(i).as_ref() != Some(&row) {
                model.set_row_data(i, row);
            }
        }
    }
}
pub fn playlists(library: &Library) -> anyhow::Result<Vec<PlaylistRow>> {
    Ok(library
        .playlists()?
        .into_iter()
        .map(|p| PlaylistRow {
            id: p.id as i32,
            name: p.name.into(),
            count: p.track_count as i32,
        })
        .collect())
}
pub fn lyrics(
    lyrics: Option<&crate::integrations::lyrics::Lyrics>,
    active: Option<usize>,
) -> Vec<LyricRow> {
    match lyrics {
        Some(l) if !l.synced.is_empty() => l
            .synced
            .iter()
            .enumerate()
            .map(|(i, (time, line))| LyricRow {
                text: if line.trim().is_empty() {
                    "♪".into()
                } else {
                    line.trim().into()
                },
                active: Some(i) == active,
                time: *time as f32,
            })
            .collect(),
        Some(crate::integrations::lyrics::Lyrics {
            plain: Some(plain), ..
        }) if !plain.trim().is_empty() => plain
            .lines()
            .map(|line| LyricRow {
                text: line.trim().into(),
                active: false,
                time: -1.0,
            })
            .collect(),
        _ => Vec::new(),
    }
}
type Field<'a> = (&'a str, &'a str, String, &'a str, &'a str, bool);

pub fn settings(s: &Settings) -> Vec<SettingRow> {
    let dirs = s
        .music_dirs
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join(";");
    let sections: [(&str, Vec<Field>); 6] = [
        (
            "Playback",
            vec![
                (
                    "crossfade_secs",
                    "Crossfade",
                    s.crossfade_secs.to_string(),
                    "Seconds of overlap between songs. 0 turns it off.",
                    "text",
                    false,
                ),
                (
                    "gapless",
                    "Gapless playback",
                    s.gapless.to_string(),
                    "No silence between songs on the same album.",
                    "toggle",
                    false,
                ),
                (
                    "normalize",
                    "Even out volume",
                    s.normalize.to_string(),
                    "Keeps loud and quiet songs at a similar level.",
                    "toggle",
                    false,
                ),
                (
                    "autoplay",
                    "Autoplay",
                    s.autoplay.to_string(),
                    "Keep playing similar songs when the queue ends.",
                    "toggle",
                    false,
                ),
                ("eq", "Equalizer", s.eq_preset.clone(), "", "action", false),
                (
                    "visualizer",
                    "Wave visualizer",
                    s.visualizer.to_string(),
                    "Flowing waves under the player while music plays.",
                    "toggle",
                    false,
                ),
            ],
        ),
        (
            "Library",
            vec![
                (
                    "music_dirs",
                    "Music folders",
                    dirs,
                    "Separate folders with ;",
                    "text",
                    false,
                ),
                (
                    "download_dir",
                    "Download folder",
                    s.download_dir.to_string_lossy().into_owned(),
                    "",
                    "text",
                    false,
                ),
                (
                    "rescan",
                    "Scan music folders",
                    "Rescan".into(),
                    "Finds new songs in your music folders.",
                    "action",
                    false,
                ),
            ],
        ),
        (
            "YouTube",
            vec![
                (
                    "youtube_cookies",
                    "Use browser login",
                    s.youtube_cookies.clone(),
                    "If YouTube asks to sign in, type your browser: chromium, firefox, brave, chrome",
                    "text",
                    false,
                ),
                (
                    "update-ytdlp",
                    "yt-dlp",
                    "Update".into(),
                    "Keeps YouTube and SoundCloud working",
                    "action",
                    false,
                ),
            ],
        ),
        (
            "Appearance",
            vec![(
                "accent",
                "Accent color",
                s.accent.clone(),
                "Any hex color. Default #ececee.",
                "text",
                false,
            )],
        ),
        (
            "Spotify",
            vec![
                (
                    "spotify_client_id",
                    "Client ID",
                    s.spotify_client_id.clone(),
                    "Free app at developer.spotify.com, redirect URI http://127.0.0.1:8898/callback",
                    "text",
                    false,
                ),
                (
                    "spotify-login",
                    "Import liked songs and playlists",
                    "Log in".into(),
                    "Needed for playlists over 100 songs and your liked songs.",
                    "action",
                    false,
                ),
            ],
        ),
        (
            "Scrobbling",
            vec![
                (
                    "scrobble_enabled",
                    "Scrobble what you play",
                    s.scrobble_enabled.to_string(),
                    "",
                    "toggle",
                    false,
                ),
                (
                    "lastfm_api_key",
                    "Last.fm API key",
                    s.lastfm_api_key.clone(),
                    "From last.fm/api/account/create",
                    "text",
                    false,
                ),
                (
                    "lastfm_api_secret",
                    "Last.fm API secret",
                    s.lastfm_api_secret.clone(),
                    "",
                    "text",
                    true,
                ),
                (
                    "lastfm-login",
                    "Last.fm account",
                    if s.lastfm_session.is_some() {
                        "Reconnect".into()
                    } else {
                        "Connect".into()
                    },
                    if s.lastfm_session.is_some() {
                        "Connected"
                    } else {
                        "Not connected"
                    },
                    "action",
                    false,
                ),
                (
                    "listenbrainz_token",
                    "ListenBrainz token",
                    s.listenbrainz_token.clone(),
                    "From listenbrainz.org/settings",
                    "text",
                    true,
                ),
            ],
        ),
    ];
    let mut rows = Vec::new();
    for (header, fields) in sections {
        let count = fields.len();
        for (i, (key, label, value, hint, kind, secret)) in fields.into_iter().enumerate() {
            rows.push(SettingRow {
                key: key.into(),
                label: label.into(),
                value: value.into(),
                hint: hint.into(),
                kind: kind.into(),
                secret,
                header: if i == 0 {
                    header.into()
                } else {
                    Default::default()
                },
                first: i == 0,
                last: i + 1 == count,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_times_and_fallback_lyrics() {
        assert_eq!(time(369.5), "6:09");
        assert_eq!(time(-1.0), "0:00");
        assert!(lyrics(None, None).is_empty());
    }
    #[test]
    fn settings_exposes_playback_and_credentials() {
        let fields = settings(&Settings::default());
        assert!(fields.iter().any(|f| f.key == "music_dirs"));
        let update = fields.iter().find(|f| f.key == "update-ytdlp").unwrap();
        assert_eq!(update.label, "yt-dlp");
        assert_eq!(update.value, "Update");
        assert_eq!(update.hint, "Keeps YouTube and SoundCloud working");
        assert_eq!(update.kind, "action");
        assert!(
            fields
                .iter()
                .any(|f| f.key == "lastfm_api_secret" && f.secret)
        );
    }
}
