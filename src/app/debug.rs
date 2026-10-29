use super::controller::Controller;
use crate::AppWindow;
use slint::ComponentHandle;

pub(super) fn debug_hooks(app: &Controller, window: &AppWindow) {
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
            "mini" | "queue" | "lyrics" => app.command(&view),
            "settings" => app.load_page("Settings"),
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
