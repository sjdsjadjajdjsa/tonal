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
}

impl Mpv {
    pub async fn spawn(
        events: UnboundedSender<Message>,
        volume: f64,
        normalize: bool,
    ) -> Result<Self> {
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
            "--ytdl-format=bestaudio/best",
            "--cache=yes",
            "--gapless-audio=weak",
            "--prefetch-playlist=yes",
        ]);
        command.arg(format!("--input-ipc-server={}", endpoint.display()));
        command.arg(format!("--volume={volume}"));
        command.arg(format!(
            "--ytdl-raw-options={}",
            crate::sources::ytdl_raw_options()
        ));
        command.arg(format!(
            "--audio-client-name=tonal-{}-{id}",
            std::process::id()
        ));
        if normalize {
            command.arg("--af=dynaudnorm=f=250:g=15");
        }
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
                        return Err(error).context("mpv IPC connection failed");
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
            #[cfg(unix)]
            socket: Some(endpoint),
            #[cfg(windows)]
            socket: None,
        };
        for (index, property) in [
            "time-pos",
            "duration",
            "pause",
            "paused-for-cache",
            "core-idle",
            "volume",
            "playlist",
        ]
        .iter()
        .enumerate()
        {
            mpv.send(json!(["observe_property", index + 1, property]))
                .await?;
        }
        Ok(mpv)
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
