//! Selected-endpoint capture and a bounded, paced 48 kHz stereo f32-LE source.
//!
//! Wrap `CaptureSession::input` in `songbird::input::RawAdapter::new(input, 48_000, 2)`.
//! Reads never report temporary EOF or WouldBlock. Silence uses the same real-time
//! budget as captured audio, including when a buffering reader repeatedly refills.
//! A slow consumer cannot accumulate unbounded pacing credit. This bounds source read-ahead;
//! already returned bytes in the decoder/mixer remain outside this queue's control.

use crate::model::{AppError, Capture, CapturePhase, RecordingSource};
#[path = "application.rs"]
pub mod application;
#[cfg(any(windows, test))]
pub(crate) mod recording_format;
use rtrb::Consumer;
#[cfg(any(windows, test))]
use rtrb::{Producer, RingBuffer};
#[cfg(windows)]
use std::time::{Duration, Instant};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
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
const SILENCE_DBFS: f32 = -96.0;
const MUTED: u64 = 1 << 32;
const GENERATION_SHIFT: u32 = 33;
const STOPPED: u8 = 0;
const OPENING: u8 = 1;
const RUNNING: u8 = 2;
const MISSING: u8 = 3;
#[cfg(any(windows, test))]
const FAILED: u8 = 4;
const DENIED: u8 = 5;
const ROUTE_CHANGED: u8 = 6;
#[cfg(any(windows, test))]
const APPLICATION_EXITED: u8 = 7;
#[cfg(any(windows, test))]
const CLEANUP_FAILED: u8 = 8;

pub(crate) fn observation_clock_ms() -> f64 {
    #[cfg(windows)]
    let milliseconds = crate::mix_bus::now_ms() as f64;
    #[cfg(not(windows))]
    let milliseconds = {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_secs_f64()
            * 1000.0
    };
    milliseconds.min(9_007_199_254_740_991.0)
}

