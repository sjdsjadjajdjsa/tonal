//! "Listening to …" on your Discord profile, through the Discord app running on this
//! computer. Builds get the Discord application id at compile time from
//! `TONAL_DISCORD_APP`; without it the feature is off.

use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{Activity, ActivityType, Assets, StatusDisplayType, Timestamps},
};
use std::{
    sync::{
        OnceLock,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::Duration,
};

const APP: Option<&str> = option_env!("TONAL_DISCORD_APP");
/// How often to look for Discord again while it isn't running.
const RETRY: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq)]
pub struct Presence {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
    /// When the song started and will end, in ms since the epoch.
    pub start: i64,
    pub end: Option<i64>,
}

/// Whether this build can talk to Discord.
pub fn available() -> bool {
    APP.is_some_and(|a| !a.is_empty())
}

/// Shows `presence` on your profile, or clears it with None.
pub fn show(presence: Option<Presence>) {
    let Some(app) = APP.filter(|a| !a.is_empty()) else {
        return;
    };
    static TX: OnceLock<Sender<Option<Presence>>> = OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("discord".into())
            .spawn(move || run(app, rx));
        tx
    });
    let _ = tx.send(presence);
}

fn run(app: &str, rx: Receiver<Option<Presence>>) {
    let mut client: Option<DiscordIpcClient> = None;
    let mut wanted: Option<Presence> = None;
    let mut shown = false;
    loop {
        let wait = if client.is_none() && wanted.is_some() {
            RETRY
        } else {
            Duration::from_secs(3600)
        };
        match rx.recv_timeout(wait) {
            Ok(next) => wanted = next,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(next) = rx.try_recv() {
            wanted = next;
        }
        if client.is_none() {
            if wanted.is_none() {
                continue;
            }
            let mut c = DiscordIpcClient::new(app);
            if c.connect().is_err() {
                continue;
            }
            client = Some(c);
            shown = false;
        }
        let Some(c) = client.as_mut() else {
            continue;
        };
        let sent = match &wanted {
            Some(p) => c.set_activity(activity(p)),
            None if shown => c.clear_activity(),
            None => Ok(()),
        };
        match sent {
            Ok(()) => shown = wanted.is_some(),
            Err(e) => {
                log::debug!("Discord: {e}");
                let _ = c.close();
                client = None;
            }
        }
    }
    if let Some(mut c) = client {
        let _ = c.close();
    }
}

/// Discord wants 2 to 128 characters in each line.
fn line(text: &str) -> String {
    let text: String = text.trim().chars().take(128).collect();
    if text.chars().count() < 2 {
        format!("{text:<2}")
    } else {
        text
    }
}

fn activity(p: &Presence) -> Activity<'static> {
    let mut timestamps = Timestamps::new().start(p.start);
    if let Some(end) = p.end {
        timestamps = timestamps.end(end);
    }
    let mut activity = Activity::new()
        .activity_type(ActivityType::Listening)
        .status_display_type(StatusDisplayType::Details)
        .details(line(&p.title))
        .state(line(&p.artist))
        .timestamps(timestamps);
    if let Some(url) = p
        .artwork_url
        .as_ref()
        .filter(|u| u.starts_with("https://") && u.len() <= 256)
    {
        let mut assets = Assets::new().large_image(url.clone());
        if let Some(album) = p.album.as_ref().filter(|a| !a.trim().is_empty()) {
            assets = assets.large_text(line(album));
        }
        activity = activity.assets(assets);
    }
    activity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_fit_what_discord_accepts() {
        assert_eq!(line("A"), "A ");
        assert_eq!(line("  Song  "), "Song");
        assert_eq!(line(&"x".repeat(300)).len(), 128);
    }
}
