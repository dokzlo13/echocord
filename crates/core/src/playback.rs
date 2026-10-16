//! Explicit, isolated Windows playback destinations and a nonblocking bus renderer.
use crate::model::{AppError, ErrorCode, PlaybackDevice};
#[cfg(any(windows, test))]
use crate::{
    audio::link::{
        self, Direction, DriftMode, Link, Plan, RateEstimator, Timing, TimingPublication,
    },
    mix_bus::{Bus, CorrectionRead},
    model::{Output, TimingPhase, TimingResetReason},
};
use serde::{Deserialize, Serialize};
#[cfg(any(windows, test))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::SeqCst},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RenderObservation {
    #[serde(default)]
    pub observed_at_ms: Option<f64>,
    pub level_dbfs: Option<f32>,
    pub queued_frames: u32,
    pub dropped_frames: u32,
    pub underrun_frames: u32,
    pub callback_frames: u32,
    #[serde(default)]
    pub timing: Option<crate::model::TimingObservation>,
}

#[cfg(any(windows, test))]
const MUTED: u64 = 1 << 32;

/// Updated before parent mailbox coalescing, preserving every mute/mode edge.
#[cfg(any(windows, test))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct PlaybackControls {
    output: Output,
    output_epoch: u64,
    drift: DriftMode,
}
#[cfg(any(windows, test))]
impl PlaybackControls {
    fn new(output: Output, enabled: bool) -> Self {
        Self {
            output,
            output_epoch: 0,
            drift: DriftMode { enabled, epoch: 0 },
        }
    }
    fn update(&mut self, output: Output, enabled: bool) {
        if self.output.muted != output.muted {
            self.output_epoch = self.output_epoch.wrapping_add(1);
        }
        if self.drift.enabled != enabled {
            self.drift.epoch = self.drift.epoch.wrapping_add(1);
        }
        self.output = output;
        self.drift.enabled = enabled;
    }
}
#[cfg(any(windows, test))]
#[derive(Clone, Copy)]
struct RenderSettings {
    output: u64,
    output_epoch: u64,
    drift: DriftMode,
}
#[cfg(any(windows, test))]
impl RenderSettings {
    fn epoch(self, session: u32) -> [u64; 3] {
        [u64::from(session), self.output_epoch, self.drift.epoch]
    }
    fn muted(self) -> bool {
        self.output & MUTED != 0
    }
    fn gain(self) -> f32 {
        f32::from_bits(self.output as u32)
    }
}

