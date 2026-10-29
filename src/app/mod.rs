mod browse;
mod controller;
#[cfg(feature = "debug-hooks")]
mod debug;
mod integrations;
mod library_actions;
mod models;
mod playback;
mod search;
mod settings;
mod state;
mod waves;

use crate::{
    AppWindow, LyricRow, PlaylistRow, SettingRow, TrackRow,
    library::Library,
    media_controls::ShellRequest,
    player::{self, PlayerConfig},
};
use controller::Controller;
use parking_lot::Mutex;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::sync::Arc;

pub fn run() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _guard = runtime.enter();
    let library = Library::open_default()?;
    let settings = library.load_settings();
    crate::sources::set_cookies_browser(&settings.youtube_cookies);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let player = player::spawn(
        PlayerConfig {
            volume: settings.volume,
            crossfade_secs: settings.crossfade_secs,
            normalize: settings.normalize,
            gapless: settings.gapless,
            shuffle: settings.shuffle,
            repeat: settings.repeat,
        },
        tx,
    )?;
    log::info!("Creating Tonal window");
    let window = AppWindow::new()?;
    log::info!("Window created");
    window.set_tracks(ModelRc::new(VecModel::<TrackRow>::default()));
    window.set_queue(ModelRc::new(VecModel::<TrackRow>::default()));
    window.set_albums(ModelRc::new(VecModel::<crate::CardRow>::default()));
    window.set_singles(ModelRc::new(VecModel::<crate::CardRow>::default()));
    window.set_similar(ModelRc::new(VecModel::<crate::CardRow>::default()));
    window.set_playlists(ModelRc::new(VecModel::<PlaylistRow>::default()));
    window.set_settings(ModelRc::new(VecModel::<SettingRow>::from(
        models::settings(&settings),
    )));
    window.set_lyrics(ModelRc::new(VecModel::<LyricRow>::from(models::lyrics(
        None, None,
    ))));
    window.set_volume(settings.volume as f32);
    window.set_waves(settings.visualizer && crate::visualizer::available());
    window.set_shuffle(settings.shuffle);
    window.set_repeat(format!("{:?}", settings.repeat).into());
    window.set_source(match settings.default_source {
        crate::model::SourceKind::YouTube => 0,
        crate::model::SourceKind::SoundCloud => 1,
        crate::model::SourceKind::Audius => 2,
        crate::model::SourceKind::Local => 3,
    });
    if let Some(color) = controller::parse_color(&settings.accent) {
        window.global::<crate::Theme>().set_accent(color);
    }
    let weak = window.as_weak();
    #[cfg(windows)]
    let hwnd = {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        window.show()?;
        let slint_handle = window.window().window_handle();
        match slint_handle.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => Some(handle.hwnd.get() as *mut std::ffi::c_void),
            _ => None,
        }
    };
    #[cfg(not(windows))]
    let hwnd = None;
    let media = crate::media_controls::spawn(player.clone(), hwnd, move |request| {
        let _ = weak.upgrade_in_event_loop(move |w| match request {
            ShellRequest::Raise => {
                let _ = w.show();
                w.window().request_redraw();
            }
            ShellRequest::Quit => {
                let _ = slint::quit_event_loop();
            }
        });
    });
    let scrobbler = Arc::new(Mutex::new(crate::integrations::scrobble::Scrobbler::new(
        &settings,
    )));
    let app = Controller {
        scrobbler,
        state: Arc::new(Mutex::new(state::State::new(settings))),
        library: library.clone(),
        player: player.clone(),
        window: window.as_weak(),
        media,
    };
    let c = app.clone();
    let weak = window.as_weak();
    window.on_search(move |query| {
        if let Some(w) = weak.upgrade() {
            c.replace_nav(browse::Nav::Search(query.to_string(), w.get_source()));
        }
    });
    let c = app.clone();
    window.on_navigate(move |page| c.go(browse::Nav::Page(page.to_string())));
    let c = app.clone();
    window.on_playlist(move |id| c.go(browse::Nav::Playlist(id as i64)));
    let c = app.clone();
    window.on_open_artist(move |list, index, which| c.open_artist(&list, index, which));
    let c = app.clone();
    window.on_open_album(move |list, index| c.open_album(&list, index));
    let c = app.clone();
    window.on_open_card(move |kind, index| c.open_card(&kind, index));
    let c = app.clone();
    window.on_artist_action(move |action| c.artist_action(&action));
    let c = app.clone();
    window.on_album_action(move |action| c.album_action(&action));
    let c = app.clone();
    window.on_nav(move |step| c.nav(step));
    {
        use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
        let c = app.clone();
        window.window().on_winit_window_event(move |_, event| match event {
            winit::event::WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button: button @ (winit::event::MouseButton::Back | winit::event::MouseButton::Forward),
                ..
            } => {
                c.nav(if *button == winit::event::MouseButton::Back { -1 } else { 1 });
                EventResult::PreventDefault
            }
            _ => EventResult::Propagate,
        });
    }
    let c = app.clone();
    window.on_play(move |index| {
        if index >= 0 {
            c.play(index as usize);
        }
    });
    let c = app.clone();
    window.on_track_action(move |index, action| {
        if index >= 0 {
            c.track_action(index as usize, &action);
        }
    });
    let c = app.clone();
    window.on_menu(move |index| {
        c.state.lock().selected = usize::try_from(index).ok();
    });
    let c = app.clone();
    window.on_menu_action(move |action| {
        let index = c.state.lock().selected;
        if let Some(i) = index {
            c.track_action(i, &action);
        }
    });
    let c = app.clone();
    window.on_add_playlist(move |id| c.add_playlist(id as i64));
    let c = app.clone();
    window.on_command(move |command| c.command(&command));
    let c = app.clone();
    window.on_seek(move |position| c.seek(position as f64));
    let c = app.clone();
    window.on_volume_change(move |v| c.player.set_volume(v as f64));
    let c = app.clone();
    window.on_setting_change(move |key, value| c.setting(&key, &value));
    let c = app.clone();
    let weak = window.as_weak();
    window.on_dialog_submit(move |value| {
        if let Some(w) = weak.upgrade() {
            c.dialog(&w.get_dialog(), &value);
        }
    });
    let c = app.clone();
    window.on_queue_action(move |i, action| {
        if i >= 0 {
            if action == "jump" {
                c.player.jump(i as usize);
            } else {
                c.player.remove(i as usize);
            }
        }
    });
    let c = app.clone();
    window.on_queue_move(move |i, d| {
        let len = c.state.lock().queue.len();
        let target = (i + d).max(0) as usize;
        if i >= 0 && target < len {
            c.player.move_track(i as usize, target);
        }
    });
    let c = app.clone();
    window.on_shortcut(move |key, ctrl, typing| {
        if typing && !ctrl {
            return;
        }
        match key.as_str() {
            " " if !typing => c.player.toggle_pause(),
            "l" if ctrl => c.command("like"),
            "m" if ctrl => c.command("mini"),
            k if k == slint::SharedString::from(slint::platform::Key::LeftArrow).as_str()
                && ctrl =>
            {
                c.player.prev()
            }
            k if k == slint::SharedString::from(slint::platform::Key::RightArrow).as_str()
                && ctrl =>
            {
                c.player.next()
            }
            k if k == slint::SharedString::from(slint::platform::Key::LeftArrow).as_str()
                && !typing =>
            {
                c.player.seek_relative(-5.0)
            }
            k if k == slint::SharedString::from(slint::platform::Key::RightArrow).as_str()
                && !typing =>
            {
                c.player.seek_relative(5.0)
            }
            k if k == slint::SharedString::from(slint::platform::Key::UpArrow).as_str() && ctrl => {
                let volume = c.state.lock().settings.volume;
                c.player.set_volume((volume + 5.0).min(100.0));
            }
            k if k == slint::SharedString::from(slint::platform::Key::DownArrow).as_str()
                && ctrl =>
            {
                let volume = c.state.lock().settings.volume;
                c.player.set_volume((volume - 5.0).max(0.0));
            }
            _ => {}
        }
    });
    log::info!("Loading library");
    app.refresh_playlists();
    app.go(browse::Nav::Page("Home".into()));
    let missing = crate::sources::missing_tools();
    if !missing.is_empty() {
        app.status(format!(
            "Missing tools: {} · Install these to play online music",
            missing.join(", ")
        ));
    }
    if let Some(session) = library.load_session().filter(|s| !s.tracks.is_empty()) {
        let mut tracks = session.tracks;
        app.playable(&mut tracks);
        app.state.lock().restore_position = Some(session.position);
        player.restore_tracks(tracks, session.current.unwrap_or(0), session.position);
    }
    let c = app.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            c.event(event);
        }
    });
    if window.get_source() == 0 {
        app.check_youtube_runtime();
    }
    #[cfg(feature = "debug-hooks")]
    debug::debug_hooks(&app, &window);
    log::info!("Starting Tonal event loop");
    window.run()?;
    app.save_session();
    player.shutdown();
    runtime.block_on(tokio::time::sleep(std::time::Duration::from_millis(100)));
    Ok(())
}
