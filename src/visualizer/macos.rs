//! Listens to Tonal's own mpv processes through a Core Audio process tap (macOS 14.4+).
//! The tap feeds a private aggregate device whose IO block copies mono samples into a
//! ring buffer. Every Core Audio and Objective-C object lives on one capture thread.

use super::FFT_SIZE;
use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, Message};
use objc2_core_audio::{
    AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceStart, AudioDeviceStop, AudioHardwareCreateAggregateDevice,
    AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectID, AudioObjectPropertyAddress, CATapDescription, kAudioAggregateDeviceIsPrivateKey,
    kAudioAggregateDeviceIsStackedKey, kAudioAggregateDeviceMainSubDeviceKey,
    kAudioAggregateDeviceNameKey, kAudioAggregateDeviceSubDeviceListKey,
    kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey,
    kAudioAggregateDeviceUIDKey, kAudioDevicePropertyDeviceUID,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyProcessObjectList,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    kAudioProcessPropertyPID, kAudioSubDeviceUIDKey, kAudioSubTapDriftCompensationKey,
    kAudioSubTapUIDKey,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Ring {
    samples: VecDeque<f32>,
    last: Option<Instant>,
}

pub(super) struct Capture {
    active: Arc<AtomicBool>,
    ring: Arc<Mutex<Ring>>,
}

impl Capture {
    pub(super) fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            ring: Arc::new(Mutex::new(Ring::default())),
        }
    }

    pub(super) fn set_active(&self, on: bool) {
        if self.active.swap(on, Ordering::SeqCst) == on || !on {
            return;
        }
        let active = self.active.clone();
        let ring = self.ring.clone();
        let spawned = std::thread::Builder::new()
            .name("visualizer-capture".into())
            .spawn(move || {
                let mut current: Option<(Vec<AudioObjectID>, String, Tap)> = None;
                while active.load(Ordering::SeqCst) {
                    let processes = own_processes();
                    let output = default_output_uid();
                    let stale = match (&current, &output) {
                        (Some((p, o, _)), Some(out)) => *p != processes || o != out,
                        (Some(_), None) => true,
                        (None, _) => true,
                    };
                    if stale {
                        current = None;
                        if let Some(out) = output
                            && !processes.is_empty()
                        {
                            match Tap::new(&processes, &out, ring.clone()) {
                                Ok(tap) => current = Some((processes, out, tap)),
                                Err(status) => log::warn!("audio tap failed: {status}"),
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                drop(current);
                *ring.lock() = Ring::default();
            });
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
        self.active.store(false, Ordering::SeqCst);
    }
}

pub fn available() -> bool {
    true
}

type IoBlock = RcBlock<
    dyn Fn(
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
    ),
>;

/// One running tap: the tap, the aggregate device reading it and the IO block.
/// Dropping it stops and destroys them in the order Core Audio expects.
struct Tap {
    tap: AudioObjectID,
    device: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    _block: IoBlock,
    _queue: dispatch2::DispatchRetained<DispatchQueue>,
}

impl Tap {
    fn new(
        processes: &[AudioObjectID],
        output_uid: &str,
        ring: Arc<Mutex<Ring>>,
    ) -> Result<Self, i32> {
        let ids: Vec<Retained<NSNumber>> =
            processes.iter().map(|&p| NSNumber::new_u32(p)).collect();
        let ids = NSArray::from_retained_slice(&ids);
        let description = unsafe {
            CATapDescription::initMonoMixdownOfProcesses(CATapDescription::alloc(), &ids)
        };
        unsafe {
            description.setPrivate(true);
            description.setName(&NSString::from_str("Tonal visualizer"));
        }
        let tap_uid = unsafe { description.UUID() }.UUIDString().to_string();
        let mut tap: AudioObjectID = 0;
        check(unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap) })?;

        let sub_device = dictionary(&[(kAudioSubDeviceUIDKey, string(output_uid))]);
        let sub_tap = dictionary(&[
            (kAudioSubTapUIDKey, string(&tap_uid)),
            (
                kAudioSubTapDriftCompensationKey,
                any(NSNumber::new_bool(true)),
            ),
        ]);
        let aggregate = dictionary(&[
            (kAudioAggregateDeviceNameKey, string("Tonal visualizer")),
            (
                kAudioAggregateDeviceUIDKey,
                string(&format!("tonal.visualizer.{}", tap_uid)),
            ),
            (kAudioAggregateDeviceMainSubDeviceKey, string(output_uid)),
            (
                kAudioAggregateDeviceIsPrivateKey,
                any(NSNumber::new_bool(true)),
            ),
            (
                kAudioAggregateDeviceIsStackedKey,
                any(NSNumber::new_bool(false)),
            ),
            (
                kAudioAggregateDeviceTapAutoStartKey,
                any(NSNumber::new_bool(true)),
            ),
            (
                kAudioAggregateDeviceSubDeviceListKey,
                any(NSArray::from_retained_slice(&[sub_device])),
            ),
            (
                kAudioAggregateDeviceTapListKey,
                any(NSArray::from_retained_slice(&[sub_tap])),
            ),
        ]);
        let mut device: AudioObjectID = 0;
        let cf = unsafe { &*(Retained::as_ptr(&aggregate).cast::<CFDictionary>()) };
        if let Err(status) =
            check(unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut device)) })
        {
            unsafe { AudioHardwareDestroyProcessTap(tap) };
            return Err(status);
        }

        let block: IoBlock = RcBlock::new(
            move |_: NonNull<AudioTimeStamp>,
                  input: NonNull<AudioBufferList>,
                  _: NonNull<AudioTimeStamp>,
                  _: NonNull<AudioBufferList>,
                  _: NonNull<AudioTimeStamp>| {
                let mut ring = ring.lock();
                push(&mut ring.samples, unsafe { input.as_ref() });
                ring.last = Some(Instant::now());
            },
        );
        let queue = DispatchQueue::new("tonal.visualizer", None);
        let mut proc_id: AudioDeviceIOProcID = None;
        let created = check(unsafe {
            AudioDeviceCreateIOProcIDWithBlock(
                NonNull::from(&mut proc_id),
                device,
                Some(&*queue),
                RcBlock::as_ptr(&block),
            )
        })
        .and_then(|()| check(unsafe { AudioDeviceStart(device, proc_id) }));
        let tap = Self {
            tap,
            device,
            proc_id,
            _block: block,
            _queue: queue,
        };
        created.map(|()| tap)
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        unsafe {
            if self.proc_id.is_some() {
                AudioDeviceStop(self.device, self.proc_id);
                AudioDeviceDestroyIOProcID(self.device, self.proc_id);
            }
            AudioHardwareDestroyAggregateDevice(self.device);
            AudioHardwareDestroyProcessTap(self.tap);
        }
    }
}