#[cfg(any(windows, test))]
pub struct RenderControl {
    session: u32,
    sequence: AtomicU64,
    output: AtomicU64,
    output_epoch: AtomicU64,
    drift_enabled: AtomicBool,
    drift_epoch: AtomicU64,
    stopped: AtomicBool,
    failed: AtomicBool,
    correction_failed: AtomicBool,
    meter_sequence: AtomicU64,
    meter_output: AtomicU64,
    meter_output_epoch: AtomicU64,
    meter_drift_epoch: AtomicU64,
    meter_peak: AtomicU32,
    meter_ms: AtomicU64,
    callback_frames: AtomicU32,
    callback_ms: AtomicU64,
    timing: TimingPublication,
}
#[cfg(any(windows, test))]
impl Default for RenderControl {
    fn default() -> Self {
        Self::new(
            0,
            &PlaybackControls::new(
                Output {
                    volume_linear: Some(1.0),
                    volume_percent: 100,
                    muted: false,
                },
                true,
            ),
        )
    }
}
#[cfg(any(windows, test))]
impl RenderControl {
    fn new(session: u32, controls: &PlaybackControls) -> Self {
        let control = Self {
            session,
            sequence: AtomicU64::new(0),
            output: AtomicU64::new(0),
            output_epoch: AtomicU64::new(0),
            drift_enabled: AtomicBool::new(true),
            drift_epoch: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            correction_failed: AtomicBool::new(false),
            meter_sequence: AtomicU64::new(0),
            meter_output: AtomicU64::new(0),
            meter_output_epoch: AtomicU64::new(0),
            meter_drift_epoch: AtomicU64::new(0),
            meter_peak: AtomicU32::new(0),
            meter_ms: AtomicU64::new(u64::MAX),
            callback_frames: AtomicU32::new(0),
            callback_ms: AtomicU64::new(0),
            timing: TimingPublication::default(),
        };
        control.apply(controls);
        control
    }
    /// One command-thread writer. Callback reads make one attempt, never spin.
    fn apply(&self, controls: &PlaybackControls) {
        let gain = controls
            .output
            .volume_linear
            .unwrap_or(controls.output.volume_percent as f32 / 100.0);
        let gain = if gain.is_finite() {
            gain.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let output = u64::from(gain.to_bits()) | if controls.output.muted { MUTED } else { 0 };
        self.sequence.fetch_add(1, SeqCst);
        self.output.store(output, SeqCst);
        self.output_epoch.store(controls.output_epoch, SeqCst);
        self.drift_enabled.store(controls.drift.enabled, SeqCst);
        self.drift_epoch.store(controls.drift.epoch, SeqCst);
        self.sequence.fetch_add(1, SeqCst);
    }
    fn settings(&self) -> Option<RenderSettings> {
        let sequence = self.sequence.load(SeqCst);
        let settings = RenderSettings {
            output: self.output.load(SeqCst),
            output_epoch: self.output_epoch.load(SeqCst),
            drift: DriftMode {
                enabled: self.drift_enabled.load(SeqCst),
                epoch: self.drift_epoch.load(SeqCst),
            },
        };
        (sequence.is_multiple_of(2) && sequence == self.sequence.load(SeqCst)).then_some(settings)
    }
    #[cfg(test)]
    pub fn set_output(&self, gain: f32, muted: bool) {
        let before = self.settings().unwrap();
        let mut controls = PlaybackControls {
            output: Output {
                volume_linear: Some(before.gain()),
                volume_percent: 100,
                muted: before.muted(),
            },
            output_epoch: before.output_epoch,
            drift: before.drift,
        };
        controls.update(
            Output {
                volume_linear: Some(gain),
                volume_percent: 100,
                muted,
            },
            before.drift.enabled,
        );
        self.apply(&controls);
    }
    #[cfg(test)]
    fn set_drift(&self, enabled: bool) {
        let before = self.settings().unwrap();
        let output = Output {
            volume_linear: Some(before.gain()),
            volume_percent: 100,
            muted: before.muted(),
        };
        let mut controls = PlaybackControls {
            output: output.clone(),
            output_epoch: before.output_epoch,
            drift: before.drift,
        };
        controls.update(output, enabled);
        self.apply(&controls);
    }
    pub fn stop(&self) {
        self.stopped.store(true, SeqCst);
    }
    pub fn snapshot(&self, now_ms: u64) -> RenderObservation {
        let Some(settings) = self.settings() else {
            return RenderObservation::default();
        };
        let epoch = settings.epoch(self.session);
        let timing = self
            .timing
            .snapshot()
            .filter(|v| v.epoch == format!("{}:{}:{}", epoch[0], epoch[1], epoch[2]));
        let sequence = self.meter_sequence.load(SeqCst);
        let peak = f32::from_bits(self.meter_peak.load(SeqCst));
        let measured = self.meter_ms.load(SeqCst);
        let valid = sequence != 0
            && sequence.is_multiple_of(2)
            && self.meter_output.load(SeqCst) == settings.output
            && self.meter_output_epoch.load(SeqCst) == settings.output_epoch
            && self.meter_drift_epoch.load(SeqCst) == settings.drift.epoch
            && sequence == self.meter_sequence.load(SeqCst)
            && !self.stopped.load(SeqCst)
            && !self.failed.load(SeqCst)
            && measured != u64::MAX
            && measured <= now_ms.saturating_add(1)
            && now_ms.saturating_sub(measured) <= 200;
        let observed_at_ms =
            crate::audio::clean_observation_time(valid.then_some(measured as f64), now_ms as f64);
        RenderObservation {
            observed_at_ms,
            level_dbfs: observed_at_ms.map(|_| {
                if peak > 0.0 {
                    (20.0 * peak.log10()).max(-96.0)
                } else {
                    -96.0
                }
            }),
            queued_frames: timing.as_ref().and_then(|v| v.queued_frames).unwrap_or(0),
            dropped_frames: timing.as_ref().map_or(0, |v| {
                v.dropped_input_frames.min(u64::from(u32::MAX)) as u32
            }),
            underrun_frames: timing
                .as_ref()
                .map_or(0, |v| v.gap_output_frames.min(u64::from(u32::MAX)) as u32),
            callback_frames: self.callback_frames.load(SeqCst),
            timing,
        }
    }
}

/// A bounded failure window also catches alternating unsupported callbacks.
#[cfg(any(windows, test))]
#[derive(Default)]
struct UnsupportedCallbacks {
    recent: [bool; 32],
    next: usize,
    consecutive: usize,
}
#[cfg(any(windows, test))]
impl UnsupportedCallbacks {
    fn observe(&mut self, unsupported: bool) -> bool {
        self.recent[self.next] = unsupported;
        self.next = (self.next + 1) % self.recent.len();
        self.consecutive = if unsupported { self.consecutive + 1 } else { 0 };
        self.consecutive >= 3 || self.recent.iter().filter(|v| **v).count() >= 8
    }
}

#[cfg(any(windows, test))]
pub struct Renderer {
    cursor: u64,
    continuity: u64,
    pub control: Arc<RenderControl>,
    link: Link,
    input: Vec<[f32; 2]>,
    origins: Vec<u64>,
    timing: Timing,
    clock: RateEstimator,
    native_position: u64,
    last_callback_ns: Option<u64>,
    unsupported: UnsupportedCallbacks,
    held_peak: f32,
    held_ms: u64,
    held_output: u64,
}
#[cfg(any(windows, test))]
impl Renderer {
    pub fn attach(bus: &Bus, control: Arc<RenderControl>) -> Result<Self, &'static str> {
        let settings = control.settings().ok_or("Playback controls are changing")?;
        let cursor = bus.cursor();
        Ok(Self {
            cursor,
            continuity: bus.correction_window(cursor, 0).continuity,
            timing: Timing::new(settings.drift.enabled, settings.epoch(control.session)),
            control,
            link: Link::new(Direction::Playback, link::CAPACITY)?,
            input: vec![[0.0; 2]; link::CAPACITY],
            origins: vec![0; link::CAPACITY],
            clock: RateEstimator::default(),
            native_position: 0,
            last_callback_ns: None,
            unsupported: UnsupportedCallbacks::default(),
            held_peak: 0.0,
            held_ms: 0,
            held_output: 0,
        })
    }
    #[cfg(test)]
    pub fn render<T: Copy>(
        &mut self,
        bus: &Bus,
        data: &mut [T],
        now_ms: u64,
        convert: impl Fn(f32) -> T,
    ) {
        self.render_clocked(
            bus,
            data,
            now_ms * 1_000_000,
            None,
            48_000,
            || now_ms * 1_000_000,
            convert,
        );
    }
    fn reset_intentional(&mut self, bus: &Bus, settings: RenderSettings) {
        self.cursor = bus.cursor();
        self.continuity = bus.correction_window(self.cursor, 0).continuity;
        self.link.reset(false);
        self.clock.reset();
        self.timing = Timing::new(settings.drift.enabled, settings.epoch(self.control.session));
        self.unsupported = UnsupportedCallbacks::default();
        self.held_peak = 0.0;
    }
    fn reset_unexpected(&mut self, reason: TimingResetReason, now_ms: u64) {
        self.link.reset(true);
        self.record_reset(reason, now_ms);
    }
    fn record_reset(&mut self, reason: TimingResetReason, now_ms: u64) {
        if self.timing.phase == TimingPhase::Running {
            self.timing.reset(reason, now_ms as f64);
        } else {
            self.timing.phase = TimingPhase::Priming;
            self.timing.applied = None;
            self.timing.audio_ms = None;
        }
    }
    fn discard_to(&mut self, cursor: u64) {
        self.timing.dropped = self
            .timing
            .dropped
            .saturating_add(cursor.saturating_sub(self.cursor));
        self.cursor = cursor;
    }
    fn gap(&mut self, frames: usize) {
        if self.timing.started {
            self.timing.gaps = self.timing.gaps.saturating_add(frames as u64);
        }
    }
    fn unsupported(&mut self, bus: &Bus, frames: usize, now_ms: u64) {
        self.discard_to(bus.cursor());
        self.reset_unexpected(TimingResetReason::PausedOutput, now_ms);
        self.clock.reset();
        self.gap(frames);
        if self.unsupported.observe(true) {
            self.control.correction_failed.store(true, SeqCst);
            self.control.failed.store(true, SeqCst);
        }
    }
    /// One admission/DLL observation per complete endpoint callback. All scratch
    /// and filters are constructed before entering this no-allocation path.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_clocked<T: Copy>(
        &mut self,
        bus: &Bus,
        data: &mut [T],
        started_ns: u64,
        playback_ns: Option<u64>,
        native_rate: u32,
        mut completed: impl FnMut() -> u64,
        convert: impl Fn(f32) -> T,
    ) {
        if data.is_empty() {
            return;
        }
        data.fill(convert(0.0));
        let frames = data.len() / 2;
        let position = self.native_position;
        self.native_position = self.native_position.saturating_add(frames as u64);
        let previous_callback = self.last_callback_ns.replace(started_ns);
        let now_ms = started_ns / 1_000_000;
        self.control
            .callback_frames
            .fetch_max(frames.min(u32::MAX as usize) as u32, SeqCst);
        self.control.callback_ms.store(now_ms, SeqCst);
        let Some(settings) = self.control.settings() else {
            return;
        };
        if self.control.stopped.load(SeqCst) || self.control.failed.load(SeqCst) {
            return;
        }
        let intentional_reset = settings.epoch(self.control.session) != self.timing.epoch;
        if intentional_reset {
            self.reset_intentional(bus, settings);
        }
        self.timing.state_ms = Some(now_ms as f64);
        self.timing.audio_ms = None;
        self.timing.applied = None;
        self.timing.queued = Some(0);
        self.timing.target = None;
        let mut audio = false;
        let mut consumed = 0_u64;
        let mut admitted_output = 0_u64;
        let mut peak = 0.0_f32;
        let mut origin = None;
        let mut epoch = self.continuity;
        if settings.muted() {
            self.cursor = bus.cursor();
            self.link.reset(true);
            audio = true; // The device receives intentional measured silence.
        } else if !settings.drift.enabled {
            self.unsupported.observe(false);
            let mut scratch = [[0.0; 2]; 240];
            for chunk in data.chunks_mut(480) {
                let count = chunk.len() / 2;
                let stats = bus.read_counted(&mut self.cursor, &mut scratch[..count], now_ms);
                self.timing.queued = Some(stats.queued_frames);
                self.timing.dropped = self
                    .timing
                    .dropped
                    .saturating_add(u64::from(stats.dropped_frames));
                if !intentional_reset {
                    self.timing.gaps = self
                        .timing
                        .gaps
                        .saturating_add(u64::from(stats.underrun_frames));
                }
                let admitted = count.saturating_sub(stats.underrun_frames as usize) as u64;
                admitted_output = admitted_output.saturating_add(admitted);
                consumed = consumed.saturating_add(admitted);
                audio |= admitted != 0;
                for (out, frame) in chunk
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(&scratch[..count])
                {
                    for channel in 0..2 {
                        let value = frame[channel] * settings.gain();
                        out[channel] = convert(value);
                        peak = peak.max(value.abs());
                    }
                }
            }
        } else if frames == 0
            || frames > 1440
            || frames > self.input.len()
            || !data.len().is_multiple_of(2)
        {
            self.unsupported(bus, frames, now_ms);
        } else {
            let window = bus.correction_window(self.cursor, now_ms);
            epoch = window.continuity;
            if !window.alive {
                self.discard_to(window.latest);
                self.reset_unexpected(TimingResetReason::StalledInput, now_ms);
                self.clock.reset();
                self.gap(frames);
                self.unsupported.observe(false);
            } else if previous_callback
                .is_some_and(|last| started_ns.saturating_sub(last) > link::STALL_NS)
            {
                self.discard_to(window.latest);
                self.reset_unexpected(TimingResetReason::PausedOutput, now_ms);
                self.clock.reset();
                self.gap(frames);
                self.unsupported.observe(false);
            } else {
                let oldest = window.latest.saturating_sub(window.available as u64);
                if self.continuity != window.continuity || self.cursor < oldest {
                    let reason = if self.continuity != window.continuity {
                        TimingResetReason::Discontinuity
                    } else {
                        TimingResetReason::Overrun
                    };
                    self.discard_to(oldest);
                    self.reset_unexpected(reason, now_ms);
                }
                self.continuity = window.continuity;
                if playback_ns.is_none_or(|v| v == 0) {
                    self.clock.reset();
                }
                let rate = playback_ns.filter(|v| *v != 0).and_then(|timestamp| {
                    if previous_callback.is_some_and(|previous| {
                        started_ns.saturating_sub(previous) < 1_000_000
                            || started_ns - previous > link::STALL_NS
                    }) {
                        self.clock.reanchor(timestamp, position);
                        None
                    } else {
                        self.clock.observe(timestamp, position, native_rate)
                    }
                });
                let plan = self
                    .link
                    .plan(window.available, frames, 240, rate, started_ns);
                self.timing.target = Some(self.link.target().min(link::CAPACITY) as u32);
                self.timing.queued = Some(window.available as u32);
                match plan {
                    Plan::Unsupported => self.unsupported(bus, frames, now_ms),
                    Plan::Cut { frames: cut } => {
                        self.unsupported.observe(false);
                        self.discard_to(self.cursor.saturating_add(cut as u64).min(window.latest));
                        self.record_reset(TimingResetReason::Overrun, now_ms);
                        self.timing.queued = Some(window.latest.saturating_sub(self.cursor) as u32);
                        self.gap(frames);
                    }
                    Plan::Prime => {
                        self.unsupported.observe(false);
                        if self.timing.phase == TimingPhase::Running {
                            self.reset_unexpected(TimingResetReason::Starved, now_ms);
                        }
                        self.gap(frames);
                    }
                    Plan::Pull { input_frames } if input_frames <= self.input.len() => {
                        self.unsupported.observe(false);
                        let read = if input_frames == 0 {
                            CorrectionRead {
                                queued_frames: window.available as u32,
                                continuity: window.continuity,
                                ..CorrectionRead::default()
                            }
                        } else {
                            bus.read_correction_with_origins(
                                &mut self.cursor,
                                &mut self.input[..input_frames],
                                &mut self.origins[..input_frames],
                                now_ms,
                            )
                        };
                        self.timing.dropped = self
                            .timing
                            .dropped
                            .saturating_add(u64::from(read.dropped_frames));
                        self.timing.queued = Some(read.queued_frames);
                        self.timing.target =
                            Some(self.link.target().saturating_sub(input_frames) as u32);
                        let after_read = completed();
                        if read.valid_frames != input_frames
                            || read.dropped_frames != 0
                            || after_read < started_ns
                            || after_read.saturating_sub(started_ns) > link::STALL_NS
                        {
                            self.timing.dropped =
                                self.timing.dropped.saturating_add(read.valid_frames as u64);
                            self.reset_unexpected(TimingResetReason::PausedOutput, now_ms);
                            self.gap(frames);
                        } else {
                            match self.link.process_timed(
                                &self.input[..input_frames],
                                &self.origins[..input_frames],
                                started_ns,
                                &mut completed,
                            ) {
                                Ok(processed) => {
                                    origin = Some(processed.oldest_ns / 1_000_000);
                                    epoch = read.continuity;
                                    for (out, frame) in
                                        data.as_chunks_mut::<2>().0.iter_mut().zip(processed.frames)
                                    {
                                        for channel in 0..2 {
                                            let value =
                                                (frame[channel] * settings.gain()).clamp(-1.0, 1.0);
                                            out[channel] = convert(value);
                                            peak = peak.max(value.abs());
                                        }
                                    }
                                    consumed = input_frames as u64;
                                    admitted_output = frames as u64;
                                    self.timing.phase = TimingPhase::Running;
                                    self.timing.applied = self.link.applied_ppm().map(f64::from);
                                    audio = true;
                                }
                                Err(_) => {
                                    self.timing.dropped =
                                        self.timing.dropped.saturating_add(input_frames as u64);
                                    self.reset_unexpected(TimingResetReason::PausedOutput, now_ms);
                                    self.gap(frames);
                                }
                            }
                        }
                    }
                    Plan::Pull { .. } => self.unsupported(bus, frames, now_ms),
                }
            }
        }
        let final_ns = completed();
        let controls_changed = self
            .control
            .settings()
            .is_none_or(|v| v.epoch(self.control.session) != settings.epoch(self.control.session));
        let expired = final_ns < started_ns
            || final_ns.saturating_sub(started_ns) > link::STALL_NS
            || origin.is_some_and(|oldest| {
                final_ns.saturating_sub(oldest.saturating_mul(1_000_000)) > link::FRESH_NS
                    || !bus.correction_lease_valid(epoch, oldest, final_ns / 1_000_000)
            });
        if controls_changed
            || self.control.stopped.load(SeqCst)
            || self.control.failed.load(SeqCst)
            || expired
        {
            data.fill(convert(0.0));
            peak = 0.0;
            if expired && !settings.muted() && !controls_changed {
                self.timing.dropped = self.timing.dropped.saturating_add(consumed);
                self.discard_to(bus.cursor());
                if settings.drift.enabled {
                    self.reset_unexpected(TimingResetReason::PausedOutput, final_ns / 1_000_000);
                    if audio {
                        self.gap(frames);
                    }
                } else {
                    self.timing.gaps = self.timing.gaps.saturating_add(admitted_output);
                }
            }
            audio = false;
        }
        let final_ms = final_ns / 1_000_000;
        self.timing.state_ms = Some(final_ms as f64);
        // These are actual callback queue/counter observations even when no
        // PCM was admitted. Amplitude freshness is published separately below.
        let available = bus.correction_window(self.cursor, final_ms).available;
        self.timing.queued = Some(available.min(if settings.drift.enabled {
            link::CAPACITY
        } else {
            link::LEGACY_CAPACITY
        }) as u32);
        self.timing.audio_ms = (!controls_changed).then_some(final_ms as f64);
        if audio && !settings.muted() {
            self.timing.started = true;
        }
        if !audio {
            self.timing.applied = None;
        }
        if settings.output != self.held_output || final_ms.saturating_sub(self.held_ms) >= 100 {
            self.held_peak = peak;
            self.held_ms = final_ms;
            self.held_output = settings.output;
        } else {
            self.held_peak = self.held_peak.max(peak);
        }
        self.control.meter_sequence.fetch_add(1, SeqCst);
        self.control
            .meter_peak
            .store(self.held_peak.to_bits(), SeqCst);
        self.control.meter_output.store(settings.output, SeqCst);
        self.control
            .meter_output_epoch
            .store(settings.output_epoch, SeqCst);
        self.control
            .meter_drift_epoch
            .store(settings.drift.epoch, SeqCst);
        self.control
            .meter_ms
            .store(if audio { final_ms } else { u64::MAX }, SeqCst);
        self.control.meter_sequence.fetch_add(1, SeqCst);
        self.control.timing.publish(&self.timing);
    }
}

