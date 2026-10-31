#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod artwork;
mod integrations;
#[cfg(windows)]
mod job;
mod library;
mod media_controls;
mod model;
mod player;
mod scenes;
mod sources;
mod visualizer;
mod ytdl_wait;

slint::include_modules!();

fn bundled_tools_on_path() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
    else {
        return;
    };
    let mut paths = vec![dir];
    // Apps opened from Finder or the Dock don't get the shell's PATH, so add Homebrew's.
    #[cfg(target_os = "macos")]
    paths.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(std::path::PathBuf::from));
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
    if let Some(code) = ytdl_wait::run() {
        std::process::exit(code);
    }
    // glibc gives every thread its own malloc arena; two are plenty here and save several MB.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
    bundled_tools_on_path();
    env_logger::init();
    let _ = rustls::crypto::ring::default_provider().install_default();
    app::run()
}
