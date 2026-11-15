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
mod sfx;
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
    let mut paths = vec![dir.clone()];
    #[cfg(target_os = "macos")]
    {
        // Tonal.app/Contents/Helpers/mpv (packaging/macos/fetch-mpv.sh), ahead of
        // Homebrew's. mpv's macOS build needs macOS 15.
        let helpers = dir.with_file_name("Helpers").join("mpv");
        if macos_major() >= 15 && helpers.is_dir() {
            paths.push(helpers);
        }
        // Apps opened from Finder or the Dock don't get the shell's PATH, so add Homebrew's.
        paths.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(std::path::PathBuf::from));
    }
    paths.extend(
        std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    // The yt-dlp Tonal downloads when none is installed, after every other.
    #[cfg(target_os = "macos")]
    paths.push(sources::ytdlp_setup::dir());
    if let Ok(joined) = std::env::join_paths(paths) {
        // SAFETY: runs first in main, before any other thread exists.
        unsafe { std::env::set_var("PATH", joined) };
    }
}

/// The running macOS major version, like 15, or 0 when it can't be read.
#[cfg(target_os = "macos")]
fn macos_major() -> u32 {
    let mut buf = [0u8; 32];
    let mut len = buf.len();
    // SAFETY: sysctlbyname writes at most `len` bytes into `buf` and updates `len`.
    let read = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return 0;
    }
    String::from_utf8_lossy(&buf[..len.min(buf.len())])
        .split(['.', '\0'])
        .next()
        .and_then(|major| major.parse().ok())
        .unwrap_or(0)
}

/// Writes panics to crash.log in the data folder too: Windows and macOS builds have
/// no terminal, so without it a crash leaves nothing to look at.
fn crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        let thread = std::thread::current().name().unwrap_or("main").to_owned();
        let path = model::paths::data_dir().join("crash.log");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(
                file,
                "Tonal {} thread '{thread}': {info}",
                env!("CARGO_PKG_VERSION")
            );
        }
        default(info);
    }));
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
    crash_log();
    let _ = rustls::crypto::ring::default_provider().install_default();
    app::run()
}
