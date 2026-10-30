use super::controller::Controller;
use crate::AppWindow;
use slint::ComponentHandle;

/// Opens the now playing view once a song is loaded.
fn open_now_playing(weak: slint::Weak<AppWindow>, tries: u32) {
    slint::Timer::single_shot(std::time::Duration::from_millis(500), move || {
        let Some(w) = weak.upgrade() else { return };
        if w.get_now_id().is_empty() && tries > 0 {
            open_now_playing(weak, tries - 1);
        } else {
            w.set_now_open(true);
        }
    });
}

pub(super) fn debug_hooks(app: &Controller, window: &AppWindow) {
    if let Ok(dir) = std::env::var("TONAL_DEBUG_MUSIC") {
        app.state.lock().settings.music_dirs = vec![dir.into()];
        app.command("rescan");
    }
    if let Ok(query) = std::env::var("TONAL_DEBUG_QUERY") {
        let source = std::env::var("TONAL_DEBUG_SOURCE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        window.set_query(query.clone().into());
        window.set_source(source);
        app.search(query, source, true);
    }
    if let Ok(view) = std::env::var("TONAL_DEBUG_VIEW") {
        match view.as_str() {
            "mini" | "queue" | "lyrics" | "eq" => app.command(&view),
            "settings" => app.load_page("Settings"),
            "nowplaying" => open_now_playing(window.as_weak(), 40),
            "stats" | "stats-all" => {
                let period = (view == "stats-all") as i32;
                app.ui(move |w| {
                    w.set_home_tab(2);
                    w.set_stats_period(period);
                });
                app.load_stats(period);
            }
            "albums" => {
                app.command("local:albums");
                app.go(super::browse::Nav::Page("Local files".into()));
            }
            _ => {}
        }
    }
    if let Some(id) = std::env::var("TONAL_DEBUG_PLAYLIST")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        app.playlist(id);
        app.play(0);
    }
    let source = match std::env::var("TONAL_DEBUG_SOURCE").as_deref() {
        Ok("1") => crate::model::SourceKind::SoundCloud,
        Ok("2") => crate::model::SourceKind::Audius,
        Ok("3") => crate::model::SourceKind::Local,
        _ => crate::model::SourceKind::YouTube,
    };
    if let Ok(name) = std::env::var("TONAL_DEBUG_ARTIST") {
        app.go(super::browse::Nav::Artist(crate::model::ArtistRef {
            source,
            name,
            id: None,
        }));
    }
    if let Ok(spec) = std::env::var("TONAL_DEBUG_ALBUM") {
        let (title, artist) = spec.split_once('|').unwrap_or((spec.as_str(), ""));
        app.go(super::browse::Nav::Album(crate::model::AlbumRef {
            source,
            title: title.into(),
            artist: artist.into(),
            ..Default::default()
        }));
    }
    // Clicks a card after the page has loaded, like "local-album:0" or "top-artist:1".
    if let Ok(spec) = std::env::var("TONAL_DEBUG_CARD") {
        let c = app.clone();
        slint::Timer::single_shot(std::time::Duration::from_secs(5), move || {
            if let Some((kind, index)) = spec.split_once(':') {
                c.open_card(kind, index.parse().unwrap_or(0));
            }
        });
    }
    if let Ok(preset) = std::env::var("TONAL_DEBUG_EQ") {
        app.eq_choose(&preset);
    }
    if let Ok(title) = std::env::var("TONAL_DEBUG_TITLE") {
        window.set_window_title(title.into());
    }
    if let Ok(path) = std::env::var("TONAL_DEBUG_SNAPSHOT") {
        let weak = window.as_weak();
        let delay = std::env::var("TONAL_DEBUG_SNAPSHOT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(12);
        slint::Timer::single_shot(std::time::Duration::from_secs(delay), move || {
            if let Some(w) = weak.upgrade() {
                match w.window().take_snapshot() {
                    Ok(pixels) => {
                        if let Err(e) = image::save_buffer(
                            &path,
                            pixels.as_bytes(),
                            pixels.width(),
                            pixels.height(),
                            image::ColorType::Rgba8,
                        ) {
                            log::error!("Snapshot: {e}");
                        }
                    }
                    Err(e) => log::error!("Snapshot: {e}"),
                }
            }
        });
    }
}
