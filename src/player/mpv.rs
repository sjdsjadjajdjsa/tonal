use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{mpsc::UnboundedSender, oneshot},
    task::JoinHandle,
    time::{Duration, Instant},
};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

/// Linux kills mpv through PR_SET_PDEATHSIG and Windows through a Job Object
/// (src/job.rs). macOS has neither, so a small shell loop stops mpv once Tonal is
/// gone, even after Cmd+Q or a crash.
#[cfg(all(unix, not(target_os = "linux")))]
fn stop_with_parent(mpv: u32) {
    let script = format!(
        "while kill -0 {tonal} 2>/dev/null; do kill -0 {mpv} 2>/dev/null || exit 0; sleep 1; done; kill {mpv} 2>/dev/null",
        tonal = std::process::id()
    );
    let _ = Command::new("/bin/sh")
        .args(["-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
tokio::task_local! {
    pub(super) static SPAWN_FAILURES: std::cell::Cell<usize>;
}

#[derive(Debug)]
pub(super) struct Disconnected {
    pub instance: u64,
}

impl std::fmt::Display for Disconnected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mpv IPC disconnected")
    }
}

impl std::error::Error for Disconnected {}

pub(super) struct Message {
    pub instance: u64,
    pub value: Value,
}

/// How a new mpv instance is set up, from the engine's current settings.
pub(super) struct Launch {
    pub volume: f64,
    pub af: String,
    /// mpv device name, empty for the system default (or whatever mpv.conf says).
    pub device: String,
    pub replaygain: bool,
    pub bit_perfect: bool,
}

impl Launch {
    fn args(&self) -> Vec<String> {
        let mut args = vec![
            format!("--volume={}", self.volume),
            format!(
                "--gapless-audio={}",
                if self.bit_perfect { "yes" } else { "weak" }
            ),
            format!(
                "--replaygain={}",
                if self.replaygain && !self.bit_perfect {
                    "album"
                } else {
                    "no"
                }
            ),
            "--replaygain-fallback=0".into(),
            "--replaygain-clip=no".into(),
        ];
        if !self.device.is_empty() {
            args.push(format!("--audio-device={}", self.device));
        }
        if self.bit_perfect {
            // Exclusive mode is WASAPI exclusive on Windows and hog mode on macOS.
            // The sample format already stays native unless --audio-format is set.
            args.push("--audio-exclusive=yes".into());
            args.push("--audio-samplerate=0".into());
        } else if !self.af.is_empty() {
            args.push(format!("--af={}", self.af));
        }
        args
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
}

/// The devices of the audio backend mpv uses first (PipeWire, WASAPI, CoreAudio),
/// without mpv's "auto" and per-backend default entries.
pub(super) fn pick_devices(all: Vec<AudioDevice>) -> Vec<AudioDevice> {
    let Some(backend) = all
        .iter()
        .find(|d| d.name != "auto")
        .map(|d| d.name.split('/').next().unwrap_or_default().to_string())
    else {
        return Vec::new();
    };
    let prefix = format!("{backend}/");
    all.into_iter()
        .filter(|d| d.name.starts_with(&prefix))
        .collect()
}

pub(super) fn devices_from_list(list: &Value) -> Vec<AudioDevice> {
    pick_devices(
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| {
                Some(AudioDevice {
                    name: d["name"].as_str()?.to_string(),
                    description: d["description"].as_str().unwrap_or_default().to_string(),
                })
            })
            .collect(),
    )
}

/// Parses `mpv --audio-device=help`: lines like `  'name' (description)`.
pub(super) fn devices_from_help(text: &str) -> Vec<AudioDevice> {
    pick_devices(
        text.lines()
            .filter_map(|line| {
                let rest = line.trim().strip_prefix('\'')?;
                let (name, rest) = rest.split_once("' (")?;
                Some(AudioDevice {
                    name: name.to_string(),
                    description: rest.strip_suffix(')').unwrap_or(rest).to_string(),
                })
            })
            .collect(),
    )
}

/// Lists output devices without an IPC session, so Settings can show them before
/// anything has played.
pub(super) async fn audio_devices() -> Vec<AudioDevice> {
    let mut command = Command::new("mpv");
    command
        .arg("--audio-device=help")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::sources::CREATE_NO_WINDOW);
    match tokio::time::timeout(Duration::from_secs(5), command.output()).await {
        Ok(Ok(output)) => devices_from_help(&String::from_utf8_lossy(&output.stdout)),
        _ => Vec::new(),
    }
}

pub(super) struct Mpv {
    pub id: u64,
    child: Child,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
    reader: JoinHandle<()>,
    socket: Option<PathBuf>,
    requests: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    request_id: u64,
    pub entry_id: Option<i64>,
    alive: Arc<AtomicBool>,
    playback: Arc<Mutex<(Option<i64>, Option<f64>)>>,
}

