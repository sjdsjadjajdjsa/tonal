use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SourceKind {
    #[default]
    YouTube,
    SoundCloud,
    Audius,
    Local,
}

impl SourceKind {
    pub fn prefix(self) -> &'static str {
        match self {
            SourceKind::YouTube => "yt",
            SourceKind::SoundCloud => "sc",
            SourceKind::Audius => "au",
            SourceKind::Local => "local",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SourceKind::YouTube => "YouTube",
            SourceKind::SoundCloud => "SoundCloud",
            SourceKind::Audius => "Audius",
            SourceKind::Local => "Local",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub source: SourceKind,
    pub source_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: Option<f64>,
    pub artwork_url: Option<String>,
    pub play_url: String,
}

impl Track {
    pub fn make_id(source: SourceKind, source_id: &str) -> String {
        format!("{}:{}", source.prefix(), source_id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub track_count: usize,
    pub origin: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub music_dirs: Vec<PathBuf>,
    pub download_dir: PathBuf,
    pub volume: f64,
    pub crossfade_secs: f64,
    pub normalize: bool,
    pub gapless: bool,
    pub autoplay: bool,
    pub default_source: SourceKind,
    pub accent: String,
    pub dark: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub spotify_client_id: String,
    pub spotify_refresh_token: Option<String>,
    pub lastfm_api_key: String,
    pub lastfm_api_secret: String,
    pub lastfm_session: Option<String>,
    pub listenbrainz_token: String,
    pub scrobble_enabled: bool,
    pub visualizer: bool,
    pub youtube_cookies: String,
}

impl Default for Settings {
    fn default() -> Self {
        let music = directories::UserDirs::new()
            .and_then(|d| d.audio_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("Music"));
        Self {
            music_dirs: vec![music.clone()],
            download_dir: music.join("Tonal"),
            volume: 80.0,
            crossfade_secs: 0.0,
            normalize: true,
            gapless: true,
            autoplay: true,
            default_source: SourceKind::YouTube,
            accent: "#ececee".into(),
            dark: true,
            shuffle: false,
            repeat: RepeatMode::Off,
            spotify_client_id: String::new(),
            spotify_refresh_token: None,
            lastfm_api_key: String::new(),
            lastfm_api_secret: String::new(),
            lastfm_session: None,
            listenbrainz_token: String::new(),
            scrobble_enabled: false,
            visualizer: true,
            youtube_cookies: String::new(),
        }
    }
}

pub mod paths {
    use std::path::PathBuf;

    fn dirs() -> directories::ProjectDirs {
        directories::ProjectDirs::from("", "", "tonal").expect("no home directory")
    }

    pub fn data_dir() -> PathBuf {
        let p = dirs().data_dir().to_path_buf();
        let _ = std::fs::create_dir_all(&p);
        p
    }

    pub fn cache_dir() -> PathBuf {
        let p = dirs().cache_dir().to_path_buf();
        let _ = std::fs::create_dir_all(&p);
        p
    }
}
