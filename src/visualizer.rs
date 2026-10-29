use parking_lot::Mutex;
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const RATE: f32 = 48_000.0;
const FFT_SIZE: usize = 2048;
const BANDS: usize = 36;
const POINTS: usize = 40;
const LOW_HZ: f32 = 35.0;
const HIGH_HZ: f32 = 12_000.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaveFrame {
    pub back: String,
    pub front: String,
    pub line: String,
}

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

struct Capture {
    active: Arc<AtomicBool>,
    streams: Arc<Mutex<HashMap<String, Stream>>>,
}

impl Capture {
    fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn set_active(&self, on: bool) {
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

    fn mix_into(&self, out: &mut [f32]) -> bool {
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

pub struct Visualizer {
    capture: Capture,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    samples: Vec<f32>,
    spectrum: Vec<Complex32>,
    edges: Vec<(usize, usize)>,
    levels: [f32; BANDS],
    gain: f32,
    time: f32,
    energy: f32,
    playing: bool,
    width: f32,
    height: f32,
    presence: f32,
}

impl Visualizer {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE)
            .map(|i| {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos()
            })
            .collect();
        let bin_hz = RATE / FFT_SIZE as f32;
        let edges = (0..BANDS)
            .map(|b| {
                let lo = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(b as f32 / BANDS as f32);
                let hi = LOW_HZ * (HIGH_HZ / LOW_HZ).powf((b + 1) as f32 / BANDS as f32);
                let lo_bin = ((lo / bin_hz) as usize).max(1);
                let hi_bin = ((hi / bin_hz) as usize).max(lo_bin + 1).min(FFT_SIZE / 2);
                (lo_bin, hi_bin)
            })
            .collect();
        Self {
            capture: Capture::new(),
            fft,
            window,
            samples: vec![0.0; FFT_SIZE],
            spectrum: vec![Complex32::default(); FFT_SIZE],
            edges,
            levels: [0.0; BANDS],
            gain: 6.0,
            time: 0.0,
            energy: 0.0,
            playing: false,
            width: 1000.0,
            height: 60.0,
            presence: 0.0,
        }
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        self.capture.set_active(playing);
    }

    pub fn is_settled(&self) -> bool {
        !self.playing && self.energy < 0.002
    }

    pub fn frame(&mut self, dt: f32, width: f32, height: f32) -> WaveFrame {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        let dt = dt.clamp(0.001, 0.1);
        self.time += dt;
        let mut target = [0.0f32; BANDS];
        if self.playing && self.capture.mix_into(&mut self.samples) {
            for (i, c) in self.spectrum.iter_mut().enumerate() {
                *c = Complex32::new(self.samples[i] * self.window[i], 0.0);
            }
            self.fft.process(&mut self.spectrum);
            let mut peak = 0.0f32;
            for (b, &(lo, hi)) in self.edges.iter().enumerate() {
                let sum: f32 = self.spectrum[lo..hi].iter().map(|c| c.norm()).sum();
                let mean = sum / (hi - lo) as f32;
                let tilt = 1.0 + 6.0 * (b as f32 / BANDS as f32).powf(1.5);
                target[b] = (mean * tilt / FFT_SIZE as f32).sqrt() * self.gain;
                peak = peak.max(target[b]);
            }
            if peak > 0.95 {
                self.gain *= (0.95 / peak).powf(dt * 6.0);
            } else if peak > 0.02 {
                self.gain *= 1.0 + 0.25 * dt;
            }
            self.gain = self.gain.clamp(0.2, 60.0);
            target.iter_mut().for_each(|v| *v = v.min(1.0));
        }
        let rise = 1.0 - (-dt * 12.0).exp();
        let fall = 1.0 - (-dt * 3.5).exp();
        let mut energy = 0.0;
        for (level, t) in self.levels.iter_mut().zip(target) {
            let k = if t > *level { rise } else { fall };
            *level += (t - *level) * k;
            energy += *level;
        }
        self.energy = energy / BANDS as f32;
        let mut blurred = self.levels;
        for _ in 0..3 {
            let src = blurred;
            for i in 0..BANDS {
                let l = src[i.saturating_sub(1)];
                let r = src[(i + 1).min(BANDS - 1)];
                blurred[i] = 0.25 * l + 0.5 * src[i] + 0.25 * r;
            }
        }
        let t = self.time;
        self.presence = (self.energy * 8.0).min(1.0);
        WaveFrame {
            back: self.layer(&blurred, t * 0.7, 0.82, 0.8, true),
            front: self.layer(&blurred, t * 1.6 + 4.2, 1.0, 1.5, true),
            line: self.layer(&blurred, t * 1.6 + 4.2, 1.0, 1.5, false),
        }
    }

    fn layer(
        &self,
        levels: &[f32; BANDS],
        phase: f32,
        scale: f32,
        ripple: f32,
        closed: bool,
    ) -> String {
        use std::f32::consts::TAU;
        let over = self.width * 0.04;
        let span = self.width + 2.0 * over;
        let base = (self.height * 0.22).min(14.0);
        let floor = self.height - base;
        let range = (floor - 3.0) * 0.62;
        let mut points = [(0.0f32, self.height); POINTS];
        for (i, point) in points.iter_mut().enumerate() {
            let x = i as f32 / (POINTS - 1) as f32;
            let pos = x * (BANDS - 1) as f32;
            let i0 = pos.floor() as usize;
            let i1 = (i0 + 1).min(BANDS - 1);
            let f = pos - i0 as f32;
            let level = (levels[i0] * (1.0 - f) + levels[i1] * f).powf(1.2) * 1.2;
            let swell = 0.72
                + 0.28
                    * (phase + x * 7.0 * ripple).sin()
                    * (0.6 + 0.4 * (phase * 0.37 + x * 3.1).cos());
            let flow = 0.5 + 0.5 * (x * TAU * 1.4 - self.time * 0.9 * ripple).sin();
            let value = (level * swell * (0.78 + 0.22 * flow)
                + self.presence * (0.04 + 0.06 * flow))
                * scale;
            *point = (-over + x * span, floor - value.clamp(0.0, 1.0) * range);
        }
        let mut path = String::with_capacity(POINTS * 48);
        use std::fmt::Write;
        if closed {
            let _ = write!(
                path,
                "M {:.1} {:.1} L {:.1} {:.1}",
                points[0].0, floor, points[0].0, points[0].1
            );
        } else {
            let _ = write!(path, "M {:.1} {:.1}", points[0].0, points[0].1);
        }
        for i in 0..POINTS - 1 {
            let p0 = points[i.saturating_sub(1)];
            let p1 = points[i];
            let p2 = points[i + 1];
            let p3 = points[(i + 2).min(POINTS - 1)];
            let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
            let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
            let _ = write!(
                path,
                " C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
                c1.0, c1.1, c2.0, c2.1, p2.0, p2.1
            );
        }
        if closed {
            let _ = write!(path, " L {:.1} {:.1} Z", points[POINTS - 1].0, floor);
        }
        path
    }
}

impl Default for Visualizer {
    fn default() -> Self {
        Self::new()
    }
}