fn unavailable(message: &str) -> AppError {
    AppError::new(ErrorCode::ConnectionFailed, message)
}
#[cfg(not(windows))]
pub fn enumerate_devices() -> Result<Vec<PlaybackDevice>, AppError> {
    Err(unavailable("Playback endpoints require Windows."))
}
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::*;
#[cfg(test)]
mod tests {
    use super::*;
    fn legacy_control() -> Arc<RenderControl> {
        let control = Arc::new(RenderControl::default());
        control.set_drift(false);
        control
    }
    #[test]
    fn callback_timestamp_is_not_refreshed_by_status_polling() {
        let bus = Bus::test_bus();
        let control = legacy_control();
        control.set_output(1.0, false);
        let mut renderer = Renderer::attach(&bus, control.clone()).unwrap();
        bus.write(&[[0.25, -0.5]; 240], 100);
        renderer.render(&bus, &mut [0.0; 480], 100, |sample| sample);
        assert_eq!(control.snapshot(150).observed_at_ms, Some(100.0));
        assert_eq!(control.snapshot(200).observed_at_ms, Some(100.0));
        assert_eq!(control.snapshot(301).observed_at_ms, None);
        assert_eq!(control.snapshot(301).level_dbfs, None);
        bus.write(&[[0.0; 2]; 240], 302);
        renderer.render(&bus, &mut [0.0; 480], 302, |sample| sample);
        assert_eq!(control.snapshot(302).observed_at_ms, Some(302.0));
        assert_eq!(control.snapshot(302).level_dbfs, Some(-96.0));
    }
    #[test]
    fn independent_callbacks_apply_precise_gain_mute_and_do_not_replay_at_attach() {
        let bus = Bus::test_bus();
        bus.write(&[[1.0, -1.0]; 240], 1);
        let a = legacy_control();
        a.set_output(0.125, false);
        let b = legacy_control();
        b.set_output(0.5, false);
        let mut first = Renderer::attach(&bus, a.clone()).unwrap();
        let mut second = Renderer::attach(&bus, b.clone()).unwrap();
        let mut out = [9.0; 480];
        first.render(&bus, &mut out, 1, |x| x);
        assert_eq!(out, [0.0; 480]);
        bus.write(&[[0.5, -0.25]; 240], 2);
        first.render(&bus, &mut out, 2, |x| x);
        assert_eq!(&out[..4], &[0.0625, -0.03125, 0.0625, -0.03125]);
        second.render(&bus, &mut out, 2, |x| x);
        assert_eq!(&out[..2], &[0.25, -0.125]);
        a.set_output(0.00425, false);
        assert_eq!(a.snapshot(2).level_dbfs, None);
        bus.write(&[[1.0, -1.0]; 240], 3);
        first.render(&bus, &mut out, 3, |x| x);
        assert_eq!(&out[..2], &[0.00425, -0.00425]);
        a.set_output(1.0, true);
        bus.write(&[[1.0, -1.0]; 240], 4);
        first.render(&bus, &mut out, 4, |x| x);
        assert_eq!(out, [0.0; 480]);
        assert_eq!(a.snapshot(4).level_dbfs, Some(-96.0));
        assert!(b.snapshot(4).level_dbfs.unwrap() > -13.0);
    }
    #[test]
    fn mute_unmute_between_callbacks_discards_pre_mute_audio() {
        let bus = Bus::test_bus();
        let control = legacy_control();
        control.set_output(1.0, false);
        let mut renderer = Renderer::attach(&bus, control.clone()).unwrap();
        bus.write(&[[1.0, -1.0]; 240], 1);
        control.set_output(1.0, true);
        control.set_output(1.0, false);
        let mut out = [9.0; 480];
        renderer.render(&bus, &mut out, 2, |x| x);
        assert_eq!(out, [0.0; 480]);
        bus.write(&[[0.25, -0.5]; 240], 3);
        renderer.render(&bus, &mut out, 3, |x| x);
        assert_eq!(&out[..2], &[0.25, -0.5]);
    }

