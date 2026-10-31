//! Stands in for yt-dlp under mpv's ytdl hook. YouTube stream links only work
//! a few seconds after yt-dlp hands them out (`available_at`). yt-dlp waits
//! when it downloads; mpv fetches at once and gets HTTP 403. This runs the
//! real yt-dlp, waits until the chosen streams are available, then passes its
//! output on unchanged.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Path of the real yt-dlp; set on mpv, which hands it to this wrapper.
pub const REAL: &str = "TONAL_YTDL_REAL";

/// Runs as the yt-dlp wrapper when started by mpv, returning the exit code.
pub fn run() -> Option<i32> {
    let real = std::env::var_os(REAL)?;
    let output = std::process::Command::new(real)
        .args(std::env::args_os().skip(1))
        .env_remove(REAL)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            eprintln!("ERROR: could not run yt-dlp: {e}");
            return Some(1);
        }
    };
    if let Some(at) = available_at(&output.stdout) {
        std::thread::sleep(wait_for(at, now()));
    }
    use std::io::Write;
    let _ = std::io::stdout().write_all(&output.stdout);
    Some(output.status.code().unwrap_or(1))
}

/// Latest `available_at` (Unix seconds) among the formats yt-dlp picked in
/// its `-J` output.
pub fn available_at(json: &[u8]) -> Option<f64> {
    let info: serde_json::Value = serde_json::from_slice(json).ok()?;
    let mut picked: Vec<&serde_json::Value> = vec![&info];
    picked.extend(info["requested_formats"].as_array().into_iter().flatten());
    if let Some(id) = info["format_id"].as_str() {
        let formats = info["formats"].as_array().into_iter().flatten();
        picked.extend(formats.filter(|f| f["format_id"] == id));
    }
    picked
        .iter()
        .filter_map(|f| f["available_at"].as_f64())
        .reduce(f64::max)
}

/// Time left until `at`, plus a little slack, never more than 15 s.
pub fn wait_for(at: f64, now: f64) -> Duration {
    Duration::from_secs_f64((at - now + 0.5).clamp(0.0, 15.0))
}

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_the_latest_picked_stream() {
        let json = br#"{"format_id":"251","available_at":100,
            "formats":[{"format_id":"251","available_at":104},{"format_id":"18","available_at":900}]}"#;
        assert_eq!(available_at(json), Some(104.0));
        assert_eq!(available_at(b"{}"), None);
        assert_eq!(wait_for(104.0, 100.0), Duration::from_secs_f64(4.5));
        assert_eq!(wait_for(50.0, 100.0), Duration::ZERO);
        assert_eq!(wait_for(1e9, 0.0), Duration::from_secs(15));
    }
}
