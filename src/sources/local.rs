use crate::model::{SourceKind, Track, paths::cache_dir};
use lofty::{
    prelude::{Accessor, AudioFile, TaggedFileExt},
    probe::Probe,
};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use walkdir::WalkDir;

pub fn scan(dirs: &[PathBuf]) -> Vec<Track> {
    let covers = cache_dir().join("covers");
    let mut tracks = Vec::new();
    for dir in dirs {
        for item in WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !item.file_type().is_file() {
                continue;
            }
            let path = item.path();
            let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
                continue;
            };
            if ![
                "mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "wma", "aiff",
            ]
            .contains(&ext.to_ascii_lowercase().as_str())
            {
                continue;
            }
            let Ok(tagged) = Probe::open(path).and_then(|probe| probe.read()) else {
                continue;
            };
            let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let source_id = absolute.to_string_lossy().into_owned();
            let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
            let title = tag
                .and_then(|t| t.title())
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                });
            let artist = tag
                .and_then(|t| t.artist())
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Unknown artist".into());
            let album = tag.and_then(|t| t.album()).map(|s| s.to_string());
            let artwork_url = tag.and_then(|t| t.pictures().first()).and_then(|picture| {
                let hash = Sha256::digest(source_id.as_bytes());
                let filename = covers.join(format!(
                    "{}.jpg",
                    hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
                ));
                if !filename.is_file() {
                    let image = image::load_from_memory(picture.data()).ok()?;
                    std::fs::create_dir_all(&covers).ok()?;
                    image
                        .save_with_format(&filename, image::ImageFormat::Jpeg)
                        .ok()?;
                }
                Some(format!("file://{}", filename.to_string_lossy()))
            });
            tracks.push(Track {
                id: Track::make_id(SourceKind::Local, &source_id),
                source: SourceKind::Local,
                source_id: source_id.clone(),
                title,
                artist,
                album,
                duration_secs: Some(tagged.properties().duration().as_secs_f64()),
                artwork_url,
                play_url: source_id,
            });
        }
    }
    tracks
}
