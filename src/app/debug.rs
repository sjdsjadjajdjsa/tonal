use super::controller::Controller;
use crate::AppWindow;
use slint::ComponentHandle;

/// Runs a test script, one step per line, by sending input straight to the window so the
/// real pointer and keyboard focus stay untouched: `wait ms`, `click x y`, `dclick x y`,
/// `rclick x y`, `move x y`, `scroll x y dy`,
/// `type text`, `key name`, `shot path`, `cmd command`, `page name`,
/// `theme action value`, `eq preset`, `size w h`, `log text`.
fn run_script(
    app: Controller,
    weak: slint::Weak<AppWindow>,
    mut steps: std::collections::VecDeque<String>,
) {
    use slint::platform::{Key, PointerEventButton, WindowEvent};
    let Some(line) = steps.pop_front() else {
        log::info!("SCRIPT done");
        return;
    };
    let (verb, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
    let mut wait = 150;
    if let Some(w) = weak.upgrade() {
        let win = w.window();
        let num = |i: usize| {
            rest.split_whitespace()
                .nth(i)
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0)
        };
        let at = slint::LogicalPosition::new(num(0), num(1));
        let key = |text: slint::SharedString| {
            win.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
            win.dispatch_event(WindowEvent::KeyReleased { text });
        };
        match verb {
            "wait" => wait = num(0) as u64,
            "click" => {
                win.dispatch_event(WindowEvent::PointerMoved { position: at });
                win.dispatch_event(WindowEvent::PointerPressed {
                    position: at,
                    button: PointerEventButton::Left,
                });
                win.dispatch_event(WindowEvent::PointerReleased {
                    position: at,
                    button: PointerEventButton::Left,
                });
            }
            "dclick" => {
                win.dispatch_event(WindowEvent::PointerMoved { position: at });
                for _ in 0..2 {
                    win.dispatch_event(WindowEvent::PointerPressed {
                        position: at,
                        button: PointerEventButton::Left,
                    });
                    win.dispatch_event(WindowEvent::PointerReleased {
                        position: at,
                        button: PointerEventButton::Left,
                    });
                }
            }
            "rclick" => {
                win.dispatch_event(WindowEvent::PointerMoved { position: at });
                win.dispatch_event(WindowEvent::PointerPressed {
                    position: at,
                    button: PointerEventButton::Right,
                });
                win.dispatch_event(WindowEvent::PointerReleased {
                    position: at,
                    button: PointerEventButton::Right,
                });
            }
            "move" => win.dispatch_event(WindowEvent::PointerMoved { position: at }),
            "scroll" => {
                win.dispatch_event(WindowEvent::PointerMoved { position: at });
                win.dispatch_event(WindowEvent::PointerScrolled {
                    position: at,
                    delta_x: 0.0,
                    delta_y: num(2),
                });
            }
            "type" => rest.chars().for_each(|c| key(c.to_string().into())),
            "key" => match rest {
                "enter" => key(Key::Return.into()),
                "escape" => key(Key::Escape.into()),
                "tab" => key(Key::Tab.into()),
                "down" => key(Key::DownArrow.into()),
                "up" => key(Key::UpArrow.into()),
                combo if combo.starts_with("ctrl+") => {
                    win.dispatch_event(WindowEvent::KeyPressed {
                        text: Key::Control.into(),
                    });
                    key(combo[5..].into());
                    win.dispatch_event(WindowEvent::KeyReleased {
                        text: Key::Control.into(),
                    });
                }
                other => key(other.into()),
            },
            "shot" => match win.take_snapshot() {
                Ok(px) => {
                    if let Err(e) = image::save_buffer(
                        rest,
                        px.as_bytes(),
                        px.width(),
                        px.height(),
                        image::ColorType::Rgba8,
                    ) {
                        log::error!("Script shot: {e}");
                    }
                }
                Err(e) => log::error!("Script shot: {e}"),
            },
            "cmd" => app.command(rest),
            "size" => win.set_size(slint::LogicalSize::new(num(0), num(1))),
            "eq" => app.eq_choose(rest),
            "page" => app.load_page(rest),
            "theme" => {
                let (action, value) = rest.split_once(' ').unwrap_or((rest, ""));
                app.theme_action(action, value);
            }
            "open" => app.theme_open_file(rest.into()),
            "log" => log::info!("SCRIPT {rest}"),
            _ => log::warn!("SCRIPT unknown step: {line}"),
        }
    }
    slint::Timer::single_shot(std::time::Duration::from_millis(wait), move || {
        run_script(app, weak, steps)
    });
}

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

