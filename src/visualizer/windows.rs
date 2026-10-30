//! Listens to Tonal's own audio through WASAPI process loopback (Windows 10 2004 or
//! newer, Windows 11). One stream captures Tonal's whole process tree, which
//! covers every mpv child. Every COM object lives on one MTA capture thread.

use super::FFT_SIZE;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{CloseHandle, E_FAIL, E_NOINTERFACE, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, BLOB, COINIT_MULTITHREADED};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows_core::{implement, Error, IUnknown, Interface, Ref, Result, HRESULT, PCWSTR};

/// Microsoft documents process loopback from build 20348, but it already works on
/// Windows 10 2004 (build 19041), as OBS relies on. Older builds fail cleanly anyway.
const FIRST_BUILD: u32 = 19041;
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

#[derive(Default)]
struct Ring {
    samples: VecDeque<f32>,
    last: Option<Instant>,
}

pub(super) struct Capture {
    active: AtomicBool,
    /// Bumped on every start and stop, so an old capture thread always ends.
    generation: Arc<AtomicU64>,
    ring: Arc<Mutex<Ring>>,
}

impl Capture {
    pub(super) fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            generation: Arc::new(AtomicU64::new(0)),
            ring: Arc::new(Mutex::new(Ring::default())),
        }
    }

    pub(super) fn set_active(&self, on: bool) {
        if self.active.swap(on, Ordering::SeqCst) == on {
            return;
        }
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if !on {
            return;
        }
        let generation = self.generation.clone();
        let ring = self.ring.clone();
        let spawned = std::thread::Builder::new()
            .name("visualizer-capture".into())
            .spawn(move || run(&|| generation.load(Ordering::SeqCst) == mine, &ring));
        if spawned.is_err() {
            self.active.store(false, Ordering::SeqCst);
        }
    }

    pub(super) fn mix_into(&self, out: &mut [f32]) -> bool {
        let ring = self.ring.lock();
        let fresh = ring
            .last
            .is_some_and(|t| t.elapsed() < Duration::from_millis(500));
        if !fresh || ring.samples.len() < out.len() {
            return false;
        }
        let start = ring.samples.len() - out.len();
        for (o, s) in out.iter_mut().zip(ring.samples.range(start..)) {
            *o = *s;
        }
        true
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn available() -> bool {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    let status = unsafe { RtlGetVersion(&mut info) };
    status.is_ok() && (info.dwMajorVersion > 10 || info.dwBuildNumber >= FIRST_BUILD)
}

/// The capture thread: opens the loopback stream, reads it until Tonal stops playing,
/// and reopens it after errors (a device change, for one). Failures only log.
fn run(alive: &dyn Fn() -> bool, ring: &Mutex<Ring>) {
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if com.is_err() {
        log::warn!("visualizer: COM init failed: {}", com.message());
        return;
    }
    let mut warned = false;
    while alive() {
        match Stream::open() {
            Ok(stream) => {
                warned = false;
                if let Err(error) = stream.pump(alive, ring) {
                    log::warn!("audio capture stopped: {error}");
                }
            }
            Err(error) if !warned => {
                log::warn!("audio capture unavailable: {error}");
                warned = true;
            }
            Err(_) => {}
        }
        for _ in 0..10 {
            if !alive() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    *ring.lock() = Ring::default();
    unsafe { CoUninitialize() };
}

#[derive(Clone, Copy)]
enum Sample {
    Float,
    Int16,
}

struct Stream {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
    sample: Sample,
}

impl Stream {
    /// Float32 first; 16-bit PCM if the system refuses it.
    fn open() -> Result<Self> {
        let event = unsafe { CreateEventW(None, false, false, PCWSTR::null())? };
        let mut last = Error::from(E_FAIL);
        for sample in [Sample::Float, Sample::Int16] {
            match Self::init(sample, event) {
                Ok((client, capture)) => {
                    return Ok(Self {
                        client,
                        capture,
                        event,
                        sample,
                    });
                }
                Err(error) => last = error,
            }
        }
        unsafe {
            let _ = CloseHandle(event);
        }
        Err(last)
    }

    fn init(sample: Sample, event: HANDLE) -> Result<(IAudioClient, IAudioCaptureClient)> {
        let client = activate()?;
        let format = wave_format(sample);
        let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
            | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        unsafe {
            // 100 ms of buffer, in 100 ns units.
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 1_000_000, 0, &format, None)?;
            client.SetEventHandle(event)?;
            let capture: IAudioCaptureClient = client.GetService()?;
            Ok((client, capture))
        }
    }

    fn pump(&self, alive: &dyn Fn() -> bool, ring: &Mutex<Ring>) -> Result<()> {
        unsafe { self.client.Start()? };
        let mut result = Ok(());
        while alive() {
            if unsafe { WaitForSingleObject(self.event, 100) } != WAIT_OBJECT_0 {
                continue;
            }
            if let Err(error) = self.drain(ring) {
                result = Err(error);
                break;
            }
        }
        unsafe {
            let _ = self.client.Stop();
        }
        result
    }

    fn drain(&self, ring: &Mutex<Ring>) -> Result<()> {
        loop {
            if unsafe { self.capture.GetNextPacketSize()? } == 0 {
                return Ok(());
            }
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            unsafe {
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?
            };
            let silent = data.is_null() || flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
            {
                let mut ring = ring.lock();
                let source = if silent {
                    Source::Silent
                } else {
                    Source::Interleaved(data, self.sample)
                };
                push(&mut ring.samples, source, frames as usize);
                ring.last = Some(Instant::now());
            }
            unsafe { self.capture.ReleaseBuffer(frames)? };
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

fn wave_format(sample: Sample) -> WAVEFORMATEX {
    let (tag, bits) = match sample {
        Sample::Float => (WAVE_FORMAT_IEEE_FLOAT, 32u16),
        Sample::Int16 => (WAVE_FORMAT_PCM, 16u16),
    };
    let block = CHANNELS * bits / 8;
    WAVEFORMATEX {
        wFormatTag: tag,
        nChannels: CHANNELS,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * block as u32,
        nBlockAlign: block,
        wBitsPerSample: bits,
        cbSize: 0,
    }
}

enum Source {
    Silent,
    Interleaved(*const u8, Sample),
}

/// Mixes interleaved frames down to mono and appends them to the ring.
fn push(ring: &mut VecDeque<f32>, source: Source, frames: usize) {
    let channels = CHANNELS as usize;
    for frame in 0..frames {
        let mut sum = 0.0;
        if let Source::Interleaved(data, sample) = source {
            for c in 0..channels {
                let i = frame * channels + c;
                sum += match sample {
                    Sample::Float => unsafe { data.cast::<f32>().add(i).read_unaligned() },
                    Sample::Int16 => {
                        f32::from(unsafe { data.cast::<i16>().add(i).read_unaligned() }) / 32768.0
                    }
                };
            }
        }
        ring.push_back(sum / channels as f32);
    }
    let excess = ring.len().saturating_sub(FFT_SIZE * 2);
    ring.drain(..excess);
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated(Mutex<Option<mpsc::Sender<()>>>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _: Ref<IActivateAudioInterfaceAsyncOperation>) -> Result<()> {
        if let Some(done) = self.0.lock().take() {
            let _ = done.send(());
        }
        Ok(())
    }
}

/// An audio client on the process loopback device for Tonal and all its children.
fn activate() -> Result<IAudioClient> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: std::process::id(),
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    let mut variant = PROPVARIANT::default();
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob = BLOB {
            cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
            pBlobData: (&raw mut params).cast(),
        };
    }
    let (done, completed) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler =
        Activated(Mutex::new(Some(done))).into();
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&raw const variant),
            &handler,
        )?
    };
    completed
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| Error::new(E_FAIL, "audio activation timed out"))?;
    let mut status = HRESULT(0);
    let mut unknown: Option<IUnknown> = None;
    unsafe { operation.GetActivateResult(&mut status, &mut unknown)? };
    status.ok()?;
    unknown.ok_or_else(|| Error::from(E_NOINTERFACE))?.cast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_starts_and_stops_without_hanging() {
        let capture = Capture::new();
        capture.set_active(true);
        std::thread::sleep(Duration::from_millis(1500));
        let mut out = vec![0.0; FFT_SIZE];
        // Nothing plays, so there is no fresh audio; this must simply not panic.
        let _ = capture.mix_into(&mut out);
        capture.set_active(false);
        capture.set_active(true);
        capture.set_active(false);
        drop(capture);
        std::thread::sleep(Duration::from_millis(500));
    }

    #[test]
    fn stereo_frames_mix_to_mono() {
        let mut ring = VecDeque::new();
        let data = [1.0f32, 0.0, 0.5, 0.5];
        push(
            &mut ring,
            Source::Interleaved(data.as_ptr().cast(), Sample::Float),
            2,
        );
        assert_eq!(ring, [0.5, 0.5]);
        let pcm = [i16::MAX, i16::MAX];
        push(
            &mut ring,
            Source::Interleaved(pcm.as_ptr().cast(), Sample::Int16),
            1,
        );
        assert!((ring[2] - 1.0).abs() < 0.001);
        push(&mut ring, Source::Silent, FFT_SIZE * 3);
        assert_eq!(ring.len(), FFT_SIZE * 2);
    }

    #[test]
    fn version_check_runs() {
        let _ = available();
    }
}