    #[test]
    fn stalled_output_loses_only_its_backlog_and_stale_producer_is_silent() {
        let bus = Bus::test_bus();
        let slow = legacy_control();
        slow.set_output(1.0, false);
        let fast = legacy_control();
        fast.set_output(1.0, false);
        let mut first = Renderer::attach(&bus, slow.clone()).unwrap();
        let mut second = Renderer::attach(&bus, fast.clone()).unwrap();
        let mut out = [0.0; 480];
        for tick in 0..20 {
            bus.write(&[[0.5, -0.5]; 240], tick);
            second.render(&bus, &mut out, tick, |x| x);
        }
        first.render(&bus, &mut out, 20, |x| x);
        assert!(slow.snapshot(20).dropped_frames > 0);
        assert_eq!(fast.snapshot(20).dropped_frames, 0);
        first.render(&bus, &mut out, 200, |x| x);
        assert_eq!(out, [0.0; 480]);
        slow.stop();
        bus.write(&[[1.0, -1.0]; 240], 201);
        first.render(&bus, &mut out, 201, |x| x);
        assert_eq!(out, [0.0; 480]);
        assert_eq!(slow.snapshot(201).level_dbfs, None);
    }
}

#[cfg(test)]
#[path = "playback/correction_tests.rs"]
mod correction_tests;
