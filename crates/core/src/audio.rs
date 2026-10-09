//! Selected-endpoint capture and a bounded, paced 48 kHz stereo f32-LE source.
//!
//! Wrap `CaptureSession::input` in `songbird::input::RawAdapter::new(input, 48_000, 2)`.
//! Reads never report temporary EOF or WouldBlock. Silence uses the same real-time
//! budget as captured audio, including when a buffering reader repeatedly refills.
//! A slow consumer cannot accumulate unbounded pacing credit. This bounds source read-ahead;
//! already returned bytes in the decoder/mixer remain outside this queue's control.

use crate::model::{AppError, Capture, CapturePhase, RecordingSource};
use rtrb::Consumer;
#[cfg(any(windows, test))]
use rtrb::{Producer, RingBuffer};
#[cfg(windows)]
use std::time::{Duration, Instant};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
};
use symphonia_core::io::MediaSource;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;
const FRAME_BYTES: usize = 8;
const QUANTUM_FRAMES: usize = 240; // 5 ms; also the maximum per-read allocation-free output.
const QUANTUM_NS: u64 = 5_000_000;
const MAX_CATCH_UP_NS: u64 = QUANTUM_NS; // At most two newly granted quanta without waiting.
const QUEUE_FRAMES: usize = 1_920; // 40 ms of complete stereo frames.
const MAX_AGE_NS: u64 = 40_000_000;
const WAIT_SLICE_NS: u64 = 2_000_000;
const METER_AGE_NS: u64 = 200_000_000;
const MUTED: u64 = 1 << 8;
const GENERATION_SHIFT: u32 = 9;
const STOPPED: u8 = 0;
const OPENING: u8 = 1;
const RUNNING: u8 = 2;
const MISSING: u8 = 3;
#[cfg(any(windows, test))]
const FAILED: u8 = 4;
const DENIED: u8 = 5;
const ROUTE_CHANGED: u8 = 6;

/// PCM and its independently cloneable control handle. Capture opens only in
/// `start_capture`; enumeration never opens a stream.
pub struct CaptureSession {
    pub input: LivePcm,
    pub control: CaptureControl,
}

/// Controls and snapshot access never touch the callback's producer or samples.
#[derive(Clone)]
pub struct CaptureControl {
    owner: Arc<Owner>,
}

impl CaptureControl {
    /// Idempotently cancel reads and reject future callback frames, then release
    /// the native stream. CPAL stream destruction can join its native thread;
    /// the parent engine-process exit deadline remains the shutdown guarantee.
    pub fn stop(&self) {
        self.owner.shared.status.store(STOPPED, Ordering::Release);
        #[cfg(windows)]
        {
            // Never hold this mutex on an audio callback. Drop outside the lock.
            let stream = self
                .owner
                .stream
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            drop(stream);
        }
    }

    /// Volume is clamped to 0..=100 as a defensive boundary; the command layer
    /// validates user input. Gain is applied when a frame is handed to the reader.
    /// Every mute transition invalidates older queued frames, even if unmuted
    /// before the reader runs. At most one already-started frame is staged here.
    pub fn set_output(&self, volume_percent: u8, muted: bool) {
        let shared = &self.owner.shared;
        let mut old = shared.output.load(Ordering::Acquire);
        loop {
            let generation =
                (old >> GENERATION_SHIFT).wrapping_add(u64::from((old & MUTED != 0) != muted));
            let new = (generation << GENERATION_SHIFT)
                | u64::from(volume_percent.min(100))
                | if muted { MUTED } else { 0 };
            match shared
                .output
                .compare_exchange_weak(old, new, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(actual) => old = actual,
            }
        }
    }

    /// Number of nonterminal native underrun/overrun/discontinuity notifications.
    /// This is an event count; the backend has not supplied a lost-frame count.
    /// Kept separate from the shared UI/TypeScript snapshot and dropped frames.
    pub fn xrun_events(&self) -> u64 {
        self.owner.shared.xruns.load(Ordering::Relaxed)
    }

    /// Pre-volume peak input level, independent of mute. Counts are stereo frames
    /// and saturate at u32::MAX. Occupancy is an approximate concurrent snapshot.
    pub fn snapshot(&self) -> Capture {
        let shared = &self.owner.shared;
        let status = shared.status.load(Ordering::Acquire);
        let (phase, message) = match status {
            STOPPED => (CapturePhase::Stopped, None),
            OPENING => (CapturePhase::Opening, None),
            RUNNING => (CapturePhase::Running, None),
            MISSING => (
                CapturePhase::Missing,
                Some("The selected recording device is no longer available."),
            ),
            DENIED => (
                CapturePhase::Error,
                Some("Access to the selected recording device was denied."),
            ),
            ROUTE_CHANGED => (
                CapturePhase::Error,
                Some("The capture route changed. Select the recording device again."),
            ),
            _ => (
                CapturePhase::Error,
                Some("Capture failed. Reconnect the selected recording device."),
            ),
        };
        let peak_bits = shared.peak_bits.load(Ordering::Acquire);
        let level_dbfs = if status == RUNNING && peak_bits != u32::MAX {
            let peak = if shared
                .clock
                .now()
                .saturating_sub(shared.last_callback_ns.load(Ordering::Acquire))
                <= METER_AGE_NS
            {
                f32::from_bits(peak_bits)
            } else {
                0.0
            };
            Some(if peak > 0.0 {
                (20.0 * peak.log10()).max(-96.0)
            } else {
                -96.0
            })
        } else {
            None
        };
        let popped = shared.popped.load(Ordering::Acquire);
        let pushed = shared.pushed.load(Ordering::Acquire);
        Capture {
            phase,
            level_dbfs,
            output_level_dbfs: None,
            message: message.map(str::to_owned),
            dropped_frames: count(shared.dropped.load(Ordering::Relaxed)),
            underrun_frames: count(shared.underrun.load(Ordering::Relaxed)),
            queued_frames: if status == RUNNING || status == OPENING {
                pushed.saturating_sub(popped).min(QUEUE_FRAMES as u64) as u32
            } else {
                0
            },
        }
    }
}

fn count(value: u64) -> u32 {
    value.min(u64::from(u32::MAX)) as u32
}

struct Owner {
    shared: Arc<Shared>,
    #[cfg(windows)]
    stream: Mutex<Option<cpal::Stream>>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.shared.status.store(STOPPED, Ordering::Release);
    }
}

struct Shared {
    status: AtomicU8,
    output: AtomicU64,
    peak_bits: AtomicU32,
    last_callback_ns: AtomicU64,
    pushed: AtomicU64,
    popped: AtomicU64,
    dropped: AtomicU64,
    underrun: AtomicU64,
    xruns: AtomicU64,
    clock: Arc<dyn Clock>,
}

