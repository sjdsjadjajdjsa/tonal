#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod artwork;
mod integrations;
mod library;
mod media_controls;
mod model;
mod player;
mod sources;
mod visualizer;

slint::include_modules!();

fn bundled_tools_on_path() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
    else {
        return;
    };
    let mut paths = vec![dir];
    paths.extend(
        std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    if let Ok(joined) = std::env::join_paths(paths) {
        // SAFETY: runs first in main, before any other thread exists.
        unsafe { std::env::set_var("PATH", joined) };
    }
}

fn main() -> anyhow::Result<()> {
    bundled_tools_on_path();
    env_logger::init();
    let _ = rustls::crypto::ring::default_provider().install_default();
    app::run()
}