/// Fills an empty library for screenshots: "# Name" starts a playlist, every other line is a
/// YouTube Music search whose top hit goes in it, gets played once and every third one liked.
fn demo_library(app: Controller, file: String) {
    tokio::spawn(async move {
        let Ok(text) = std::fs::read_to_string(&file) else {
            return;
        };
        let mut playlist = None;
        for (n, line) in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .enumerate()
        {
            if let Some(name) = line.strip_prefix("# ") {
                playlist = app.result(app.library.create_playlist(name, None));
                continue;
            }
            let found = crate::sources::search(crate::model::SourceKind::YouTube, line, 1).await;
            let Some(track) = app.result(found).and_then(|t| t.into_iter().next()) else {
                continue;
            };
            app.result(app.library.upsert_tracks(std::slice::from_ref(&track)));
            if let Some(id) = playlist {
                app.result(
                    app.library
                        .add_to_playlist(id, std::slice::from_ref(&track)),
                );
            }
            app.result(app.library.record_play(&track));
            if n % 3 == 0 {
                app.result(app.library.set_liked(&track, true));
            }
            if let Some(art) = &track.artwork_url {
                let _ = crate::artwork::load(art, 88).await;
            }
        }
        app.refresh_playlists();
        app.load_page("Home");
        log::info!("Demo library ready");
    });
}

pub(super) fn debug_hooks(app: &Controller, window: &AppWindow) {
    if let Ok(file) = std::env::var("TONAL_DEBUG_DEMO") {
        demo_library(app.clone(), file);
    }
    if let Ok(dir) = std::env::var("TONAL_DEBUG_MUSIC") {
        app.state.lock().settings.music_dirs = vec![dir.into()];
        app.command("rescan");
    }
    if let Some(id) = std::env::var("TONAL_DEBUG_PLAYLIST")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        app.playlist(id);
        app.play(0);
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
    if let Ok(preset) = std::env::var("TONAL_DEBUG_EQ") {
        app.eq_choose(&preset);
    }
    if let Ok(active) = std::env::var("TONAL_DEBUG_LYRICS") {
        let active = active.parse::<usize>().ok();
        let lines = [
            "I'ma mess up, wear colours, then hide it in a puffer",
            "'Cause my friends told me, \"If we all look the same, then we'll look tougher\"",
            "No funds left, all spent on something vintage that I'll never wear again",
            "I look fly when I pull up tonight and you say \"What's up?\"",
            "One too many late nights, another delayed flight",
            "",
            "I just landed, random country, and I'm under stage lights",
            "Got no time for stage fright, pitch black, still no daylight",
            "I don't have a clue for what's in store, just pray that fate might (I pray)",
            "I just hope that fate knows where I'm going because I don't know (Don't know)",
            "Taking flights to make these",
        ];
        let lyrics = crate::integrations::lyrics::Lyrics {
            synced: lines.iter().enumerate().map(|(i, l)| (i as f64 * 4.0, l.to_string())).collect(),
            ..Default::default()
        };
        window.set_lyrics(slint::ModelRc::new(slint::VecModel::from(super::models::lyrics(Some(&lyrics), active))));
    }
    if let Ok(theme) = std::env::var("TONAL_DEBUG_THEME") {
        app.theme_action("apply", &format!("g:{theme}"));
    }
    if std::env::var_os("TONAL_DEBUG_RESCAN").is_some() {
        app.command("rescan");
    }
    if let Ok(view) = std::env::var("TONAL_DEBUG_VIEW") {
        match view.as_str() {
            "home" => app.load_page("Home"),
            "mini" | "queue" | "lyrics" | "eq" => app.command(&view),
            "settings" => app.load_page("Settings"),
            "themes" => app.load_page("Themes"),
            "nowplaying" => open_now_playing(window.as_weak(), 40),
            "stats" | "stats-all" => {
                let period = (view == "stats-all") as i32;
                app.ui(move |w| {
                    w.set_home_tab(2);
                    w.set_stats_period(period);
                });
                let app = app.clone();
                slint::Timer::single_shot(std::time::Duration::from_millis(1500), move || {
                    app.command(&format!("stats:{period}"));
                });
            }
            "albums" => {
                app.command("local:albums");
                app.go(super::browse::Nav::Page("Local files".into()));
            }
            _ => {}
        }
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
    // "model" applies the best match's correction, "?query" opens the search with the
    // query typed, "model?query" does both; "-" takes the correction off.
    if let Ok(value) = std::env::var("TONAL_DEBUG_HEADPHONE") {
        let (name, query) = match value.split_once('?') {
            Some((name, query)) => (name.to_string(), Some(query.to_string())),
            None => (value, None),
        };
        match name.as_str() {
            "" => {}
            "-" => app.eq_headphones_clear(),
            _ => app.eq_headphones_debug(name),
        }
        if let Some(query) = query {
            window.set_eq_headphone_query(query.as_str().into());
            app.eq_headphones_open();
            // Once the panel is laid out and the list is in.
            let weak = window.as_weak();
            let app = app.clone();
            slint::Timer::single_shot(std::time::Duration::from_millis(3000), move || {
                if let Some(w) = weak.upgrade() {
                    app.eq_headphones_search(&query);
                    w.set_eq_headphone_show(w.get_eq_headphone_show() + 1);
                }
            });
        }
    }
    if let Some(text) = std::env::var("TONAL_DEBUG_SCRIPT")
        .ok()
        .and_then(|f| std::fs::read_to_string(f).ok())
    {
        let steps = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect();
        let (app, weak) = (app.clone(), window.as_weak());
        slint::Timer::single_shot(std::time::Duration::from_secs(4), move || {
            run_script(app, weak, steps)
        });
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
