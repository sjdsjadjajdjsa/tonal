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
/// Seconds a bubble rings.
const LENGTH: f32 = 0.16;

enum Msg {
    Warm,
    Bubble { freq: f32, level: f32 },
}

struct Voice {
    freq: f32,
    level: f32,
    t: f32,
    phase: f32,
}

impl Voice {
    /// A bubble: a sine that slides up as it fades, with a soft overtone.
    fn next(&mut self, rate: f32) -> f32 {
        let t = self.t;
        if t >= LENGTH {
            return 0.0;
        }
        let freq = self.freq * (1.0 + 0.9 * (1.0 - (-t / 0.03).exp()));
        self.phase = (self.phase + freq / rate).fract();
        self.t += 1.0 / rate;
        let attack = (t / 0.002).min(1.0);
        let decay = (-t / 0.04).exp();
        let tone = (self.phase * TAU).sin() + 0.18 * (2.0 * self.phase * TAU).sin();
        tone * attack * decay * self.level
    }
}

type Voices = Arc<Mutex<Vec<Voice>>>;

static OUT: OnceLock<Sender<Msg>> = OnceLock::new();

fn send(msg: Msg) {
    let tx = OUT.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("sfx".into())
            .spawn(move || run(rx));
        tx
    });
    let _ = tx.send(msg);
}

/// Opens the output ahead of a sound so the first one isn't late.
pub fn warm() {
    send(Msg::Warm);
}

/// Plays one bubble at `freq` Hz; `level` is 0 to 1.
pub fn bubble(freq: f32, level: f32) {
    if level > 0.0 {
        send(Msg::Bubble { freq, level });
    }
}

fn run(rx: Receiver<Msg>) {
    let voices: Voices = Arc::default();
    let add = |msg: Msg| {
        if let Msg::Bubble { freq, level } = msg {
            voices.lock().push(Voice {
                freq,
                level,
                t: 0.0,
                phase: 0.0,
            });
        }
    };
    while let Ok(msg) = rx.recv() {
        add(msg);
        let stream = open(voices.clone());
        loop {
            match rx.recv_timeout(IDLE) {
                Ok(msg) => add(msg),
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
                voices.retain(|v| v.t < LENGTH);
            },
            |err| log::warn!("sfx output: {err}"),
            None,
        )
        .ok()
}