/// Accept only finite JS-safe milliseconds from the shared clock. One ms of
/// tolerance covers integer clock quantization; future measurements stay absent.
pub(crate) fn clean_observation_time(value: Option<f64>, now_ms: f64) -> Option<f64> {
    value.filter(|value| {
        value.is_finite()
            && (0.0..=9_007_199_254_740_991.0).contains(value)
            && now_ms.is_finite()
            && *value <= now_ms + 1.0
    })
}

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
    #[cfg(all(windows, test))]
    pub(crate) fn simulate_application_exit(&self) {
        self.owner.shared.fail(APPLICATION_EXITED);
    }
    #[cfg(any(windows, test))]
    pub(crate) fn failure_code(&self) -> crate::model::ErrorCode {
        match self.owner.shared.status.load(Ordering::Acquire) {
            APPLICATION_EXITED | MISSING => crate::model::ErrorCode::InvalidSelection,
            CLEANUP_FAILED => crate::model::ErrorCode::CleanupIncomplete,
            _ => crate::model::ErrorCode::CaptureFailed,
        }
    }
    /// Idempotently cancel reads and reject future callback frames, then release
    /// the native stream. CPAL stream destruction can join its native thread;
    /// the parent engine-process exit deadline remains the shutdown guarantee.
    pub fn stop(&self) -> bool {
        #[cfg(windows)]
        let _stop = self
            .owner
            .stop_gate
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        #[cfg(windows)]
        if self.owner.shared.status.load(Ordering::Acquire) == CLEANUP_FAILED {
            return false;
        }
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
            let worker = self
                .owner
                .worker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            if let Some(worker) = worker {
                let deadline = Instant::now() + Duration::from_millis(200);
                while !worker.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(2));
                }
                if !worker.is_finished() {
                    self.owner
                        .shared
                        .status
                        .store(CLEANUP_FAILED, Ordering::Release);
                    return false;
                }
                if worker.join().is_err() {
                    self.owner
                        .shared
                        .status
                        .store(CLEANUP_FAILED, Ordering::Release);
                    return false;
                }
            }
        }
        true
    }

    /// Volume is clamped to 0..=100 as a defensive boundary; the command layer
    /// validates user input. Gain is applied when a frame is handed to the reader.
    /// Every mute transition invalidates older queued frames, even if unmuted
    /// before the reader runs. At most one already-started frame is staged here.
    pub fn set_output(&self, volume_percent: u8, muted: bool) {
        self.set_linear_output(volume_percent.min(100) as f32 / 100.0, muted);
    }

    pub fn set_linear_output(&self, gain: f32, muted: bool) {
        let gain = if gain.is_finite() {
            gain.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let shared = &self.owner.shared;
        let mut old = shared.output.load(Ordering::Acquire);
        loop {
            let generation =
                (old >> GENERATION_SHIFT).wrapping_add(u64::from((old & MUTED != 0) != muted));
            let new = (generation << GENERATION_SHIFT)
                | u64::from(gain.clamp(0.0, 1.0).to_bits())
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

    /// Latches every native pre-gain clipping observation until consumed.
    pub fn take_pre_clip(&self) -> bool {
        self.owner.shared.pre_clip.swap(false, Ordering::AcqRel)
    }

    /// Number of nonterminal native underrun/overrun/discontinuity notifications.
    /// This is an event count; the backend has not supplied a lost-frame count.
    /// Kept separate from the shared UI/TypeScript snapshot and dropped frames.
    pub fn xrun_events(&self) -> u64 {
        self.owner.shared.xruns.load(Ordering::Relaxed)
    }

    /// Input is the pre-volume callback peak, independent of mute. Output is a
    /// peak held for at most 200 ms over completed post-gain reader frames.
    /// Counts are stereo frames and saturate at u32::MAX. Occupancy is approximate.
    pub fn snapshot(&self) -> Capture {
        self.snapshot_with_observation().0
    }

    pub(crate) fn snapshot_with_observation(&self) -> (Capture, Option<f64>) {
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
            #[cfg(any(windows, test))]
            APPLICATION_EXITED => (
                CapturePhase::Missing,
                Some(
                    "The selected application exited. Refresh sources and select its new instance.",
                ),
            ),
            #[cfg(any(windows, test))]
            CLEANUP_FAILED => (
                CapturePhase::Error,
                Some(
                    "Capture cleanup did not finish. Choose Quit from the tray, then reopen Echocord.",
                ),
            ),
            _ => (
                CapturePhase::Error,
                Some("Capture failed. Reconnect the selected recording device."),
            ),
        };
        let peak_bits = shared.peak_bits.load(Ordering::Acquire);
        let level_dbfs = if status == RUNNING && peak_bits != u32::MAX {
            if shared
                .clock
                .now()
                .saturating_sub(shared.last_callback_ns.load(Ordering::Acquire))
                <= METER_AGE_NS
            {
                Some(peak_dbfs(f32::from_bits(peak_bits)))
            } else {
                None
            }
        } else {
            None
        };
        let popped = shared.popped.load(Ordering::Acquire);
        let pushed = shared.pushed.load(Ordering::Acquire);
        let output = if status == RUNNING {
            shared.output_meter.observation(shared)
        } else {
            None
        };
        (
            Capture {
                phase,
                level_dbfs,
                output_level_dbfs: output.map(|(level, _)| level),
                message: message.map(str::to_owned),
                dropped_frames: count(shared.dropped.load(Ordering::Relaxed)),
                underrun_frames: count(shared.underrun.load(Ordering::Relaxed)),
                queued_frames: if status == RUNNING || status == OPENING {
                    pushed.saturating_sub(popped).min(QUEUE_FRAMES as u64) as u32
                } else {
                    0
                },
            },
            output.map(|(_, observed)| observed),
        )
    }
}

fn count(value: u64) -> u32 {
    value.min(u64::from(u32::MAX)) as u32
}

fn peak_dbfs(peak: f32) -> f32 {
    if peak > 0.0 {
        (20.0 * peak.log10()).max(SILENCE_DBFS)
    } else {
        SILENCE_DBFS
    }
}

/// One PCM-reader writer, with a bounded single-attempt snapshot. Sequentially
/// consistent atomics make the sequence and payload one coherent observation;
/// a concurrent publication returns null instead of spinning or mixing fields.
/// Capture callbacks never access this meter.
#[derive(Default)]
struct OutputMeter {
    sequence: AtomicU64,
    peak_bits: AtomicU32,
    peak_ns: AtomicU64,
    completed_ns: AtomicU64,
    observed_at_ms: AtomicU64,
    output: AtomicU64,
}

impl OutputMeter {
    fn publish(&self, peak: ReaderPeak, completed_ns: Option<u64>) {
        self.sequence.fetch_add(1, Ordering::SeqCst);
        self.peak_bits.store(peak.value.to_bits(), Ordering::SeqCst);
        self.peak_ns.store(peak.ns, Ordering::SeqCst);
        if let Some(completed) = completed_ns {
            self.completed_ns.store(completed, Ordering::SeqCst);
            self.observed_at_ms
                .store(observation_clock_ms().floor() as u64, Ordering::SeqCst);
        }
        self.output.store(peak.output, Ordering::SeqCst);
        self.sequence.fetch_add(1, Ordering::SeqCst);
    }

    fn observation(&self, shared: &Shared) -> Option<(f32, f64)> {
        let output = shared.output.load(Ordering::Acquire);
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence == 0 || sequence & 1 != 0 {
            return None;
        }
        let peak = f32::from_bits(self.peak_bits.load(Ordering::SeqCst));
        let ns = self.peak_ns.load(Ordering::SeqCst);
        let completed = self.completed_ns.load(Ordering::SeqCst);
        let observed_at_ms = self.observed_at_ms.load(Ordering::SeqCst);
        let observed_output = self.output.load(Ordering::SeqCst);
        if sequence != self.sequence.load(Ordering::SeqCst)
            || output != observed_output
            || output != shared.output.load(Ordering::Acquire)
        {
            return None;
        }
        let now = shared.clock.now();
        (now.saturating_sub(ns) <= METER_AGE_NS && now.saturating_sub(completed) <= METER_AGE_NS)
            .then(|| (peak_dbfs(peak), observed_at_ms as f64))
    }
}

struct Owner {
    shared: Arc<Shared>,
    #[cfg(windows)]
    stream: Mutex<Option<cpal::Stream>>,
    #[cfg(windows)]
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    #[cfg(windows)]
    stop_gate: Mutex<()>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.shared.status.store(STOPPED, Ordering::Release);
    }
}