/// Mixes every buffer and channel down to mono and appends it to the ring.
fn push(ring: &mut VecDeque<f32>, list: &AudioBufferList) {
    let count = list.mNumberBuffers as usize;
    let buffers: &[AudioBuffer] =
        unsafe { std::slice::from_raw_parts(list.mBuffers.as_ptr(), count) };
    let Some(first) = buffers.first() else { return };
    let channels = first.mNumberChannels.max(1) as usize;
    let frames = first.mDataByteSize as usize / 4 / channels;
    let total = (count * channels) as f32;
    for frame in 0..frames {
        let mut sum = 0.0;
        for buffer in buffers {
            if buffer.mData.is_null() {
                continue;
            }
            let data = buffer.mData.cast::<f32>();
            let width = buffer.mNumberChannels.max(1) as usize;
            let len = buffer.mDataByteSize as usize / 4;
            for c in 0..width {
                let i = frame * width + c;
                if i < len {
                    sum += unsafe { *data.add(i) };
                }
            }
        }
        ring.push_back(sum / total);
    }
    let excess = ring.len().saturating_sub(FFT_SIZE * 2);
    ring.drain(..excess);
}

fn check(status: i32) -> Result<(), i32> {
    if status == 0 { Ok(()) } else { Err(status) }
}

fn any<T: Message>(object: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an AnyObject.
    unsafe { Retained::cast_unchecked(object) }
}

fn string(value: &str) -> Retained<AnyObject> {
    any(NSString::from_str(value))
}

fn dictionary(
    entries: &[(&CStr, Retained<AnyObject>)],
) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = entries
        .iter()
        .map(|(k, _)| NSString::from_str(k.to_str().unwrap_or_default()))
        .collect();
    let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let values: Vec<&AnyObject> = entries.iter().map(|(_, v)| &**v).collect();
    NSDictionary::from_slices(&keys, &values)
}

fn address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn get<T: Copy>(object: AudioObjectID, selector: u32) -> Option<T> {
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    let mut size = std::mem::size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address(selector)),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new_unchecked(value.as_mut_ptr().cast::<c_void>()),
        )
    };
    (status == 0).then(|| unsafe { value.assume_init() })
}

/// Core Audio process objects of mpv instances Tonal started.
fn own_processes() -> Vec<AudioObjectID> {
    let system = kAudioObjectSystemObject as AudioObjectID;
    let list = address(kAudioHardwarePropertyProcessObjectList);
    let mut size = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            system,
            NonNull::from(&list),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 || size == 0 {
        return Vec::new();
    }
    let mut objects =
        vec![0 as AudioObjectID; size as usize / std::mem::size_of::<AudioObjectID>()];
    let status = unsafe {
        AudioObjectGetPropertyData(
            system,
            NonNull::from(&list),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new_unchecked(objects.as_mut_ptr().cast::<c_void>()),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    objects.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
    let me = std::process::id();
    let mut own: Vec<AudioObjectID> = objects
        .into_iter()
        .filter(|&object| {
            get::<libc::pid_t>(object, kAudioProcessPropertyPID)
                .is_some_and(|pid| parent_of(pid) == Some(me))
        })
        .collect();
    own.sort_unstable();
    own
}

fn parent_of(pid: libc::pid_t) -> Option<u32> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    (read == size).then(|| unsafe { info.assume_init() }.pbi_ppid)
}

fn default_output_uid() -> Option<String> {
    let system = kAudioObjectSystemObject as AudioObjectID;
    let device: AudioObjectID = get(system, kAudioHardwarePropertyDefaultOutputDevice)?;
    let uid: *mut NSString = get(device, kAudioDevicePropertyDeviceUID)?;
    let uid = unsafe { Retained::from_raw(uid) }?;
    Some(uid.to_string())
}
