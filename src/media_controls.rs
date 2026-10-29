use crate::model::Track;
use crate::player::PlayerHandle;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub enum ShellRequest {
    Raise,
    Quit,
}

enum Update {
    Track(Option<Box<Track>>),
    Playback { playing: bool, position: f64 },
    Volume(f64),
}

#[derive(Clone)]
pub struct MediaControlsHandle {
    tx: mpsc::Sender<Update>,
}

impl MediaControlsHandle {
    pub fn set_track(&self, track: Option<&Track>) {
        let _ = self.tx.send(Update::Track(track.cloned().map(Box::new)));
    }

    pub fn set_playback(&self, playing: bool, position: f64) {
        let _ = self.tx.send(Update::Playback { playing, position });
    }

    pub fn set_volume(&self, volume: f64) {
        let _ = self.tx.send(Update::Volume(volume));
    }
}

fn position(secs: f64) -> Option<MediaPosition> {
    Some(MediaPosition(Duration::from_secs_f64(secs.max(0.0))))
}

fn cover_url(track: &Track) -> Option<String> {
    let url = track.artwork_url.as_deref()?;
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
        Some(url.to_string())
    } else {
        Some(format!("file://{url}"))
    }
}

fn handle_event(
    player: &PlayerHandle,
    shell: &(dyn Fn(ShellRequest) + Send + Sync),
    event: MediaControlEvent,
) {
    match event {
        MediaControlEvent::Play => player.set_paused(false),
        MediaControlEvent::Pause => player.set_paused(true),
        MediaControlEvent::Toggle => player.toggle_pause(),
        MediaControlEvent::Next => player.next(),
        MediaControlEvent::Previous => player.prev(),
        MediaControlEvent::Stop => player.stop(),
        MediaControlEvent::Seek(SeekDirection::Forward) => player.seek_relative(10.0),
        MediaControlEvent::Seek(SeekDirection::Backward) => player.seek_relative(-10.0),
        MediaControlEvent::SeekBy(SeekDirection::Forward, by) => {
            player.seek_relative(by.as_secs_f64())
        }
        MediaControlEvent::SeekBy(SeekDirection::Backward, by) => {
            player.seek_relative(-by.as_secs_f64())
        }
        MediaControlEvent::SetPosition(MediaPosition(at)) => player.seek(at.as_secs_f64()),
        MediaControlEvent::SetVolume(volume) => {
            player.set_volume((volume * 100.0).clamp(0.0, 100.0))
        }
        MediaControlEvent::SetShuffle(on) => player.set_shuffle(on),
        MediaControlEvent::Raise => shell(ShellRequest::Raise),
        MediaControlEvent::Quit => shell(ShellRequest::Quit),
        _ => {}
    }
}

pub fn spawn(
    player: PlayerHandle,
    hwnd: Option<*mut std::ffi::c_void>,
    shell: impl Fn(ShellRequest) + Send + Sync + 'static,
) -> MediaControlsHandle {
    let (tx, rx) = mpsc::channel::<Update>();
    let hwnd = hwnd.map(|h| h as usize);
    let spawned = std::thread::Builder::new()
        .name("media-controls".into())
        .spawn(move || {
            let config = PlatformConfig {
                display_name: "Tonal",
                dbus_name: "tonal",
                hwnd: hwnd.map(|h| h as *mut std::ffi::c_void),
            };
            let mut controls = match MediaControls::new(config) {
                Ok(controls) => controls,
                Err(err) => {
                    log::warn!("media controls unavailable: {err:?}");
                    return;
                }
            };
            if let Err(err) = controls.attach(move |event| handle_event(&player, &shell, event)) {
                log::warn!("media controls attach failed: {err:?}");
                return;
            }
            for update in rx {
                let result = match update {
                    Update::Track(Some(track)) => {
                        let cover = cover_url(&track);
                        controls.set_metadata(MediaMetadata {
                            title: Some(&track.title),
                            artist: Some(&track.artist),
                            album: track.album.as_deref(),
                            cover_url: cover.as_deref(),
                            duration: track
                                .duration_secs
                                .filter(|d| d.is_finite() && *d > 0.0)
                                .map(Duration::from_secs_f64),
                        })
                    }
                    Update::Track(None) => controls.set_playback(MediaPlayback::Stopped),
                    Update::Playback {
                        playing,
                        position: at,
                    } => controls.set_playback(if playing {
                        MediaPlayback::Playing {
                            progress: position(at),
                        }
                    } else {
                        MediaPlayback::Paused {
                            progress: position(at),
                        }
                    }),
                    Update::Volume(volume) => volume_update(&mut controls, volume),
                };
                if let Err(err) = result {
                    log::debug!("media controls update failed: {err:?}");
                }
            }
        });
    if let Err(err) = spawned {
        log::warn!("media controls thread failed: {err}");
    }
    MediaControlsHandle { tx }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn volume_update(controls: &mut MediaControls, volume: f64) -> Result<(), souvlaki::Error> {
    controls.set_volume((volume / 100.0).clamp(0.0, 1.0))
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn volume_update(_controls: &mut MediaControls, _volume: f64) -> Result<(), souvlaki::Error> {
    Ok(())
}