impl Mpv {
    pub async fn spawn(events: UnboundedSender<Message>, launch: &Launch) -> Result<Self> {
        let id = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        #[cfg(unix)]
        let endpoint = std::env::temp_dir().join(format!("tonal-{}-{id}.sock", std::process::id()));
        #[cfg(windows)]
        let endpoint = PathBuf::from(format!(r"\\.\pipe\tonal-{}-{id}", std::process::id()));
        let mut command = Command::new("mpv");
        command.args([
            "--idle=yes",
            "--no-video",
            "--no-terminal",
            // yt-dlp's default sort already ranks audio by the site's quality
            // field, then codec (lossless, Opus, Vorbis, AAC, MP3), then bitrate,
            // and drops DRC variants. That picks Opus 251 (or premium 774) on
            // YouTube and AAC 160k over Opus 64k on SoundCloud, so no -S here.
            "--ytdl-format=bestaudio/best",
            "--cache=yes",
            "--demuxer-max-bytes=16MiB",
            "--demuxer-max-back-bytes=8MiB",
            "--prefetch-playlist=yes",
        ]);
        command.arg(format!("--input-ipc-server={}", endpoint.display()));
        command.args(launch.args());
        command.arg(format!(
            "--ytdl-raw-options={}",
            crate::sources::ytdl_raw_options()
        ));
        command.arg(format!(
            "--audio-client-name=tonal-{}-{id}",
            std::process::id()
        ));
        // The hook runs Tonal as its yt-dlp: the same yt-dlp Tonal probes with,
        // and it waits until YouTube's stream links work before mpv opens them.
        #[cfg(not(test))]
        if let (Some(real), Some(wrapper)) = (
            crate::sources::find_on_path("yt-dlp"),
            std::env::current_exe()
                .ok()
                .and_then(|path| path.to_str().map(str::to_owned))
                .filter(|path| !path.contains(',')),
        ) {
            command.arg(format!("--script-opts=ytdl_hook-ytdl_path={wrapper}"));
            command.env(crate::ytdl_wait::REAL, real);
        }
        // Deno under the hook must never stop to ask a question.
        command
            .env("DENO_NO_PROMPT", "1")
            .env("DENO_NO_UPDATE_CHECK", "1");

        #[cfg(test)]
        command.arg("--ao=null");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(crate::sources::CREATE_NO_WINDOW);
        #[cfg(target_os = "linux")]
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        let mut child = command
            .spawn()
            .context("could not start mpv (is it installed?)")?;
        #[cfg(all(unix, not(target_os = "linux")))]
        if let Some(pid) = child.id() {
            stop_with_parent(pid);
        }
        #[cfg(windows)]
        crate::job::adopt(child.raw_handle());
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            #[cfg(unix)]
            let result = tokio::net::UnixStream::connect(&endpoint).await;
            #[cfg(windows)]
            let result = tokio::net::windows::named_pipe::ClientOptions::new().open(&endpoint);
            match result {
                Ok(stream) => break stream,
                Err(error) => {
                    if child.try_wait()?.is_some() || Instant::now() >= deadline {
                        let _ = child.kill().await;
                        #[cfg(unix)]
                        let _ = std::fs::remove_file(&endpoint);
                        return Err(Disconnected { instance: id })
                            .context(format!("mpv IPC connection failed: {error}"));
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        };
        let (read, write) = tokio::io::split(stream);
        let requests = Arc::new(Mutex::new(HashMap::<u64, oneshot::Sender<Value>>::new()));
        let responses = requests.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let reader_alive = alive.clone();
        let playback = Arc::new(Mutex::new((None, None)));
        let reader_playback = playback.clone();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(read).lines();
            let mut entry_id = None;
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if let Ok(mut value) = serde_json::from_str::<Value>(&line) {
                            if let Some(request) = value["request_id"].as_u64() {
                                let reply = responses
                                    .lock()
                                    .expect("mpv requests poisoned")
                                    .remove(&request);
                                if let Some(reply) = reply {
                                    let _ = reply.send(value);
                                    continue;
                                }
                            }
                            if value["event"] == "start-file" {
                                entry_id = value["playlist_entry_id"].as_i64();
                                *reader_playback.lock().expect("mpv playback poisoned") =
                                    (entry_id, None);
                            }
                            if value["event"] == "property-change"
                                && value["name"] == "time-pos"
                                && let Some(position) = value["data"].as_f64()
                            {
                                reader_playback.lock().expect("mpv playback poisoned").1 =
                                    Some(position);
                            }
                            // Property observations do not carry mpv's entry ID. Stamp them
                            // in wire order, before engine commands can overtake queued events.
                            if matches!(
                                value["event"].as_str(),
                                Some("property-change" | "file-loaded")
                            ) {
                                value["playback_entry_id"] = json!(entry_id);
                            }
                            if events
                                .send(Message {
                                    instance: id,
                                    value,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    result => {
                        reader_alive.store(false, Ordering::Release);
                        // Wake in-flight requests immediately, rather than waiting for timeout.
                        responses.lock().expect("mpv requests poisoned").clear();
                        let error = match result {
                            Err(error) => error.to_string(),
                            _ => "mpv IPC disconnected".to_owned(),
                        };
                        let _ = events.send(Message {
                            instance: id,
                            value: json!({"event":"ipc-error", "error":error}),
                        });
                        break;
                    }
                }
            }
        });
        let mut mpv = Self {
            id,
            child,
            writer: Box::new(write),
            reader,
            requests,
            request_id: 0,
            entry_id: None,
            alive,
            playback,
            #[cfg(unix)]
            socket: Some(endpoint),
            #[cfg(windows)]
            socket: None,
        };
        // Fault injection kills only this task's newly spawned real child, before
        // the observation writes that can fail before Engine owns the instance.
        #[cfg(test)]
        if SPAWN_FAILURES
            .try_with(|remaining| {
                let count = remaining.get();
                remaining.set(count.saturating_sub(1));
                count > 0
            })
            .unwrap_or(false)
        {
            mpv.child
                .kill()
                .await
                .expect("kill test-owned spawning mpv");
            (&mut mpv.reader).await.expect("test mpv reader EOF");
        }
        for (index, property) in [
            "time-pos",
            "duration",
            "pause",
            "paused-for-cache",
            "core-idle",
            "volume",
            "playlist",
            "audio-codec-name",
            "audio-params",
            "audio-bitrate",
            "file-format",
            "current-tracks/audio",
            "metadata",
            "audio-device-list",
        ]
        .iter()
        .enumerate()
        {
            mpv.send(json!(["observe_property", index + 1, property]))
                .await?;
        }
        Ok(mpv)
    }

    // Snapshot in wire order, even when a failed command overtakes queued events.
    pub fn playback(&self) -> (Option<i64>, Option<f64>) {
        *self.playback.lock().expect("mpv playback poisoned")
    }

    fn disconnected(&self) -> anyhow::Error {
        self.alive.store(false, Ordering::Release);
        Disconnected { instance: self.id }.into()
    }

    pub async fn send(&mut self, command: Value) -> Result<()> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(self.disconnected());
        }
        let mut line = serde_json::to_vec(&json!({"command":command}))?;
        line.push(b'\n');
        self.writer
            .write_all(&line)
            .await
            .map_err(|_| self.disconnected())
    }

    pub async fn request(&mut self, command: Value) -> Result<Value> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(self.disconnected());
        }
        self.request_id += 1;
        let id = self.request_id;
        let (tx, rx) = oneshot::channel();
        self.requests
            .lock()
            .expect("mpv requests poisoned")
            .insert(id, tx);
        let mut line = serde_json::to_vec(&json!({"command":command, "request_id":id}))?;
        line.push(b'\n');
        let result = async {
            self.writer
                .write_all(&line)
                .await
                .map_err(|_| self.disconnected())?;
            let reply = tokio::time::timeout(Duration::from_secs(5), rx)
                .await?
                .map_err(|_| self.disconnected())?;
            if reply["error"] != "success" {
                anyhow::bail!("mpv: {}", reply["error"]);
            }
            Ok(reply["data"].clone())
        }
        .await;
        self.requests
            .lock()
            .expect("mpv requests poisoned")
            .remove(&id);
        result
    }

    pub async fn load(&mut self, url: &str, start: Option<f64>) -> Result<()> {
        // URLs are JSON arguments, never command-line options (including leading '-').
        match start {
            Some(secs) => {
                let start = format!("start={secs:.3}");
                if self
                    .request(json!(["loadfile", url, "replace", -1, start.clone()]))
                    .await
                    .is_err()
                {
                    self.request(json!(["loadfile", url, "replace", start]))
                        .await?;
                }
            }
            None => {
                self.request(json!(["loadfile", url, "replace"])).await?;
            }
        };
        let playlist = self.request(json!(["get_property", "playlist"])).await?;
        self.entry_id = playlist
            .as_array()
            .and_then(|entries| entries.first())
            .and_then(|entry| entry["id"].as_i64());
        Ok(())
    }

    #[cfg(test)]
    pub async fn kill_for_test(&mut self) {
        self.child.kill().await.expect("kill test-owned mpv");
    }

    pub async fn shutdown(&mut self) {
        self.reader.abort();
        let _ = self.child.kill().await;
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        self.reader.abort();
        let _ = self.child.start_kill();
        if let Some(path) = &self.socket {
            let _ = std::fs::remove_file(path);
        }
    }
}
