//! Short interface sounds, made in code and played on a small output of their own
//! next to mpv. The output opens on first use and closes again once it is quiet.

use cpal::{
    FromSample, OutputCallbackInfo, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use parking_lot::Mutex;
use std::{
    f32::consts::TAU,
    sync::{
        Arc, OnceLock,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::Duration,
};

/// How long the output stays open after the last sound.
const IDLE: Duration = Duration::from_secs(3);

/// The interface's other sounds, each soft and short.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    /// Two bell notes going up.
    Like,
    /// One bell note, low.
    Unlike,
    /// A single soft wooden tap.
    Queued,
    /// Two taps going up.
    Added,
    /// Three quick bell notes, a little flourish.
    Saved,
}

#[derive(Clone, Copy)]
enum Shape {
    /// A sine that slides up as it fades, with a soft overtone.
    Bubble,
    /// A sine with a faint inharmonic partial, like a small glass bell.
    Bell,
    /// A short knock whose pitch drops, like tapping wood.
    Tap,
}

impl Shape {
    fn length(self) -> f32 {
        match self {
            Shape::Bubble => 0.16,
            Shape::Bell => 0.5,
            Shape::Tap => 0.09,
        }
    }
}

struct Voice {
    shape: Shape,
    freq: f32,
    level: f32,
    /// Seconds since it started; below 0 while it waits its turn.
    t: f32,
    phase: f32,
    partial: f32,
}

impl Voice {
    fn new(shape: Shape, freq: f32, level: f32, delay: f32) -> Self {
        Self {
            shape,
            freq,
            level,
            t: -delay,
            phase: 0.0,
            partial: 0.0,
        }
    }

    fn done(&self) -> bool {
        self.t >= self.shape.length()
    }

    fn next(&mut self, rate: f32) -> f32 {
        let t = self.t;
        self.t += 1.0 / rate;
        if t < 0.0 || t >= self.shape.length() {
            return 0.0;
        }
        let (freq, attack, decay) = match self.shape {
            Shape::Bubble => (
                self.freq * (1.0 + 0.9 * (1.0 - (-t / 0.03).exp())),
                0.002,
                (-t / 0.04).exp(),
            ),
            Shape::Bell => (self.freq, 0.004, (-t / 0.14).exp()),
            Shape::Tap => (
                self.freq * (1.0 + 0.6 * (-t / 0.008).exp()),
                0.001,
                (-t / 0.022).exp(),
            ),
        };
        self.phase = (self.phase + freq / rate).fract();
        let ratio = match self.shape {
            Shape::Bell => 2.76,
            _ => 2.0,
        };
        self.partial = (self.partial + ratio * freq / rate).fract();
        let overtone = match self.shape {
            Shape::Bubble => 0.18,
            Shape::Bell => 0.22 * (-t / 0.05).exp(),
            Shape::Tap => 0.35,
        };
        let tone = (self.phase * TAU).sin() + overtone * (self.partial * TAU).sin();
        tone * (t / attack).min(1.0) * decay * self.level
    }
}

type Voices = Arc<Mutex<Vec<Voice>>>;

static OUT: OnceLock<Sender<Vec<Voice>>> = OnceLock::new();

fn send(voices: Vec<Voice>) {
    let tx = OUT.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("sfx".into())
            .spawn(move || run(rx));
        tx
    });
    let _ = tx.send(voices);
}

/// Opens the output ahead of a sound so the first one isn't late.
pub fn warm() {
    send(Vec::new());
}

/// Plays one bubble at `freq` Hz; `level` is 0 to 1.
pub fn bubble(freq: f32, level: f32) {
    if level > 0.0 {
        send(vec![Voice::new(Shape::Bubble, freq, level, 0.0)]);
    }
}

/// Plays `sound`; `level` is 0 to 1.
pub fn play(sound: Sound, level: f32) {
    if level <= 0.0 {
        return;
    }
    let notes: &[(Shape, f32, f32, f32)] = match sound {
        Sound::Like => &[
            (Shape::Bell, 1318.5, 0.5, 0.0),
            (Shape::Bell, 1975.5, 0.45, 0.07),
        ],
        Sound::Unlike => &[(Shape::Bell, 987.8, 0.4, 0.0)],
        Sound::Queued => &[(Shape::Tap, 740.0, 0.9, 0.0)],
        Sound::Added => &[
            (Shape::Tap, 659.3, 0.8, 0.0),
            (Shape::Tap, 987.8, 0.8, 0.06),
        ],
        Sound::Saved => &[
            (Shape::Bell, 1046.5, 0.4, 0.0),
            (Shape::Bell, 1318.5, 0.4, 0.06),
            (Shape::Bell, 1568.0, 0.4, 0.12),
        ],
    };
    send(
        notes
            .iter()
            .map(|&(shape, freq, gain, delay)| Voice::new(shape, freq, gain * level, delay))
            .collect(),
    );
}

fn run(rx: Receiver<Vec<Voice>>) {
    let voices: Voices = Arc::default();
    while let Ok(new) = rx.recv() {
        voices.lock().extend(new);
        let stream = open(voices.clone());
        loop {
            match rx.recv_timeout(IDLE) {
                Ok(new) => voices.lock().extend(new),
                Err(RecvTimeoutError::Timeout) if voices.lock().is_empty() => break,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        drop(stream);
        voices.lock().clear();
    }
}

fn open(voices: Voices) -> Option<Stream> {
    let device = cpal::default_host().default_output_device()?;
    let config = device.default_output_config().ok()?;
    let format = config.sample_format();
    let config: StreamConfig = config.into();
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, voices),
        SampleFormat::I16 => build::<i16>(&device, config, voices),
        SampleFormat::I32 => build::<i32>(&device, config, voices),
        SampleFormat::U16 => build::<u16>(&device, config, voices),
        _ => None,
    }?;
    stream.play().ok()?;
    Some(stream)
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: StreamConfig,
    voices: Voices,
) -> Option<Stream> {
    let rate = config.sample_rate as f32;
    let channels = config.channels as usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &OutputCallbackInfo| {
                let Some(mut voices) = voices.try_lock() else {
                    data.fill(T::from_sample(0.0));
                    return;
                };
                for frame in data.chunks_mut(channels) {
                    let mix: f32 = voices.iter_mut().map(|v| v.next(rate)).sum();
                    frame.fill(T::from_sample(mix.tanh()));
                }
                voices.retain(|v| !v.done());
            },
            |err| log::warn!("sfx output: {err}"),
            None,
        )
        .ok()
}
