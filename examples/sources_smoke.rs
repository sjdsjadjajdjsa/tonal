//! Live metadata-only checks; never starts mpv, downloads, or resolves streams.
//! Run: TONAL_SOURCE_DUMP=/tmp/tonal-browse cargo run --example sources_smoke
//! The optional dump directory contains real InnerTube browse JSON for inspection.

#![allow(dead_code)]

#[cfg(windows)]
#[path = "../src/job.rs"]
mod job;
#[path = "../src/model.rs"]
mod model;
#[path = "../src/sources/mod.rs"]
mod sources;
#[path = "../src/ytdl_wait.rs"]
#[allow(dead_code)]
mod ytdl_wait;

use model::{SourceKind, Track};
use std::time::Instant;

fn first(tracks: &[Track]) -> String {
    tracks
        .first()
        .map(|t| {
            format!(
                "{} — {} ({:?}, {:?}s, refs={:?}, album_id={:?}, art={})",
                t.title,
                t.artist,
                t.album,
                t.duration_secs,
                t.artist_refs,
                t.album_id,
                t.artwork_url.is_some()
            )
        })
        .unwrap_or_else(|| "none".to_owned())
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut youtube = None;
    for kind in [
        SourceKind::YouTube,
        SourceKind::SoundCloud,
        SourceKind::Audius,
    ] {
        let start = Instant::now();
        match sources::search(kind, "daft punk", 5).await {
            Ok(results) => {
                println!(
                    "{kind:?} search: {} results in {:.2?}; first: {}",
                    results.len(),
                    start.elapsed(),
                    first(&results)
                );
                if kind == SourceKind::YouTube {
                    youtube = results.into_iter().next();
                }
            }
            Err(error) => println!(
                "{kind:?} search error in {:.2?}: {error:#}",
                start.elapsed()
            ),
        }
    }
    for (kind, name) in [
        (SourceKind::YouTube, "Daft Punk"),
        (SourceKind::SoundCloud, "Daft Punk"),
        (SourceKind::Audius, "deadmau5"),
    ] {
        let start = Instant::now();
        let result = async {
            let r = sources::find_artist(kind, name)
                .await?
                .ok_or_else(|| anyhow::anyhow!("artist not found"))?;
            println!("{kind:?} resolved artist: {r:?}");
            sources::artist(&r).await
        }
        .await;
        match result {
            Ok(page) => {
                println!(
                    "{kind:?} artist {}: top={}, albums={}, singles={}, similar={}, image={} in {:.2?}; first: {}",
                    page.artist.name,
                    page.top.len(),
                    page.albums.len(),
                    page.singles.len(),
                    page.similar.len(),
                    page.image_url.is_some(),
                    start.elapsed(),
                    first(&page.top)
                );
                println!(
                    "  first album: {:?}; first single: {:?}; first similar: {:?}",
                    page.albums.first().map(|a| (&a.title, &a.id, &a.year)),
                    page.singles.first().map(|a| (&a.title, &a.id, &a.year)),
                    page.similar.first().map(|a| (&a.artist.name, &a.artist.id))
                );
            }
            Err(error) => println!(
                "{kind:?} artist error in {:.2?}: {error:#}",
                start.elapsed()
            ),
        }
    }
    let start = Instant::now();
    let result = async {
        let r = sources::find_album(SourceKind::YouTube, "Discovery", "Daft Punk")
            .await?
            .ok_or_else(|| anyhow::anyhow!("album not found"))?;
        println!("YouTube resolved album: {r:?}");
        sources::album(&r).await
    }
    .await;
    match result {
        Ok(page) => println!(
            "YouTube album {}: kind={}, year={:?}, artists={:?}, tracks={} in {:.2?}; first: {}; last: {}",
            page.album.title,
            page.kind,
            page.album.year,
            page.artists,
            page.tracks.len(),
            start.elapsed(),
            first(&page.tracks),
            page.tracks
                .last()
                .map(|t| t.title.as_str())
                .unwrap_or("none")
        ),
        Err(error) => println!("YouTube album error in {:.2?}: {error:#}", start.elapsed()),
    }
    if let Some(seed) = youtube {
        let start = Instant::now();
        match sources::radio(&seed, 5).await {
            Ok(results) => println!(
                "Radio: {} results in {:.2?}; first: {}",
                results.len(),
                start.elapsed(),
                first(&results)
            ),
            Err(error) => println!("Radio error in {:.2?}: {error:#}", start.elapsed()),
        }
    }
    let start = Instant::now();
    match sources::match_track(
        SourceKind::YouTube,
        "One More Time",
        "Daft Punk",
        Some(320.0),
    )
    .await
    {
        Ok(track) => println!(
            "Match in {:.2?}: {}",
            start.elapsed(),
            first(&track.into_iter().collect::<Vec<_>>())
        ),
        Err(error) => println!("Match error in {:.2?}: {error:#}", start.elapsed()),
    }
}