impl Shared {
    #[cfg(any(windows, test))]
    fn fail(&self, failure: u8) {
        self.fail_observed(failure, |_| {});
    }

    #[cfg(any(windows, test))]
    fn fail_observed(&self, failure: u8, mut observed: impl FnMut(u8)) {
        // Retry against the actual active state if Opening promotes to Running.
        // Stop and the first failure are terminal and must never be overwritten.
        // The no-op observer in production optimizes away; tests use it to
        // inject a promotion/stop/failure between observation and the CAS.
        let _ = self
            .status
            .try_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                if matches!(active, RUNNING | OPENING) {
                    observed(active);
                    Some(failure)
                } else {
                    None
                }
            });
    }

    #[cfg(any(windows, test))]
    fn record_xrun(&self) {
        if matches!(self.status.load(Ordering::Acquire), RUNNING | OPENING) {
            self.xruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

trait Clock: Send + Sync {
    fn now(&self) -> u64;
    fn wait(&self, ns: u64);
}

#[cfg(windows)]
struct SystemClock(Instant);

#[cfg(windows)]
impl Clock for SystemClock {
    fn now(&self) -> u64 {
        self.0.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }

    fn wait(&self, ns: u64) {
        std::thread::sleep(Duration::from_nanos(ns));
    }
}

#[derive(Clone, Copy)]
struct Frame {
    samples: [f32; 2],
    exposed_ns: u64,
    generation: u64,
}

/// Nonseekable, unknown-length stereo f32-LE PCM. The mutex makes the rtrb
/// consumer Sync without unsafe implementations. `Read` has exclusive access
/// and uses `get_mut`, so neither side takes a sample-transfer lock.
pub struct LivePcm {
    reader: Mutex<Reader>,
    owner: Arc<Owner>,
}

struct Reader {
    consumer: Consumer<Frame>,
    pending: [u8; FRAME_BYTES],
    pending_pos: usize,
    frames_left: usize,
    next_quantum_ns: Option<u64>,
    position: u64,
}

impl Reader {
    fn read(&mut self, shared: &Shared, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let status = shared.status.load(Ordering::Acquire);
        if status != RUNNING && status != OPENING {
            self.pending_pos = FRAME_BYTES;
            self.discard_queue(shared);
            return if status == STOPPED {
                Ok(0)
            } else {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "capture failed"))
            };
        }
        // Never fill across a new pacing quantum in the same call. Fragmented
        // reads spend credit only when starting a complete frame, not per byte.
        if self.pending_pos == FRAME_BYTES && self.frames_left == 0 {
            loop {
                if !matches!(shared.status.load(Ordering::Acquire), RUNNING | OPENING) {
                    return self.read(shared, buf);
                }
                let now = shared.clock.now();
                let deadline = self.next_quantum_ns.unwrap_or(now);
                if now >= deadline {
                    // Anchor normal cadence to the previous deadline so small
                    // wakeup oversleeps do not reduce the effective sample rate.
                    // Reset after a larger stall instead of banking old credit.
                    let anchor = if now - deadline <= MAX_CATCH_UP_NS {
                        deadline
                    } else {
                        now
                    };
                    self.next_quantum_ns = Some(anchor.saturating_add(QUANTUM_NS));
                    self.frames_left = QUANTUM_FRAMES;
                    break;
                }
                shared.clock.wait((deadline - now).min(WAIT_SLICE_NS));
            }
        }
        let limit = buf.len().min(QUANTUM_FRAMES * FRAME_BYTES);
        let mut written = 0;
        // Even if the producer keeps filling during discard, a read examines
        // at most one queue capacity. No drain-until-empty callback race.
        let mut pop_budget = QUEUE_FRAMES;
        while written < limit {
            if !matches!(shared.status.load(Ordering::Acquire), RUNNING | OPENING) {
                if written == 0 {
                    // Failure can race the clock check above. Re-enter the
                    // terminal path so only stop, never failure, returns EOF.
                    return self.read(shared, buf);
                }
                break;
            }
            if self.pending_pos == FRAME_BYTES {
                if self.frames_left == 0 {
                    break;
                }
                let samples = self.next_frame(shared, &mut pop_budget);
                self.pending[..4].copy_from_slice(&samples[0].to_le_bytes());
                self.pending[4..].copy_from_slice(&samples[1].to_le_bytes());
                self.pending_pos = 0;
                self.frames_left -= 1;
            }
            let n = (FRAME_BYTES - self.pending_pos).min(limit - written);
            buf[written..written + n]
                .copy_from_slice(&self.pending[self.pending_pos..self.pending_pos + n]);
            self.pending_pos += n;
            written += n;
        }
        self.position = self.position.saturating_add(written as u64);
        Ok(written)
    }

    fn next_frame(&mut self, shared: &Shared, budget: &mut usize) -> [f32; 2] {
        while *budget > 0 {
            let Ok(frame) = self.consumer.pop() else {
                break;
            };
            *budget -= 1;
            shared.popped.fetch_add(1, Ordering::Release);
            let output = shared.output.load(Ordering::Acquire);
            if shared.clock.now().saturating_sub(frame.exposed_ns) > MAX_AGE_NS
                || frame.generation != output >> GENERATION_SHIFT
                || output & MUTED != 0
            {
                shared.dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let gain = (output & 0xff) as f32 / 100.0;
            return [frame.samples[0] * gain, frame.samples[1] * gain];
        }
        // Intentional mute is not an underrun. Muted reads still
        // drain old queued frames instead of leaving audio to replay later.
        if shared.output.load(Ordering::Acquire) & MUTED == 0 {
            shared.underrun.fetch_add(1, Ordering::Relaxed);
        }
        [0.0; 2]
    }

    fn discard_queue(&mut self, shared: &Shared) {
        for _ in 0..QUEUE_FRAMES {
            if self.consumer.pop().is_err() {
                break;
            }
            shared.popped.fetch_add(1, Ordering::Release);
            shared.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Read for LivePcm {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader
            .get_mut()
            .map_err(|_| io::Error::other("PCM reader state is unavailable"))?
            .read(&self.owner.shared, buf)
    }
}

impl Seek for LivePcm {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match pos {
            SeekFrom::Current(0) => Ok(self
                .reader
                .get_mut()
                .map_err(|_| io::Error::other("PCM reader state is unavailable"))?
                .position),
            _ => Err(io::ErrorKind::Unsupported.into()),
        }
    }
}

impl MediaSource for LivePcm {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

#[cfg(any(windows, test))]
struct CaptureWriter {
    producer: Producer<Frame>,
    shared: Arc<Shared>,
    // A backend normally supplies whole input frames, but retain an unmatched
    // left sample across callbacks rather than swap channels on fragmentation.
    partial_left: Option<(f32, u64, u64)>,
}

#[cfg(any(windows, test))]
impl CaptureWriter {
    fn write<T: Copy>(&mut self, data: &[T], channels: u16, convert: impl Fn(T) -> f32) {
        if !matches!(
            self.shared.status.load(Ordering::Acquire),
            RUNNING | OPENING
        ) {
            return;
        }
        let now = self.shared.clock.now();
        let mut peak = 0.0_f32;
        for &raw in data {
            if !matches!(
                self.shared.status.load(Ordering::Acquire),
                RUNNING | OPENING
            ) {
                return;
            }
            let sample = convert(raw);
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            peak = peak.max(sample.abs());
            let output = self.shared.output.load(Ordering::Acquire);
            let generation = output >> GENERATION_SHIFT;
            let (samples, exposed_ns, frame_generation) = if channels == 1 {
                ([sample; 2], now, generation)
            } else if let Some((left, exposed_ns, generation)) = self.partial_left.take() {
                ([left, sample], exposed_ns, generation)
            } else {
                self.partial_left = Some((sample, now, generation));
                continue;
            };
            if output & MUTED != 0 || frame_generation != generation {
                continue;
            }
            if self
                .producer
                .push(Frame {
                    samples,
                    exposed_ns,
                    generation: frame_generation,
                })
                .is_ok()
            {
                self.shared.pushed.fetch_add(1, Ordering::Release);
            } else {
                // Only the consumer may discard queued data in an SPSC ring.
                // Overflow rejects the incoming whole frame, preserving stereo.
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        if !data.is_empty() {
            self.shared.last_callback_ns.store(now, Ordering::Release);
            self.shared
                .peak_bits
                .store(peak.to_bits(), Ordering::Release);
        }
    }
}

#[cfg(any(windows, test))]
fn pipeline(clock: Arc<dyn Clock>) -> (CaptureWriter, CaptureSession) {
    let (producer, consumer) = RingBuffer::new(QUEUE_FRAMES);
    let shared = Arc::new(Shared {
        status: AtomicU8::new(OPENING),
        output: AtomicU64::new(100),
        peak_bits: AtomicU32::new(u32::MAX),
        last_callback_ns: AtomicU64::new(0),
        pushed: AtomicU64::new(0),
        popped: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
        underrun: AtomicU64::new(0),
        xruns: AtomicU64::new(0),
        clock,
    });
    let owner = Arc::new(Owner {
        shared: shared.clone(),
        #[cfg(windows)]
        stream: Mutex::new(None),
    });
    (
        CaptureWriter {
            producer,
            shared,
            partial_left: None,
        },
        CaptureSession {
            input: LivePcm {
                reader: Mutex::new(Reader {
                    consumer,
                    pending: [0; FRAME_BYTES],
                    pending_pos: FRAME_BYTES,
                    frames_left: 0,
                    next_quantum_ns: None,
                    position: 0,
                }),
                owner: owner.clone(),
            },
            control: CaptureControl { owner },
        },
    )
}

#[cfg(windows)]
pub use windows::{enumerate_sources, start_capture};

#[cfg(not(windows))]
pub fn enumerate_sources() -> Result<Vec<RecordingSource>, AppError> {
    Err(unsupported_platform())
}

#[cfg(not(windows))]
pub fn start_capture(_source_id: &str) -> Result<CaptureSession, AppError> {
    Err(unsupported_platform())
}

#[cfg(not(windows))]
fn unsupported_platform() -> AppError {
    AppError {
        code: crate::model::ErrorCode::CaptureFailed,
        message: "Recording-device capture is supported on Windows only.".into(),
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{DeviceId, HostId, SampleFormat, SizedSample, SupportedStreamConfig};

    fn error(message: &str) -> AppError {
        AppError {
            code: crate::model::ErrorCode::CaptureFailed,
            message: message.into(),
        }
    }

    fn host() -> Result<cpal::Host, AppError> {
        cpal::host_from_id(HostId::Wasapi)
            .map_err(|_| error("The Windows audio host is unavailable."))
    }

    fn supported_config(device: &cpal::Device) -> Result<Option<SupportedStreamConfig>, AppError> {
        let configs = device.supported_input_configs().map_err(|_| {
            error("Cannot inspect the selected recording device's supported formats.")
        })?;
        // Explicitly supported rate only: no default configuration or resampling.
        // Prefer stereo, then f32. Conversion happens directly on callback slices.
        Ok(select_config(configs))
    }

    pub(super) fn select_config(
        configs: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
    ) -> Option<SupportedStreamConfig> {
        configs
            .filter_map(|range| {
                let rank = match range.sample_format() {
                    SampleFormat::F32 => 0,
                    SampleFormat::I16 => 1,
                    SampleFormat::I32 => 2,
                    SampleFormat::U16 => 3,
                    SampleFormat::F64 => 4,
                    SampleFormat::I8 => 5,
                    SampleFormat::U8 => 6,
                    SampleFormat::I64 => 7,
                    SampleFormat::U32 => 8,
                    SampleFormat::U64 => 9,
                    _ => return None,
                };
                let channel_rank = match range.channels() {
                    2 => 0,
                    1 => 1,
                    _ => return None,
                };
                range
                    .try_with_sample_rate(SAMPLE_RATE)
                    .map(|config| ((channel_rank, rank), config))
            })
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, config)| config)
    }

    pub fn enumerate_sources() -> Result<Vec<RecordingSource>, AppError> {
        let host = host()?;
        let devices = host
            .input_devices()
            .map_err(|_| error("Cannot enumerate Windows recording devices."))?;
        let mut sources = Vec::new();
        for device in devices {
            let id = device
                .id()
                .map_err(|_| error("Cannot identify a recording device."))?
                .to_string();
            let name = device
                .description()
                .map_err(|_| error("Cannot read a recording device's description."))?
                .name()
                .to_owned();
            let (available, detail) = match supported_config(&device) {
                Ok(Some(_)) => (true, Some("48 kHz mono/stereo recording".into())),
                Ok(None) => (
                    false,
                    Some("Requires a supported 48 kHz mono/stereo PCM format.".into()),
                ),
                Err(_) => (
                    false,
                    Some("Recording device formats are unavailable.".into()),
                ),
            };
            sources.push(RecordingSource {
                id,
                name,
                available,
                detail,
            });
        }
        Ok(sources)
    }

    pub fn start_capture(source_id: &str) -> Result<CaptureSession, AppError> {
        let id: DeviceId = source_id
            .parse()
            .map_err(|_| error("The selected recording-device ID is invalid."))?;
        if id.host() != HostId::Wasapi {
            return Err(error(
                "The selected recording device must use Windows WASAPI.",
            ));
        }
        let host = host()?;
        // Restrict lookup to recording endpoints. WASAPI can build loopback on
        // playback devices; such IDs must never be accepted as capture inputs.
        let device = host
            .input_devices()
            .map_err(|_| error("Cannot enumerate Windows recording devices."))?
            .find(|device| device.id().ok().as_ref() == Some(&id))
            .ok_or_else(|| error("The selected recording device is unavailable."))?;
        let config = supported_config(&device)?.ok_or_else(|| error(
            "The selected recording device does not support 48 kHz mono/stereo PCM. Configure it for 48 kHz and retry.",
        ))?;
        let (writer, session) = pipeline(Arc::new(SystemClock(Instant::now())));
        let stream = match config.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, config, writer),
            SampleFormat::I16 => build::<i16>(&device, config, writer),
            SampleFormat::I32 => build::<i32>(&device, config, writer),
            SampleFormat::U16 => build::<u16>(&device, config, writer),
            SampleFormat::F64 => build::<f64>(&device, config, writer),
            SampleFormat::I8 => build::<i8>(&device, config, writer),
            SampleFormat::U8 => build::<u8>(&device, config, writer),
            SampleFormat::I64 => build::<i64>(&device, config, writer),
            SampleFormat::U32 => build::<u32>(&device, config, writer),
            SampleFormat::U64 => build::<u64>(&device, config, writer),
            _ => return Err(error("The selected recording format is unsupported.")),
        }
        .map_err(|_| error("Cannot open the selected recording device at 48 kHz."))?;
        stream
            .play()
            .map_err(|_| error("Cannot start the selected recording device."))?;
        *session
            .control
            .owner
            .stream
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(stream);
        let shared = &session.control.owner.shared;
        shared
            .status
            .compare_exchange(OPENING, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .ok();
        if shared.status.load(Ordering::Acquire) != RUNNING {
            session.control.stop();
            return Err(error(
                "The selected recording device failed while starting capture.",
            ));
        }
        Ok(session)
    }

    fn build<T>(
        device: &cpal::Device,
        config: SupportedStreamConfig,
        mut writer: CaptureWriter,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample + Copy,
        f32: cpal::FromSample<T>,
    {
        let shared = writer.shared.clone();
        let channels = config.channels();
        device.build_input_stream::<T, _, _>(
            config.config(),
            move |data, _| writer.write(data, channels, |sample| sample.to_sample::<f32>()),
            move |failure| report_error(&shared, failure.kind()),
            None,
        )
    }

    pub(super) fn report_error(shared: &Shared, kind: cpal::ErrorKind) {
        // No logging, string formatting, locks, allocation, UI, or I/O.
        // WASAPI reports discontinuity and continues handing off capture data.
        if kind == cpal::ErrorKind::Xrun {
            shared.record_xrun();
            return;
        }
        let status = match kind {
            cpal::ErrorKind::DeviceNotAvailable => MISSING,
            cpal::ErrorKind::PermissionDenied => DENIED,
            cpal::ErrorKind::DeviceChanged => ROUTE_CHANGED,
            _ => FAILED,
        };
        shared.fail(status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Weak;
    use symphonia_core::io::{MediaSourceStream, ReadBytes, SeekBuffered};

    #[derive(Default)]
    struct ManualClock {
        ns: AtomicU64,
        waits: AtomicU64,
        oversleep_ns: AtomicU64,
        // A deterministic cancellation injected exactly when a reader waits.
        cancel_on_wait: Mutex<Option<Weak<Shared>>>,
        fail_on_now: Mutex<Option<Weak<Shared>>>,
    }

    impl Clock for ManualClock {
        fn now(&self) -> u64 {
            if let Some(shared) = self
                .fail_on_now
                .lock()
                .unwrap()
                .take()
                .and_then(|w| w.upgrade())
            {
                shared.fail(MISSING);
            }
            self.ns.load(Ordering::Relaxed)
        }

        fn wait(&self, ns: u64) {
            assert!(ns <= WAIT_SLICE_NS);
            self.waits.fetch_add(1, Ordering::Relaxed);
            self.ns.fetch_add(
                ns + self.oversleep_ns.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            if let Some(shared) = self
                .cancel_on_wait
                .lock()
                .unwrap()
                .take()
                .and_then(|w| w.upgrade())
            {
                shared.status.store(STOPPED, Ordering::Release);
            }
        }
    }

    fn setup() -> (CaptureWriter, CaptureSession, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::default());
        let (writer, session) = pipeline(clock.clone());
        session
            .control
            .owner
            .shared
            .status
            .store(RUNNING, Ordering::Release);
        (writer, session, clock)
    }

    fn feed(writer: &mut CaptureWriter, frames: usize, left: f32, right: f32) {
        let data: Vec<_> = (0..frames).flat_map(|_| [left, right]).collect();
        writer.write(&data, 2, |x| x);
    }

    fn frames(input: &mut impl Read, n: usize) -> Vec<[f32; 2]> {
        let mut data = vec![0; n * FRAME_BYTES];
        input.read_exact(&mut data).unwrap();
        data.as_chunks::<FRAME_BYTES>()
            .0
            .iter()
            .map(|frame| {
                [
                    f32::from_le_bytes(frame[..4].try_into().unwrap()),
                    f32::from_le_bytes(frame[4..].try_into().unwrap()),
                ]
            })
            .collect()
    }

    #[test]
    fn live_source_and_controls_are_send_sync() {
        fn assert_traits<T: Send + Sync>() {}
        assert_traits::<LivePcm>();
        assert_traits::<CaptureControl>();
        assert_traits::<CaptureSession>();
    }

    #[test]
    fn short_reads_preserve_every_byte_and_stereo_channel() {
        let (mut writer, mut session, _) = setup();
        let samples: Vec<f32> = (0..1_000)
            .flat_map(|n| [n as f32 / 1_000.0, -(n as f32 / 1_000.0)])
            .collect();
        writer.write(&samples, 2, |x| x);
        let expected: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut actual = Vec::new();
        let sizes = [1, 3, 7, 9, 13, 1_023, 32_768];
        let mut iteration = 0;
        while actual.len() < expected.len() {
            let mut out =
                vec![0xa5; sizes[iteration % sizes.len()].min(expected.len() - actual.len())];
            let n = session.input.read(&mut out).unwrap();
            assert!(n > 0 && n <= QUANTUM_FRAMES * FRAME_BYTES);
            actual.extend_from_slice(&out[..n]);
            iteration += 1;
        }
        assert_eq!(actual, expected);
        assert_eq!(
            session.input.stream_position().unwrap(),
            expected.len() as u64
        );
        assert_eq!(session.control.snapshot().queued_frames, 0);
    }

    #[test]
    fn empty_read_has_no_side_effect_and_source_is_unseekable() {
        let (_, mut session, clock) = setup();
        assert_eq!(session.input.read(&mut []).unwrap(), 0);
        assert_eq!(clock.now(), 0);
        assert!(!session.input.is_seekable());
        assert_eq!(session.input.byte_len(), None);
        assert_eq!(
            session.input.seek(SeekFrom::Start(0)).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            session.input.seek(SeekFrom::End(0)).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn starvation_synthesizes_paced_silence_and_resumes() {
        let (mut writer, mut session, clock) = setup();
        assert_eq!(frames(&mut session.input, 960), vec![[0.0; 2]; 960]);
        assert_eq!(clock.now(), 15_000_000);
        assert_eq!(session.control.snapshot().underrun_frames, 960);
        assert_eq!(session.control.snapshot().phase, CapturePhase::Running);
        feed(&mut writer, 960, 0.25, -0.5);
        assert_eq!(frames(&mut session.input, 960), vec![[0.25, -0.5]; 960]);
        assert_eq!(clock.now(), 35_000_000);
        assert_eq!(session.control.snapshot().underrun_frames, 960);
    }

    #[test]
    fn repeated_reads_cannot_prefetch_unbounded_silence() {
        let (_, mut session, clock) = setup();
        let mut oversized = [0xff; 32_768];
        for n in 0..200 {
            assert_eq!(
                session.input.read(&mut oversized).unwrap(),
                QUANTUM_FRAMES * FRAME_BYTES
            );
            assert_eq!(clock.now(), n * QUANTUM_NS);
        }
        assert!(clock.waits.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn slow_consumer_does_not_bank_pacing_credit() {
        let (_, mut session, clock) = setup();
        frames(&mut session.input, QUANTUM_FRAMES);
        clock.ns.store(10_000_000_000, Ordering::Relaxed);
        frames(&mut session.input, QUANTUM_FRAMES);
        assert_eq!(clock.now(), 10_000_000_000);
        frames(&mut session.input, QUANTUM_FRAMES);
        assert_eq!(clock.now(), 10_005_000_000);
    }

    #[test]
    fn wakeup_oversleep_does_not_accumulate_deadline_drift() {
        let (_, mut session, clock) = setup();
        let oversleep = 200_000;
        clock.oversleep_ns.store(oversleep, Ordering::Relaxed);
        for quantum in 0..2_000 {
            frames(&mut session.input, QUANTUM_FRAMES);
            let deadline = quantum * QUANTUM_NS;
            assert!(clock.now() >= deadline);
            assert!(clock.now() <= deadline + oversleep);
        }
    }

    #[test]
    fn symphonia_packet_cadence_stays_anchored_under_wakeup_oversleep() {
        let (_, session, clock) = setup();
        clock.oversleep_ns.store(200_000, Ordering::Relaxed);
        let mut source = MediaSourceStream::new(Box::new(session.input), Default::default());
        for packet in 0..100 {
            assert_eq!(
                source.read_boxed_slice(960 * FRAME_BYTES).unwrap().len(),
                960 * FRAME_BYTES
            );
            let deadline = packet * 20_000_000 + 15_000_000;
            assert!(clock.now() >= deadline);
            assert!(clock.now() <= deadline + 200_000);
            assert!(source.unread_buffer_len() <= QUANTUM_FRAMES * FRAME_BYTES);
        }
    }

    #[test]
    fn catchup_grants_only_two_quanta_before_waiting() {
        let (_, mut session, clock) = setup();
        frames(&mut session.input, QUANTUM_FRAMES);
        clock
            .ns
            .store(QUANTUM_NS + MAX_CATCH_UP_NS, Ordering::Relaxed);
        let mut oversized = [0; 32_768];
        for _ in 0..2 {
            assert_eq!(
                session.input.read(&mut oversized).unwrap(),
                QUANTUM_FRAMES * FRAME_BYTES
            );
            assert_eq!(clock.now(), 2 * QUANTUM_NS);
            assert_eq!(clock.waits.load(Ordering::Relaxed), 0);
        }
        assert_eq!(
            session.input.read(&mut oversized).unwrap(),
            QUANTUM_FRAMES * FRAME_BYTES
        );
        assert_eq!(clock.now(), 3 * QUANTUM_NS);
        assert!(clock.waits.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn larger_stall_resets_credit_and_cancellation_still_interrupts_wait() {
        let (_, mut session, clock) = setup();
        frames(&mut session.input, QUANTUM_FRAMES);
        let stalled = QUANTUM_NS + MAX_CATCH_UP_NS + 1;
        clock.ns.store(stalled, Ordering::Relaxed);
        frames(&mut session.input, QUANTUM_FRAMES);
        assert_eq!(clock.now(), stalled);
        *clock.cancel_on_wait.lock().unwrap() = Some(Arc::downgrade(&session.control.owner.shared));
        assert_eq!(session.input.read(&mut [0; 1]).unwrap(), 0);
        assert_eq!(clock.now(), stalled + WAIT_SLICE_NS);
        assert_eq!(session.control.snapshot().phase, CapturePhase::Stopped);
    }

    #[test]
    fn symphonia_refill_and_wraparound_do_not_bank_silence() {
        let (mut writer, session, clock) = setup();
        let control = session.control.clone();
        let mut source = MediaSourceStream::new(Box::new(session.input), Default::default());
        for packet in 0..100 {
            if packet == 50 {
                feed(&mut writer, 960, 0.125, -0.25);
            }
            let bytes = source.read_boxed_slice(960 * FRAME_BYTES).unwrap();
            assert_eq!(bytes.len(), 960 * FRAME_BYTES);
            assert!(source.unread_buffer_len() <= QUANTUM_FRAMES * FRAME_BYTES);
            // The underlying reader remains paced after MediaSourceStream's
            // exponential read size reaches 32 KiB and its ring wraps around.
            let delivered = control.owner.shared.underrun.load(Ordering::Relaxed)
                + control.owner.shared.popped.load(Ordering::Relaxed);
            assert!(delivered <= (clock.now() / QUANTUM_NS + 1) * QUANTUM_FRAMES as u64);
            let nonzero = bytes.iter().any(|&x| x != 0);
            assert_eq!(nonzero, packet == 50);
        }
    }

    #[test]
    fn overflow_is_bounded_and_drops_whole_stereo_frames() {
        let (mut writer, mut session, _) = setup();
        feed(&mut writer, QUEUE_FRAMES + 31, 0.5, -0.25);
        assert_eq!(
            session.control.snapshot().queued_frames,
            QUEUE_FRAMES as u32
        );
        assert_eq!(session.control.snapshot().dropped_frames, 31);
        assert_eq!(
            frames(&mut session.input, QUEUE_FRAMES),
            vec![[0.5, -0.25]; QUEUE_FRAMES]
        );
        feed(&mut writer, 2, -0.75, 0.125);
        assert_eq!(frames(&mut session.input, 2), vec![[-0.75, 0.125]; 2]);
    }

    #[test]
    fn stale_frames_are_discarded_before_new_input_is_returned() {
        let (mut writer, mut session, clock) = setup();
        feed(&mut writer, 700, 0.5, -0.5);
        clock.ns.store(MAX_AGE_NS + 1, Ordering::Relaxed);
        feed(&mut writer, 2, 0.25, -0.125);
        assert_eq!(frames(&mut session.input, 2), vec![[0.25, -0.125]; 2]);
        assert_eq!(session.control.snapshot().dropped_frames, 700);
        assert_eq!(session.control.snapshot().queued_frames, 0);
    }

    #[test]
    fn age_boundary_and_a_stalled_partial_frame_preserve_alignment() {
        let (mut writer, mut session, clock) = setup();
        feed(&mut writer, 1, 0.25, -0.5);
        clock.ns.store(MAX_AGE_NS, Ordering::Relaxed);
        let mut byte = [0];
        session.input.read_exact(&mut byte).unwrap();
        clock.ns.fetch_add(MAX_AGE_NS + 1, Ordering::Relaxed);
        let mut tail = [0; 7];
        session.input.read_exact(&mut tail).unwrap();
        let mut full = vec![byte[0]];
        full.extend_from_slice(&tail);
        assert_eq!(
            full,
            [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat()
        );
        // The already-started frame is completed, but no other old frame can
        // cross the age bound. This is an explicit <= 8-byte staging exception.
        assert_eq!(frames(&mut session.input, 1), vec![[0.0; 2]]);
    }

    #[test]
    fn skewed_producer_and_consumer_have_bounded_memory_and_age() {
        for produced in [950, 970] {
            let (mut writer, mut session, clock) = setup();
            for _ in 0..300 {
                let before = clock.now();
                feed(&mut writer, produced, 0.25, -0.25);
                for frame in frames(&mut session.input, 960) {
                    assert!(frame == [0.25, -0.25] || frame == [0.0; 2]);
                }
                clock.ns.fetch_max(before + 20_000_000, Ordering::Relaxed);
                assert!(session.control.snapshot().queued_frames <= QUEUE_FRAMES as u32);
            }
            if produced < 960 {
                assert!(session.control.snapshot().underrun_frames >= 300 * 10);
            } else {
                assert!(session.control.snapshot().dropped_frames > 0);
            }
            clock.ns.fetch_add(MAX_AGE_NS + 1, Ordering::Relaxed);
            assert_eq!(frames(&mut session.input, 960), vec![[0.0; 2]; 960]);
            assert_eq!(session.control.snapshot().queued_frames, 0);
        }
    }

    #[test]
    fn muted_reads_drain_and_callbacks_keep_metering() {
        let (mut writer, mut session, _) = setup();
        feed(&mut writer, 960, 0.5, -0.25);
        session.control.set_output(25, true);
        feed(&mut writer, 960, 0.75, -0.5);
        assert_eq!(session.control.snapshot().queued_frames, 960);
        assert!(
            (session.control.snapshot().level_dbfs.unwrap() - 20.0 * 0.75_f32.log10()).abs()
                < 0.001
        );
        assert_eq!(frames(&mut session.input, 960), vec![[0.0; 2]; 960]);
        assert_eq!(session.control.snapshot().queued_frames, 0);
        assert_eq!(session.control.snapshot().underrun_frames, 0);
        session.control.set_output(25, false);
        feed(&mut writer, 2, 0.5, -0.25);
        assert_eq!(frames(&mut session.input, 2), vec![[0.125, -0.0625]; 2]);
    }

    #[test]
    fn mute_unmute_without_reading_cannot_replay_queued_audio() {
        let (mut writer, mut session, _) = setup();
        feed(&mut writer, 960, 0.5, -0.5);
        session.control.set_output(100, true);
        session.control.set_output(100, false);
        feed(&mut writer, 2, 0.125, -0.25);
        assert_eq!(frames(&mut session.input, 2), vec![[0.125, -0.25]; 2]);
        assert_eq!(session.control.snapshot().dropped_frames, 960);
    }

    #[test]
    fn mute_in_a_byte_fragment_does_not_corrupt_f32_or_swap_channels() {
        let (mut writer, mut session, _) = setup();
        feed(&mut writer, 2, 0.25, -0.5);
        let mut output = [0; FRAME_BYTES * 2];
        session.input.read_exact(&mut output[..3]).unwrap();
        session.control.set_output(100, true);
        session.input.read_exact(&mut output[3..]).unwrap();
        assert_eq!(
            &output[..FRAME_BYTES],
            [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat()
        );
        assert_eq!(&output[FRAME_BYTES..], &[0; FRAME_BYTES]);
    }

    #[test]
    fn volume_changes_apply_to_queued_frames_and_meter_stays_prevolume() {
        let (mut writer, mut session, clock) = setup();
        feed(&mut writer, 3, 0.5, -1.0);
        session.control.set_output(50, false);
        assert_eq!(frames(&mut session.input, 1), vec![[0.25, -0.5]]);
        assert_eq!(session.control.snapshot().level_dbfs, Some(0.0));
        session.control.set_output(0, false);
        assert_eq!(frames(&mut session.input, 1), vec![[0.0, -0.0]]);
        session.control.set_output(255, false);
        assert_eq!(frames(&mut session.input, 1), vec![[0.5, -1.0]]);
        clock.ns.store(METER_AGE_NS + 1, Ordering::Relaxed);
        assert_eq!(session.control.snapshot().level_dbfs, Some(-96.0));
    }

    #[test]
    fn mono_and_fragmented_stereo_callbacks_convert_without_swapping() {
        let (mut writer, mut session, _) = setup();
        writer.write(&[i16::MIN, 0, i16::MAX], 1, |x| f32::from(x) / 32_768.0);
        assert_eq!(
            frames(&mut session.input, 3),
            vec![[-1.0; 2], [0.0; 2], [32_767.0 / 32_768.0; 2]]
        );
        writer.write(&[0.125], 2, |x| x);
        writer.write(&[-0.25, 0.5], 2, |x| x);
        writer.write(&[-0.75], 2, |x| x);
        assert_eq!(
            frames(&mut session.input, 2),
            vec![[0.125, -0.25], [0.5, -0.75]]
        );
    }

    #[test]
    fn incomplete_callback_frames_keep_their_age_and_mute_generation() {
        let (mut writer, mut session, clock) = setup();
        writer.write(&[0.5], 2, |x| x);
        clock.ns.store(MAX_AGE_NS + 1, Ordering::Relaxed);
        writer.write(&[-0.25], 2, |x| x);
        assert_eq!(frames(&mut session.input, 1), vec![[0.0; 2]]);
        writer.write(&[0.5], 2, |x| x);
        session.control.set_output(100, true);
        session.control.set_output(100, false);
        writer.write(&[-0.25, 0.125, -0.5], 2, |x| x);
        assert_eq!(frames(&mut session.input, 1), vec![[0.125, -0.5]]);
    }

    #[test]
    fn nonfinite_and_out_of_range_samples_are_safe() {
        let (mut writer, mut session, _) = setup();
        writer.write(&[f32::NAN, f32::INFINITY, -2.0, 3.0], 2, |x| x);
        assert_eq!(frames(&mut session.input, 2), vec![[0.0; 2], [-1.0, 1.0]]);
        assert_eq!(session.control.snapshot().level_dbfs, Some(0.0));
    }

    #[test]
    fn stop_is_idempotent_and_terminal_even_with_partial_and_queued_frames() {
        let (mut writer, mut session, _) = setup();
        let clone = session.control.clone();
        feed(&mut writer, 960, 0.5, -0.5);
        session.input.read_exact(&mut [0; 1]).unwrap();
        clone.stop();
        session.control.stop();
        feed(&mut writer, 960, 0.25, -0.25);
        assert_eq!(session.input.read(&mut [0; 100]).unwrap(), 0);
        assert_eq!(clone.snapshot().phase, CapturePhase::Stopped);
        assert_eq!(clone.snapshot().queued_frames, 0);
        assert_eq!(clone.snapshot().level_dbfs, None);
        writer.shared.fail(MISSING);
        assert_eq!(clone.snapshot().phase, CapturePhase::Stopped);
    }

    #[test]
    fn cancellation_during_a_pacing_wait_is_terminal() {
        let (_, mut session, clock) = setup();
        frames(&mut session.input, QUANTUM_FRAMES);
        *clock.cancel_on_wait.lock().unwrap() = Some(Arc::downgrade(&session.control.owner.shared));
        assert_eq!(session.input.read(&mut [0; 1]).unwrap(), 0);
        assert_eq!(clock.now(), WAIT_SLICE_NS);
        assert_eq!(session.control.snapshot().phase, CapturePhase::Stopped);
    }

    #[test]
    fn missing_and_failure_snapshots_are_visible_and_terminal() {
        for (status, phase) in [
            (MISSING, CapturePhase::Missing),
            (FAILED, CapturePhase::Error),
            (DENIED, CapturePhase::Error),
            (ROUTE_CHANGED, CapturePhase::Error),
        ] {
            let (mut writer, mut session, _) = setup();
            feed(&mut writer, 960, 0.5, -0.5);
            writer.shared.fail(status);
            writer.shared.fail(FAILED);
            assert_eq!(session.control.snapshot().phase, phase);
            assert!(session.control.snapshot().message.is_some());
            assert_eq!(
                session.input.read(&mut [0; 100]).unwrap_err().kind(),
                io::ErrorKind::BrokenPipe
            );
            assert_eq!(session.control.snapshot().queued_frames, 0);
            assert_eq!(session.control.snapshot().level_dbfs, None);
        }
    }

    #[test]
    fn xrun_counts_events_and_accepts_subsequent_input() {
        let (mut writer, mut session, _) = setup();
        for active in [OPENING, RUNNING] {
            writer.shared.status.store(active, Ordering::Release);
            writer.shared.record_xrun();
            assert_eq!(writer.shared.status.load(Ordering::Acquire), active);
        }
        assert_eq!(session.control.xrun_events(), 2);
        feed(&mut writer, 2, 0.125, -0.25);
        assert_eq!(frames(&mut session.input, 2), vec![[0.125, -0.25]; 2]);
        assert_eq!(session.control.snapshot().phase, CapturePhase::Running);
        assert_eq!(session.control.snapshot().dropped_frames, 0);
        session.control.stop();
        writer.shared.record_xrun();
        assert_eq!(session.control.xrun_events(), 2);
        assert_eq!(session.control.snapshot().phase, CapturePhase::Stopped);
    }

    #[test]
    fn opening_failure_retries_when_startup_promotes_between_observation_and_cas() {
        let (writer, mut session, _) = setup();
        writer.shared.status.store(OPENING, Ordering::Release);
        let mut observations = Vec::new();
        writer.shared.fail_observed(MISSING, |observed| {
            observations.push(observed);
            if observed == OPENING {
                writer
                    .shared
                    .status
                    .compare_exchange(OPENING, RUNNING, Ordering::AcqRel, Ordering::Acquire)
                    .unwrap();
            }
        });
        assert_eq!(observations, [OPENING, RUNNING]);
        assert_eq!(session.control.snapshot().phase, CapturePhase::Missing);
        assert_eq!(
            session.input.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn failure_retry_preserves_concurrent_stop_or_first_terminal_failure() {
        for terminal in [STOPPED, DENIED] {
            let (writer, session, _) = setup();
            writer.shared.status.store(OPENING, Ordering::Release);
            writer.shared.fail_observed(MISSING, |observed| {
                assert_eq!(observed, OPENING);
                writer
                    .shared
                    .status
                    .compare_exchange(OPENING, terminal, Ordering::AcqRel, Ordering::Acquire)
                    .unwrap();
            });
            assert_eq!(writer.shared.status.load(Ordering::Acquire), terminal);
            writer.shared.fail(FAILED);
            assert_eq!(writer.shared.status.load(Ordering::Acquire), terminal);
            assert_eq!(
                session.control.snapshot().phase,
                if terminal == STOPPED {
                    CapturePhase::Stopped
                } else {
                    CapturePhase::Error
                }
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_callback_xrun_is_nonterminal_but_device_loss_is_terminal() {
        let (mut writer, mut session, _) = setup();
        windows::report_error(&writer.shared, cpal::ErrorKind::Xrun);
        assert_eq!(session.control.xrun_events(), 1);
        feed(&mut writer, 2, 0.25, -0.5);
        assert_eq!(frames(&mut session.input, 2), vec![[0.25, -0.5]; 2]);
        assert_eq!(session.control.snapshot().phase, CapturePhase::Running);
        assert_eq!(session.control.snapshot().dropped_frames, 0);
        windows::report_error(&writer.shared, cpal::ErrorKind::DeviceNotAvailable);
        assert_eq!(session.control.snapshot().phase, CapturePhase::Missing);
        assert_eq!(
            session.input.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn failure_racing_read_budget_reports_error_instead_of_eof() {
        let (_, mut session, clock) = setup();
        *clock.fail_on_now.lock().unwrap() = Some(Arc::downgrade(&session.control.owner.shared));
        assert_eq!(
            session.input.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(session.control.snapshot().phase, CapturePhase::Missing);
    }

    #[test]
    fn telemetry_counters_saturate_at_the_public_boundary() {
        let (_, session, _) = setup();
        session
            .control
            .owner
            .shared
            .dropped
            .store(u64::from(u32::MAX) + 1, Ordering::Relaxed);
        session
            .control
            .owner
            .shared
            .underrun
            .store(u64::MAX, Ordering::Relaxed);
        assert_eq!(session.control.snapshot().dropped_frames, u32::MAX);
        assert_eq!(session.control.snapshot().underrun_frames, u32::MAX);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_capture_returns_explicit_error() {
        assert_eq!(
            enumerate_sources().unwrap_err().code,
            crate::model::ErrorCode::CaptureFailed
        );
        assert!(start_capture("wasapi:synthetic").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn actual_songbird_raw_reader_preserves_live_pcm_silence_resume_and_pacing() {
        use songbird::input::{Input, LiveInput, RawAdapter, codecs};
        use symphonia_core::audio::{AudioBufferRef, Signal};

        let (mut writer, session, clock) = setup();
        let control = session.control;
        let adapter = RawAdapter::new(session.input, SAMPLE_RATE, CHANNELS);
        let Input::Live(input, _) = Input::from(adapter) else {
            panic!("expected live input")
        };
        // Exercise Songbird's actual probe, RawReader, and registered PCM
        // decoder, rather than reproducing their container behavior in a mock.
        let LiveInput::Parsed(mut parsed) = input
            .promote(codecs::get_codec_registry(), codecs::get_probe())
            .unwrap()
        else {
            panic!("expected parsed input")
        };
        assert_eq!(
            parsed.format.tracks()[0].codec_params.sample_rate,
            Some(SAMPLE_RATE)
        );
        assert_eq!(
            parsed.format.tracks()[0]
                .codec_params
                .channels
                .unwrap()
                .count(),
            2
        );
        for n in 0..100 {
            if n >= 50 {
                feed(&mut writer, 960, 0.125, -0.5);
            }
            let packet = parsed.format.next_packet().unwrap();
            assert_eq!(packet.dur(), 960);
            assert_eq!(packet.ts(), n * 960);
            assert_eq!(packet.data.len(), 960 * FRAME_BYTES);
            for frame in packet.data.as_chunks::<FRAME_BYTES>().0 {
                let expected = if n >= 50 { [0.125_f32, -0.5] } else { [0.0; 2] };
                assert_eq!(
                    frame.as_slice(),
                    [expected[0].to_le_bytes(), expected[1].to_le_bytes()].concat()
                );
            }
            let AudioBufferRef::F32(decoded) = parsed.decoder.decode(&packet).unwrap() else {
                panic!("expected f32 PCM")
            };
            assert_eq!(decoded.frames(), 960);
            assert!(
                decoded
                    .chan(0)
                    .iter()
                    .all(|&s| s == if n >= 50 { 0.125 } else { 0.0 })
            );
            assert!(
                decoded
                    .chan(1)
                    .iter()
                    .all(|&s| s == if n >= 50 { -0.5 } else { 0.0 })
            );
            assert!(clock.now() >= (n * 20_000_000 + 15_000_000));
        }
        let stream = parsed.format.into_inner();
        assert!(stream.unread_buffer_len() <= QUANTUM_FRAMES * FRAME_BYTES);
        assert_eq!(control.snapshot().phase, CapturePhase::Running);
        assert_eq!(control.snapshot().underrun_frames, 50 * 960);
        control.stop();
    }

    #[cfg(windows)]
    #[test]
    fn capture_format_selection_requires_supported_48k_mono_or_stereo() {
        use cpal::{SampleFormat, SupportedBufferSize, SupportedStreamConfigRange};
        let range = |channels, min, max, format| {
            SupportedStreamConfigRange::new(
                channels,
                min,
                max,
                SupportedBufferSize::Unknown,
                format,
            )
        };
        let rejected = [
            range(2, 44_100, 44_100, SampleFormat::F32),
            range(6, 48_000, 48_000, SampleFormat::F32),
            range(2, 48_000, 48_000, SampleFormat::DsdU8),
        ];
        assert!(windows::select_config(rejected.into_iter()).is_none());
        let available = [
            range(1, 44_100, 96_000, SampleFormat::F32),
            range(2, 44_100, 96_000, SampleFormat::I16),
            range(2, 48_000, 48_000, SampleFormat::F32),
        ];
        let selected = windows::select_config(available.into_iter()).unwrap();
        assert_eq!(selected.channels(), 2);
        assert_eq!(selected.sample_rate(), 48_000);
        assert_eq!(selected.sample_format(), SampleFormat::F32);
        let mono =
            windows::select_config([range(1, 48_000, 48_000, SampleFormat::U16)].into_iter())
                .unwrap();
        assert_eq!(mono.channels(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn cpal_pcm_conversion_normalizes_signed_unsigned_and_float() {
        use cpal::Sample;
        let (mut writer, mut session, _) = setup();
        writer.write(&[i16::MIN, 0, i16::MAX], 1, |s| s.to_sample::<f32>());
        writer.write(&[0_u16, 32_768, u16::MAX], 1, |s| s.to_sample::<f32>());
        writer.write(&[-0.5_f64, 0.25], 1, |s| s.to_sample::<f32>());
        let data = frames(&mut session.input, 8);
        assert_eq!(data[..3], data[3..6]);
        assert_eq!(data[0], [-1.0; 2]);
        assert_eq!(data[1], [0.0; 2]);
        assert!(data[2][0] > 0.9999 && data[2][0] < 1.0);
        assert_eq!(data[6..], [[-0.5; 2], [0.25; 2]]);
    }
}
