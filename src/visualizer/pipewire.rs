use super::FFT_SIZE;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

struct Stream {
    child: Child,
    samples: Arc<Mutex<VecDeque<f32>>>,
    last_data: Arc<Mutex<Instant>>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) struct Capture {
    active: Arc<AtomicBool>,
    streams: Arc<Mutex<HashMap<String, Stream>>>,
}

impl Capture {
    pub(super) fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(super) fn set_active(&self, on: bool) {
        if self.active.swap(on, Ordering::SeqCst) == on {
            return;
        }
        if !on {
            self.streams.lock().clear();
            return;
        }
        let active = self.active.clone();
        let streams = self.streams.clone();
        let spawned = std::thread::Builder::new()
            .name("visualizer-capture".into())
            .spawn(move || {
                let prefix = format!("tonal-{}-", std::process::id());
                let mut wait = Duration::from_millis(300);
                while active.load(Ordering::SeqCst) {
                    let names = discover(&prefix);
                    {
                        let mut map = streams.lock();
                        map.retain(|name, stream| {
                            names.contains(name)
                                && stream.child.try_wait().ok().flatten().is_none()
                                && stream.last_data.lock().elapsed() < Duration::from_secs(3)
                        });
                        if !active.load(Ordering::SeqCst) {
                            map.clear();
                            break;
                        }
                        for name in &names {
                            if !map.contains_key(name)
                                && let Some(stream) = record(name)
                            {
                                map.insert(name.clone(), stream);
                            }
                        }
                    }
                    std::thread::sleep(wait);
                    wait = (wait * 2).min(Duration::from_secs(2));
                }
                streams.lock().clear();
            });
        if spawned.is_err() {
            self.active.store(false, Ordering::SeqCst);
        }
    }

    pub(super) fn mix_into(&self, out: &mut [f32]) -> bool {
        out.iter_mut().for_each(|s| *s = 0.0);
        let map = self.streams.lock();
        let mut any = false;
        for stream in map.values() {
            let samples = stream.samples.lock();
            if samples.len() < out.len() {
                continue;
            }
            any = true;
            let start = samples.len() - out.len();
            for (o, s) in out.iter_mut().zip(samples.range(start..)) {
                *o += *s;
            }
        }
        any
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        self.streams.lock().clear();
    }
}

trait DieWithParent {
    fn die_with_parent(&mut self) -> &mut Self;
}

impl DieWithParent for Command {
    fn die_with_parent(&mut self) -> &mut Self {
        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::process::CommandExt;
            self.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        self
    }
}

fn discover(prefix: &str) -> Vec<String> {
    let Ok(output) = Command::new("pw-dump")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let Ok(objects) = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout) else {
        return Vec::new();
    };
    let mut names: Vec<String> = objects
        .iter()
        .filter_map(|o| {
            let props = o.get("info")?.get("props")?;
            if props.get("media.class")?.as_str()? != "Stream/Output/Audio" {
                return None;
            }
            let name = props.get("node.name")?.as_str()?;
            name.starts_with(prefix).then(|| name.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

fn record(name: &str) -> Option<Stream> {
    let mut child = Command::new("pw-record")
        .args(["--target", name, "--format", "f32", "--rate", "48000", "--channels", "1", "--latency", "20ms", "--raw"])
        .args(["-P", "{ node.dont-fallback = true node.dont-reconnect = true node.passive = true node.name = tonal-visualizer }"])
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .die_with_parent()
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let samples = Arc::new(Mutex::new(VecDeque::with_capacity(FFT_SIZE * 2)));
    let last_data = Arc::new(Mutex::new(Instant::now()));
    let (buffer, stamp) = (samples.clone(), last_data.clone());
    let reader = std::thread::Builder::new()
        .name("visualizer-reader".into())
        .spawn(move || {
            let mut chunk = [0u8; 4096];
            let mut carry: Vec<u8> = Vec::with_capacity(4);
            loop {
                let n = match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                carry.extend_from_slice(&chunk[..n]);
                let whole = carry.len() / 4 * 4;
                {
                    let mut ring = buffer.lock();
                    for bytes in carry[..whole].as_chunks::<4>().0 {
                        ring.push_back(f32::from_le_bytes([
                            bytes[0], bytes[1], bytes[2], bytes[3],
                        ]));
                    }
                    let excess = ring.len().saturating_sub(FFT_SIZE * 2);
                    ring.drain(..excess);
                }
                carry.drain(..whole);
                *stamp.lock() = Instant::now();
            }
        });
    if reader.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    Some(Stream {
        child,
        samples,
        last_data,
    })
}

pub fn available() -> bool {
    cfg!(target_os = "linux")
        && ["pw-record", "pw-dump"].iter().all(|tool| {
            std::env::var_os("PATH")
                .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(tool).is_file()))
                .unwrap_or(false)
        })
}
