#![allow(dead_code)]

#[path = "../src/model.rs"]
mod model;
#[path = "../src/sources/mod.rs"]
mod sources;

use model::SourceKind;
use std::time::Instant;

#[tokio::main]
async fn main() {
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
                    "{:?}: {} results in {:?}; first: {:?}",
                    kind,
                    results.len(),
                    start.elapsed(),
                    results.first()
                );
                if kind == SourceKind::YouTube {
                    youtube = results.into_iter().next();
                }
            }
            Err(error) => println!("{:?}: error in {:?}: {error:#}", kind, start.elapsed()),
        }
    }
    for query in [
        "daft punk",
        "one more time daft punk",
        "kendrick lamar humble",
        "lofi hip hop",
    ] {
        match sources::search(SourceKind::YouTube, query, 3).await {
            Ok(results) => {
                println!("Songs [{query}]:");
                for (index, track) in results.iter().enumerate() {
                    println!(
                        "  {}. {} — {} ({:?})",
                        index + 1,
                        track.title,
                        track.artist,
                        track.album
                    );
                }
            }
            Err(error) => println!("Songs [{query}]: {error:#}"),
        }
    }
    if let Some(seed) = youtube {
        let start = Instant::now();
        match sources::radio(&seed, 5).await {
            Ok(results) => {
                println!(
                    "Radio: {} results in {:?}; first: {:?}",
                    results.len(),
                    start.elapsed(),
                    results.first()
                );
                if std::env::args().any(|arg| arg == "--download") {
                    let track = results
                        .iter()
                        .min_by_key(|track| track.duration_secs.unwrap_or(f64::MAX) as u64)
                        .unwrap_or(&seed);
                    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
                    let printer = tokio::spawn(async move {
                        while let Some(value) = receiver.recv().await {
                            println!("Download progress: {value:.2}");
                        }
                    });
                    match sources::download::download(
                        track,
                        std::path::Path::new("/tmp/tonal-dl-test"),
                        sender,
                    )
                    .await
                    {
                        Ok(path) => println!(
                            "Downloaded: {} ({} bytes)",
                            path.display(),
                            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
                        ),
                        Err(error) => println!("Download failed: {error:#}"),
                    }
                    let _ = printer.await;
                }
            }
            Err(error) => println!("Radio: error in {:?}: {error:#}", start.elapsed()),
        }
    }
    let start = Instant::now();
    match sources::match_track("One More Time", "Daft Punk", Some(320.0)).await {
        Ok(track) => println!("Match in {:?}: {track:?}", start.elapsed()),
        Err(error) => println!("Match error in {:?}: {error:#}", start.elapsed()),
    }
}