struct Shared {
    status: AtomicU8,
    output: AtomicU64,
    input_epoch: AtomicU64,
    peak_bits: AtomicU32,
    pre_clip: AtomicBool,
    last_callback_ns: AtomicU64,
    output_meter: OutputMeter,
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
    input_epoch: u64,
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
    pending_peak: ReaderPeak,
    output_peak: Option<ReaderPeak>,
    frames_left: usize,
    next_quantum_ns: Option<u64>,
    position: u64,
}

#[derive(Clone, Copy, Default)]
struct ReaderPeak {
    value: f32,
    ns: u64,
    output: u64,
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
        let mut completed = false;
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
                let (samples, output) = self.next_frame(shared, &mut pop_budget);
                self.pending_peak = ReaderPeak {
                    value: samples[0].abs().max(samples[1].abs()),
                    ns: shared.clock.now(),
                    output,
                };
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
            if self.pending_pos == FRAME_BYTES {
                completed = true;
                let peak = self.pending_peak;
                if peak.output == shared.output.load(Ordering::Acquire)
                    && self.output_peak.is_none_or(|held| {
                        held.output != peak.output
                            || peak.ns.saturating_sub(held.ns) > METER_AGE_NS
                            || peak.value >= held.value
                    })
                {
                    self.output_peak = Some(peak);
                }
            }
        }
        if let Some(peak) = self.output_peak {
            let now = shared.clock.now();
            shared.output_meter.publish(
                peak,
                (completed && now.saturating_sub(peak.ns) <= METER_AGE_NS).then_some(now),
            );
        }
        self.position = self.position.saturating_add(written as u64);
        Ok(written)
    }

    fn next_frame(&mut self, shared: &Shared, budget: &mut usize) -> ([f32; 2], u64) {
        while *budget > 0 {
            let Ok(frame) = self.consumer.pop() else {
                break;
            };
            *budget -= 1;
            shared.popped.fetch_add(1, Ordering::Release);
            let output = shared.output.load(Ordering::Acquire);
            if shared.clock.now().saturating_sub(frame.exposed_ns) > MAX_AGE_NS
                || frame.input_epoch != shared.input_epoch.load(Ordering::Acquire)
                || frame.generation != output >> GENERATION_SHIFT
                || output & MUTED != 0
            {
                shared.dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let gain = f32::from_bits(output as u32);
            return ([frame.samples[0] * gain, frame.samples[1] * gain], output);
        }
        // Intentional mute is not an underrun. Muted reads still
        // drain old queued frames instead of leaving audio to replay later.
        let output = shared.output.load(Ordering::Acquire);
        if output & MUTED == 0 {
            shared.underrun.fetch_add(1, Ordering::Relaxed);
        }
        ([0.0; 2], output)
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

impl LivePcm {
    /// The shared mixer owns pacing. This bounded drain never waits for a source.
    pub fn drain_frames(&mut self, frames: &mut [[f32; 2]]) {
        let shared = &self.owner.shared;
        let reader = self.reader.get_mut().unwrap_or_else(|e| e.into_inner());
        let mut budget = QUEUE_FRAMES;
        for frame in frames {
            *frame = if matches!(shared.status.load(Ordering::Acquire), RUNNING | OPENING) {
                reader.next_frame(shared, &mut budget).0
            } else {
                [0.0; 2]
            };
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
        let fence = (
            self.shared.input_epoch.load(Ordering::Acquire),
            self.shared.output.load(Ordering::Acquire) >> GENERATION_SHIFT,
        );
        self.write_fenced(data, channels, convert, fence);
    }

    fn write_fenced<T: Copy>(
        &mut self,
        data: &[T],
        channels: u16,
        convert: impl Fn(T) -> f32,
        fence: (u64, u64),
    ) {
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
            ) || self.shared.input_epoch.load(Ordering::Acquire) != fence.0
                || self.shared.output.load(Ordering::Acquire) >> GENERATION_SHIFT != fence.1
            {
                self.partial_left = None;
                return;
            }
            let sample = convert(raw);
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            peak = peak.max(sample.abs());
            if sample.abs() >= 1.0 {
                self.shared.pre_clip.store(true, Ordering::Release);
            }
            let output = self.shared.output.load(Ordering::Acquire);
            let generation = output >> GENERATION_SHIFT;
            if generation != fence.1 || self.shared.input_epoch.load(Ordering::Acquire) != fence.0 {
                self.partial_left = None;
                return;
            }
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
                    input_epoch: fence.0,
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

/// The native callback owns the converter and writer together. A discontinuity
/// invalidates both queued output and un-emitted filter/staging history.
#[cfg(any(windows, test))]
struct RecordingWriter {
    writer: CaptureWriter,
    resampler: Option<recording_format::RecordingResampler>,
    channels: u16,
    fence: (u64, u64),
    pending_right: bool,
    discard_right: bool,
}

#[cfg(any(windows, test))]
impl RecordingWriter {
    fn new(writer: CaptureWriter, rate: u32, channels: u16) -> Option<Self> {
        if !matches!(channels, 1 | 2) {
            return None;
        }
        let resampler = if rate == SAMPLE_RATE {
            None
        } else {
            Some(recording_format::RecordingResampler::new(rate, channels)?)
        };
        let fence = (
            writer.shared.input_epoch.load(Ordering::Acquire),
            writer.shared.output.load(Ordering::Acquire) >> GENERATION_SHIFT,
        );
        Some(Self {
            writer,
            resampler,
            channels,
            fence,
            pending_right: false,
            discard_right: false,
        })
    }

    fn write<T: Copy>(&mut self, mut data: &[T], convert: impl Fn(T) -> f32) {
        let shared = &self.writer.shared;
        if !matches!(shared.status.load(Ordering::Acquire), RUNNING | OPENING) {
            return;
        }
        let fence = (
            shared.input_epoch.load(Ordering::Acquire),
            shared.output.load(Ordering::Acquire) >> GENERATION_SHIFT,
        );
        if fence != self.fence {
            // Mute does not change the native stream's sample position. If an
            // odd callback left half a stereo frame, discard its matching right
            // sample too. A native discontinuity starts a fresh whole frame.
            if fence.0 != self.fence.0 {
                self.pending_right = false;
                self.discard_right = false;
            } else {
                self.discard_right = self.pending_right;
            }
            if let Some(resampler) = &mut self.resampler {
                resampler.reset();
            }
            self.writer.partial_left = None;
            self.fence = fence;
        }
        if self.discard_right && !data.is_empty() {
            data = &data[1..];
            self.pending_right = false;
            self.discard_right = false;
        }
        // Track the complete delivered slice, even if a concurrent generation
        // change makes the fenced writer reject its remaining samples.
        if self.channels == 2 && !data.len().is_multiple_of(2) {
            self.pending_right = !self.pending_right;
        }
        if let Some(resampler) = &mut self.resampler {
            let writer = &mut self.writer;
            match resampler.write(data, convert, |samples| {
                writer.write_fenced(samples, 2, |s| s, fence)
            }) {
                Ok(true) => writer.shared.pre_clip.store(true, Ordering::Release),
                Ok(false) => {}
                Err(_) => writer.shared.fail(FAILED),
            }
        } else {
            // Keep the existing 48 kHz path free of filtering or staging delay.
            self.writer
                .write_fenced(data, self.channels, convert, fence);
        }
    }
}

#[cfg(any(windows, test))]
fn pipeline(clock: Arc<dyn Clock>) -> (CaptureWriter, CaptureSession) {
    let (producer, consumer) = RingBuffer::new(QUEUE_FRAMES);
    let shared = Arc::new(Shared {
        status: AtomicU8::new(OPENING),
        output: AtomicU64::new(u64::from(1.0_f32.to_bits())),
        input_epoch: AtomicU64::new(0),
        peak_bits: AtomicU32::new(u32::MAX),
        pre_clip: AtomicBool::new(false),
        last_callback_ns: AtomicU64::new(0),
        output_meter: OutputMeter::default(),
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
        #[cfg(windows)]
        worker: Mutex::new(None),
        #[cfg(windows)]
        stop_gate: Mutex::new(()),
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
                    pending_peak: ReaderPeak::default(),
                    output_peak: None,
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
pub fn enumerate_sources() -> Result<Vec<RecordingSource>, AppError> {
    let mut sources = windows::enumerate_sources()?;
    sources.extend(application::enumerate_sources()?);
    Ok(sources)
}

#[cfg(windows)]
pub fn start_capture(source_id: &str) -> Result<CaptureSession, AppError> {
    start_capture_with_cancel(source_id, &|| false)
}

#[cfg(windows)]
pub fn start_capture_with_cancel(
    source_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<CaptureSession, AppError> {
    if source_id.starts_with("mix-handle:") {
        return crate::mix_bus::capture(source_id);
    }
    if application::is_application_id(source_id) {
        application::start_capture(source_id, cancelled)
    } else {
        windows::start_capture(source_id)
    }
}

#[cfg(windows)]
pub fn validate_source(source_id: &str) -> Result<(), AppError> {
    if application::is_application_id(source_id) {
        application::validate_source(source_id)
    } else {
        Ok(())
    }
}

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
        // Negotiate only on the explicitly selected endpoint. Prefer the engine
        // rate; the two reviewed fallback rates use our bounded converter.
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
                recording_format::RECORDING_RATES
                    .iter()
                    .enumerate()
                    .find_map(|(rate_rank, &rate)| {
                        range
                            .try_with_sample_rate(rate)
                            .map(|config| ((rate_rank, channel_rank, rank), config))
                    })
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
                Ok(Some(config)) => (
                    true,
                    Some(if config.sample_rate() == SAMPLE_RATE {
                        "48 kHz mono/stereo recording".into()
                    } else {
                        format!(
                            "{} kHz recording · converted to 48 kHz",
                            f64::from(config.sample_rate()) / 1000.0
                        )
                    }),
                ),
                Ok(None) => (
                    false,
                    Some("Requires 44.1, 48 or 96 kHz mono/stereo PCM.".into()),
                ),
                Err(_) => (
                    false,
                    Some("Recording device formats are unavailable.".into()),
                ),
            };
            sources.push(RecordingSource {
                id,
                name,
                kind: crate::model::SourceKind::Recording,
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
            "The selected recording device requires 44.1, 48 or 96 kHz mono/stereo PCM. Change its Windows format and retry.",
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
        .map_err(|_| error("Cannot open the selected recording device's supported PCM format."))?;
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
        writer: CaptureWriter,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample + Copy,
        f32: cpal::FromSample<T>,
    {
        let shared = writer.shared.clone();
        let mut writer = RecordingWriter::new(writer, config.sample_rate(), config.channels())
            .ok_or_else(|| {
                cpal::Error::with_message(
                    cpal::ErrorKind::UnsupportedConfig,
                    "Unsupported recording conversion",
                )
            })?;
        device.build_input_stream::<T, _, _>(
            config.config(),
            move |data, _| writer.write(data, |sample| sample.to_sample::<f32>()),
            move |failure| report_error(&shared, failure.kind()),
            None,
        )
    }

    pub(super) fn report_error(shared: &Shared, kind: cpal::ErrorKind) {
        // No logging, string formatting, locks, allocation, UI, or I/O.
        // WASAPI reports discontinuity and continues handing off capture data.
        if kind == cpal::ErrorKind::Xrun {
            shared.record_xrun();
            shared.input_epoch.fetch_add(1, Ordering::AcqRel);
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

#[cfg(windows)]
pub(crate) fn bus_capture(
    mut pump: impl FnMut(&mut [[f32; 2]]) + Send + 'static,
) -> CaptureSession {
    let (mut writer, session) = pipeline(Arc::new(SystemClock(Instant::now())));
    let shared = writer.shared.clone();
    shared.status.store(RUNNING, Ordering::Release);
    let thread = std::thread::spawn(move || {
        let mut frames = [[0.0; 2]; QUANTUM_FRAMES];
        let mut next = Instant::now();
        while shared.status.load(Ordering::Acquire) == RUNNING {
            pump(&mut frames);
            for frame in frames {
                writer.write(&frame, 2, |x| x);
            }
            next += Duration::from_millis(5);
            let now = Instant::now();
            if now < next {
                std::thread::sleep(next - now);
            } else if now.duration_since(next) > Duration::from_millis(5) {
                next = now;
            }
        }
    });
    *session
        .control
        .owner
        .worker
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(thread);
    session
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

    #[test]
    fn application_exit_and_unconfirmed_cleanup_have_distinct_retry_policy() {
        let (writer, session, _) = setup();
        writer.shared.fail(APPLICATION_EXITED);
        assert_eq!(session.control.snapshot().phase, CapturePhase::Missing);
        assert_eq!(
            session.control.failure_code(),
            crate::model::ErrorCode::InvalidSelection
        );
        let (writer, session, _) = setup();
        writer.shared.fail(CLEANUP_FAILED);
        assert_eq!(
            session.control.failure_code(),
            crate::model::ErrorCode::CleanupIncomplete
        );
        assert!(
            session
                .control
                .snapshot()
                .message
                .unwrap()
                .contains("Quit from the tray")
        );
    }

    #[test]
    fn reader_observation_origin_is_stable_until_real_handoff_and_stale_is_unavailable() {
        let (mut writer, mut session, clock) = setup();
        feed(&mut writer, 2, 0.25, -0.5);
        frames(&mut session.input, 1);
        let first = session.control.snapshot_with_observation();
        assert!(first.1.is_some());
        assert_eq!(session.control.snapshot_with_observation().1, first.1);
        session.input.read_exact(&mut [0; 1]).unwrap();
        assert_eq!(session.control.snapshot_with_observation().1, first.1);
        clock.ns.store(METER_AGE_NS + 1, Ordering::Relaxed);
        let stale = session.control.snapshot_with_observation();
        assert_eq!(stale.0.level_dbfs, None);
        assert_eq!(stale.0.output_level_dbfs, None);
        assert_eq!(stale.1, None);
    }

    #[test]
    fn observation_times_reject_nonfinite_negative_future_and_unsafe_values() {
        for time in [
            f64::NAN,
            f64::INFINITY,
            -1.0,
            1000.0,
            9_007_199_254_740_992.0,
        ] {
            assert_eq!(clean_observation_time(Some(time), 10.0), None);
        }
        assert_eq!(clean_observation_time(Some(0.0), 10.0), Some(0.0));
        assert_eq!(clean_observation_time(Some(10.0), 10.0), Some(10.0));
    }

    #[test]
    fn discontinuity_epoch_discards_queued_previous_signal() {
        let (mut writer, mut session, _) = setup();
        feed(&mut writer, 100, 0.9, -0.9);
        writer.shared.input_epoch.fetch_add(1, Ordering::AcqRel);
        feed(&mut writer, 10, 0.1, -0.1);
        assert_eq!(frames(&mut session.input, 10), vec![[0.1, -0.1]; 10]);
        assert_eq!(session.control.snapshot().dropped_frames, 100);
    }

    #[test]
    fn recording_48k_bypasses_filter_and_preserves_fragmented_stereo() {
        let (writer, mut session, _) = setup();
        let mut recording = RecordingWriter::new(writer, 48_000, 2).unwrap();
        assert!(recording.resampler.is_none());
        recording.write(&[0.125], |s| s);
        recording.write(&[-0.25, 0.5, -0.75], |s| s);
        assert_eq!(
            frames(&mut session.input, 2),
            [[0.125, -0.25], [0.5, -0.75]]
        );
        assert_eq!(session.control.snapshot().underrun_frames, 0);
    }

    #[test]
    fn converted_recording_discontinuity_and_mute_discard_pending_audio() {
        for rate in [44_100, 48_000, 96_000] {
            for mute in [false, true] {
                let (writer, mut session, _) = setup();
                let mut recording = RecordingWriter::new(writer, rate, 2).unwrap();
                recording.write(&[0.75; 1025], |s| s);
                if mute {
                    session.control.set_output(100, true);
                    session.control.set_output(100, false);
                } else {
                    recording
                        .writer
                        .shared
                        .input_epoch
                        .fetch_add(1, Ordering::AcqRel);
                }
                recording.write(&[0.0; 2048], |s| s);
                let mut actual = [[9.0; 2]; 240];
                session.input.drain_frames(&mut actual);
                assert_eq!(actual, [[0.0; 2]; 240]);
                assert_eq!(session.control.snapshot().underrun_frames, 0);
                assert!(session.control.snapshot().dropped_frames > 0);
            }
        }
    }

    #[test]
    fn mute_between_fragmented_callbacks_preserves_stereo_alignment() {
        use std::cell::Cell;
        for rate in [44_100, 48_000, 96_000] {
            for during_callback in [false, true] {
                let (writer, mut session, _) = setup();
                let mut recording = RecordingWriter::new(writer, rate, 2).unwrap();
                let remaining = Cell::new(7u32);
                recording.write(&[0.75; 1025], |sample| {
                    if during_callback {
                        let next = remaining.get().saturating_sub(1);
                        remaining.set(next);
                        if next == 0 {
                            remaining.set(u32::MAX);
                            session.control.set_output(100, true);
                            session.control.set_output(100, false);
                        }
                    }
                    sample
                });
                if !during_callback {
                    session.control.set_output(100, true);
                    session.control.set_output(100, false);
                }
                // Empty callbacks and another mute cycle must not lose the
                // pending right-sample discard after history has been reset.
                recording.write::<f32>(&[], |s| s);
                session.control.set_output(100, true);
                session.control.set_output(100, false);
                let mut source = vec![-0.875];
                source.extend((0..1024).flat_map(|_| [0.25, -0.5]));
                recording.write(&source, |s| s);
                let mut actual = [[0.0; 2]; 240];
                session.input.drain_frames(&mut actual);
                for frame in &actual[230..] {
                    assert!((frame[0] - 0.25).abs() < 0.001, "rate {rate}: {frame:?}");
                    assert!((frame[1] + 0.5).abs() < 0.001, "rate {rate}: {frame:?}");
                }
                assert_eq!(session.control.snapshot().underrun_frames, 0);
            }
        }
    }

    #[test]
    fn discontinuity_during_conversion_cannot_stamp_old_samples_as_new() {
        use std::cell::Cell;
        for rate in [44_100, 48_000, 96_000] {
            for mute in [false, true] {
                let (writer, mut session, _) = setup();
                let mut recording = RecordingWriter::new(writer, rate, 2).unwrap();
                let shared = recording.writer.shared.clone();
                let remaining = Cell::new(7u32);
                recording.write(&[0.75; 2048], |s| {
                    let next = remaining.get().saturating_sub(1u32);
                    remaining.set(next);
                    if next == 0 {
                        remaining.set(u32::MAX);
                        if mute {
                            session.control.set_output(100, true);
                            session.control.set_output(100, false);
                        } else {
                            shared.input_epoch.fetch_add(1, Ordering::AcqRel);
                        }
                    }
                    s
                });
                recording.write(&[0.0; 2048], |s| s);
                let mut actual = [[9.0; 2]; 240];
                session.input.drain_frames(&mut actual);
                assert_eq!(actual, [[0.0; 2]; 240]);
                assert_eq!(session.control.snapshot().underrun_frames, 0);
            }
        }
    }

    #[test]
    fn converted_recording_queue_remains_bounded_and_stop_rejects_input() {
        for rate in [44_100, 96_000] {
            let (writer, mut session, _) = setup();
            let mut recording = RecordingWriter::new(writer, rate, 1).unwrap();
            let data = vec![0.25; rate as usize];
            recording.write(&data, |s| s);
            assert_eq!(
                session.control.snapshot().queued_frames as usize,
                QUEUE_FRAMES
            );
            assert!(session.control.snapshot().dropped_frames > 40_000);
            let mut drained = [[0.0; 2]; QUEUE_FRAMES];
            session.input.drain_frames(&mut drained);
            assert_eq!(session.control.snapshot().queued_frames, 0);
            recording.write(&[0.25; 1024], |s| s);
            assert!(session.control.snapshot().queued_frames > 0);
            assert!(session.control.stop());
            session.input.drain_frames(&mut drained);
            assert!(drained.iter().all(|frame| *frame == [0.0; 2]));
            let pushed = recording.writer.shared.pushed.load(Ordering::Acquire);
            recording.write(&data, |s| s);
            assert_eq!(
                recording.writer.shared.pushed.load(Ordering::Acquire),
                pushed
            );
        }
    }

    #[test]
    fn resampled_recording_retains_native_clipping_indication() {
        let (writer, session, _) = setup();
        let mut recording = RecordingWriter::new(writer, 96_000, 1).unwrap();
        recording.write(&[1.5; 128], |s| s);
        assert!(session.control.take_pre_clip());
    }

    #[cfg(windows)]
    #[test]
    fn cleanup_deadline_failure_remains_terminal_across_repeated_stop() {
        let (_, session, _) = setup();
        *session.control.owner.worker.lock().unwrap() = Some(std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(350));
        }));
        assert!(!session.control.stop());
        assert_eq!(session.control.snapshot().phase, CapturePhase::Error);
        assert!(!session.control.stop());
        assert_eq!(session.control.snapshot().phase, CapturePhase::Error);
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
    fn native_preclip_latches_between_telemetry_polls() {
        let (mut writer, session, _) = setup();
        feed(&mut writer, 1, 1.0, -0.5);
        feed(&mut writer, 240, 0.01, -0.01);
        assert!(session.control.snapshot().level_dbfs.unwrap() < -30.0);
        assert!(session.control.take_pre_clip());
        assert!(!session.control.take_pre_clip());
    }

    #[test]
    fn nonblocking_mix_drains_share_external_timeline_and_keep_precise_output_gain() {
        let (mut first_writer, mut first, first_clock) = setup();
        let (mut second_writer, mut second, second_clock) = setup();
        feed(&mut first_writer, 240, 0.5, -0.25);
        feed(&mut second_writer, 240, 0.125, -0.5);
        let mut a = [[0.0; 2]; 240];
        let mut b = a;
        first.input.drain_frames(&mut a);
        second.input.drain_frames(&mut b);
        assert_eq!(a, [[0.5, -0.25]; 240]);
        assert_eq!(b, [[0.125, -0.5]; 240]);
        first.input.drain_frames(&mut a);
        assert_eq!(a, [[0.0; 2]; 240]);
        assert_eq!(first_clock.now(), 0);
        assert_eq!(second_clock.now(), 0);
        first.control.set_linear_output(0.004_25, false);
        feed(&mut first_writer, 1, 0.5, -1.0);
        assert_eq!(frames(&mut first.input, 1), vec![[0.002_125, -0.004_25]]);
        first.control.set_linear_output(0.003_75, false);
        assert_eq!(first.control.snapshot().output_level_dbfs, None);
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
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(peak_dbfs(0.25))
        );
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
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        assert_eq!(session.control.snapshot().queued_frames, 960);
        assert!(
            (session.control.snapshot().level_dbfs.unwrap() - 20.0 * 0.75_f32.log10()).abs()
                < 0.001
        );
        assert_eq!(frames(&mut session.input, 960), vec![[0.0; 2]; 960]);
        assert_eq!(session.control.snapshot().queued_frames, 0);
        assert_eq!(session.control.snapshot().underrun_frames, 0);
        session.control.set_output(25, false);
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        feed(&mut writer, 2, 0.5, -0.25);
        assert_eq!(frames(&mut session.input, 2), vec![[0.125, -0.0625]; 2]);
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(peak_dbfs(0.125))
        );
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
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(peak_dbfs(0.25))
        );
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
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(SILENCE_DBFS)
        );
    }

    #[test]
    fn output_peak_observes_handoff_and_gain_without_rewriting_input_or_queue_generation() {
        let (mut writer, mut session, _) = setup();
        feed(&mut writer, 4, 0.5, -1.0);
        assert_eq!(session.control.snapshot().level_dbfs, Some(0.0));
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        let generation = writer.shared.output.load(Ordering::Acquire) >> GENERATION_SHIFT;
        for (volume, peak) in [(50, 0.5), (0, 0.0), (25, 0.25), (255, 1.0)] {
            session.control.set_output(volume, false);
            assert_eq!(session.control.snapshot().output_level_dbfs, None);
            assert_eq!(frames(&mut session.input, 1), vec![[0.5 * peak, -peak]]);
            assert_eq!(
                session.control.snapshot().output_level_dbfs,
                Some(peak_dbfs(peak))
            );
            assert_eq!(session.control.snapshot().level_dbfs, Some(0.0));
            assert_eq!(
                writer.shared.output.load(Ordering::Acquire) >> GENERATION_SHIFT,
                generation
            );
        }
        assert_eq!(session.control.snapshot().dropped_frames, 0);
    }

    #[test]
    fn output_peak_hold_expires_without_silence_refreshing_an_old_peak() {
        let (mut writer, mut session, clock) = setup();
        feed(&mut writer, 1, 1.0, -0.5);
        frames(&mut session.input, 1);
        assert_eq!(session.control.snapshot().output_level_dbfs, Some(0.0));
        assert_eq!(frames(&mut session.input, 960), vec![[0.0; 2]; 960]);
        clock.ns.store(METER_AGE_NS, Ordering::Relaxed);
        assert_eq!(session.control.snapshot().output_level_dbfs, Some(0.0));
        frames(&mut session.input, 1);
        clock.ns.fetch_add(1, Ordering::Relaxed);
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        feed(&mut writer, 1, -0.25, 0.125);
        frames(&mut session.input, 1);
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(peak_dbfs(0.25))
        );
    }

    #[test]
    fn output_meter_distinguishes_initial_opening_and_starved_silence() {
        let (writer, mut session, _) = setup();
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        assert_eq!(session.input.read(&mut []).unwrap(), 0);
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        writer.shared.status.store(OPENING, Ordering::Release);
        frames(&mut session.input, 1);
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        writer.shared.status.store(RUNNING, Ordering::Release);
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(SILENCE_DBFS)
        );
        assert_eq!(session.control.snapshot().level_dbfs, None);
    }

    #[test]
    fn rejected_stale_and_overflowed_loud_frames_do_not_reach_output_meter() {
        let (mut writer, mut session, clock) = setup();
        feed(&mut writer, 1, 1.0, -1.0);
        clock.ns.store(MAX_AGE_NS + 1, Ordering::Relaxed);
        assert_eq!(frames(&mut session.input, 1), vec![[0.0; 2]]);
        assert_eq!(session.control.snapshot().level_dbfs, Some(0.0));
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(SILENCE_DBFS)
        );
        feed(&mut writer, QUEUE_FRAMES, 0.125, -0.25);
        feed(&mut writer, 1, 1.0, -1.0);
        frames(&mut session.input, 1);
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(peak_dbfs(0.25))
        );
        assert_eq!(session.control.snapshot().dropped_frames, 2);
    }

    #[test]
    fn output_peak_requires_complete_frames_and_does_not_refresh_a_stalled_fragment() {
        let (mut writer, mut session, clock) = setup();
        feed(&mut writer, 2, 0.25, -0.5);
        session.input.read_exact(&mut [0; 7]).unwrap();
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        session.input.read_exact(&mut [0; 1]).unwrap();
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(peak_dbfs(0.5))
        );
        session.input.read_exact(&mut [0; 1]).unwrap();
        clock.ns.store(METER_AGE_NS + 1, Ordering::Relaxed);
        session.input.read_exact(&mut [0; 7]).unwrap();
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
    }

    #[test]
    fn rapid_mute_generation_and_old_partial_frames_cannot_restore_a_peak() {
        let (mut writer, mut session, _) = setup();
        feed(&mut writer, 2, 1.0, -1.0);
        frames(&mut session.input, 1);
        session.input.read_exact(&mut [0; 3]).unwrap();
        session.control.set_output(100, true);
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        session.control.set_output(100, false);
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        session.input.read_exact(&mut [0; 5]).unwrap();
        assert_eq!(session.control.snapshot().output_level_dbfs, None);
        feed(&mut writer, 1, 0.125, -0.25);
        frames(&mut session.input, 1);
        assert_eq!(
            session.control.snapshot().output_level_dbfs,
            Some(peak_dbfs(0.25))
        );
    }

    #[test]
    fn new_capture_session_has_no_peak_from_a_stopped_session() {
        let (mut writer, mut old, _) = setup();
        feed(&mut writer, 1, 1.0, -1.0);
        frames(&mut old.input, 1);
        assert_eq!(old.control.snapshot().output_level_dbfs, Some(0.0));
        old.control.stop();
        assert_eq!(old.control.snapshot().output_level_dbfs, None);
        let (_, new, _) = setup();
        assert_eq!(new.control.snapshot().output_level_dbfs, None);
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
        assert_eq!(session.control.snapshot().level_dbfs, None);
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
        assert_eq!(clone.snapshot().output_level_dbfs, None);
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
            frames(&mut session.input, 1);
            assert!(session.control.snapshot().output_level_dbfs.is_some());
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
            assert_eq!(session.control.snapshot().output_level_dbfs, None);
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
    fn capture_format_selection_prefers_48k_and_accepts_reviewed_fallbacks() {
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
            range(2, 32_000, 32_000, SampleFormat::F32),
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
        for rate in [44_100, 96_000] {
            let selected =
                windows::select_config([range(2, rate, rate, SampleFormat::F32)].into_iter())
                    .unwrap();
            assert_eq!(selected.sample_rate(), rate);
        }
        let selected = windows::select_config(
            [
                range(2, 96_000, 96_000, SampleFormat::F32),
                range(1, 44_100, 44_100, SampleFormat::I16),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(selected.sample_rate(), 44_100);
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
