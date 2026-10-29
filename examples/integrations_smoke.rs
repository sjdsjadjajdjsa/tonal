//! Manual check against the live services: imports a public Spotify playlist and fetches
//! lyrics. Needs the network. Run with `cargo run --example integrations_smoke`.

#![allow(dead_code)]

#[path = "../src/integrations/mod.rs"]
mod integrations;
#[path = "../src/model.rs"]
mod model;

use model::{SourceKind, Track};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let (name, tracks) = integrations::spotify::fetch_public(
        "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
    )
    .await?;
    println!(
        "Spotify: {name}: {} tracks ({:?})",
        tracks.len(),
        started.elapsed()
    );
    if let Some(first) = tracks.first() {
        println!(
            "Spotify first: {} — {} ({:?} seconds)",
            first.artist, first.title, first.duration_secs
        );
    }
    let (_, long_tracks) =
        integrations::spotify::fetch_public("spotify:playlist:37i9dQZF1DX4o1oenSJRJd").await?;
    println!(
        "Spotify All Out 2000s embed: {} tracks (embed limit: 100)",
        long_tracks.len()
    );
    let track = Track {
        id: "yt:FGBhQbmPwH8".into(),
        source: SourceKind::YouTube,
        source_id: "FGBhQbmPwH8".into(),
        title: "One More Time".into(),
        artist: "Daft Punk".into(),
        album: Some("Discovery".into()),
        duration_secs: Some(320.0),
        artwork_url: None,
        play_url: "https://music.youtube.com/watch?v=FGBhQbmPwH8".into(),
        ..Default::default()
    };
    let started = std::time::Instant::now();
    match integrations::lyrics::fetch(&track).await? {
        Some(lyrics) => {
            println!(
                "Lyrics: {} synced lines, plain={} ({:?})",
                lyrics.synced.len(),
                lyrics.plain.is_some(),
                started.elapsed()
            );
            if let Some((time, line)) = lyrics.synced.iter().find(|(_, text)| !text.is_empty()) {
                println!("Lyrics first: [{time:.2}] {line}");
            }
        }
        None => println!("Lyrics: not found ({:?})", started.elapsed()),
    }
    Ok(())
}
