use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", windows)))]
mod pipewire;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
use self::windows::Capture;
#[cfg(windows)]
pub use self::windows::available;
#[cfg(target_os = "macos")]
use macos::Capture;
#[cfg(target_os = "macos")]
pub use macos::available;
#[cfg(not(any(target_os = "macos", windows)))]
use pipewire::Capture;
#[cfg(not(any(target_os = "macos", windows)))]
pub use pipewire::available;

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
    pub back_line: String,
    pub haze: String,
    pub glow: String,
    pub line: String,
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
        self.shape(target, dt)
    }

    fn shape(&mut self, target: [f32; BANDS], dt: f32) -> WaveFrame {
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
        let back = self.points(&blurred, t * 0.7, 0.82, 0.8);
        let front = self.points(&blurred, t * 1.6 + 4.2, 1.0, 1.5);
        WaveFrame {
            back: area(&back, self.height + 1.0),
            front: area(&front, self.height + 1.0),
            back_line: ribbon(&back, 0.6, 0.6),
            haze: ribbon(&front, 5.0, 3.0),
            glow: ribbon(&front, 2.2, 1.6),
            line: ribbon(&front, 0.7, 0.7),
        }
    }

    fn points(&self, levels: &[f32; BANDS], phase: f32, scale: f32, ripple: f32) -> Points {
        use std::f32::consts::TAU;
        let over = self.width * 0.04;
        let span = self.width + 2.0 * over;
        let range = self.height * 0.8;
        let mut points = [(0.0f32, self.height); POINTS];
        for (i, point) in points.iter_mut().enumerate() {
            let x = i as f32 / (POINTS - 1) as f32;
            let pos = x * (BANDS - 1) as f32;
            let i0 = pos.floor() as usize;
            let i1 = (i0 + 1).min(BANDS - 1);
            let f = pos - i0 as f32;
            let level = (levels[i0] * (1.0 - f) + levels[i1] * f).powf(0.9) * 1.15;
            let swell = 0.72
                + 0.28
                    * (phase + x * 7.0 * ripple).sin()
                    * (0.6 + 0.4 * (phase * 0.37 + x * 3.1).cos());
            let flow = 0.5 + 0.5 * (x * TAU * 1.4 - self.time * 0.9 * ripple).sin();
            let value = (level * swell * (0.78 + 0.22 * flow)
                + self.presence * (0.04 + 0.06 * flow))
                * scale;
            *point = (
                -over + x * span,
                self.height - value.clamp(0.0, 1.0) * range,
            );
        }
        points
    }
}

type Points = [(f32, f32); POINTS];

/// Smooth curve through every point (Catmull-Rom as cubic Béziers), continuing from the
/// current position, which must already be at points[0].
fn curve(path: &mut String, points: &[(f32, f32)]) {
    use std::fmt::Write;
    let last = points.len() - 1;
    for i in 0..last {
        let p0 = points[i.saturating_sub(1)];
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = points[(i + 2).min(last)];
        let _ = write!(
            path,
            " C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2}",
            p1.0 + (p2.0 - p0.0) / 6.0,
            p1.1 + (p2.1 - p0.1) / 6.0,
            p2.0 - (p3.0 - p1.0) / 6.0,
            p2.1 - (p3.1 - p1.1) / 6.0,
            p2.0,
            p2.1
        );
    }
}

/// The wave filled down to `floor`.
fn area(points: &Points, floor: f32) -> String {
    use std::fmt::Write;
    let (first, last) = (points[0], points[POINTS - 1]);
    let mut path = format!(
        "M {:.2} {floor:.2} L {:.2} {:.2}",
        first.0, first.0, first.1
    );
    curve(&mut path, points);
    let _ = write!(path, " L {:.2} {floor:.2} Z", last.0);
    path
}

/// A closed band of even thickness along the wave's crest, `up` above it and `down`
/// below, measured along the curve's normal. Filled bands render identically on
/// every backend, unlike open stroked paths.
fn ribbon(points: &Points, up: f32, down: f32) -> String {
    use std::fmt::Write;
    let normal = |i: usize| {
        let a = points[i.saturating_sub(1)];
        let b = points[(i + 1).min(POINTS - 1)];
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
        (dy / len, -dx / len)
    };
    let mut top = [(0.0f32, 0.0f32); POINTS];
    let mut bottom = [(0.0f32, 0.0f32); POINTS];
    for i in 0..POINTS {
        let (nx, ny) = normal(i);
        let (x, y) = points[i];
        top[i] = (x + nx * up, y + ny * up);
        bottom[POINTS - 1 - i] = (x - nx * down, y - ny * down);
    }
    let mut path = format!("M {:.2} {:.2}", top[0].0, top[0].1);
    curve(&mut path, &top);
    let _ = write!(path, " L {:.2} {:.2}", bottom[0].0, bottom[0].1);
    curve(&mut path, &bottom);
    path.push_str(" Z");
    path
}

impl Default for Visualizer {
    fn default() -> Self {
        Self::new()
    }
}
