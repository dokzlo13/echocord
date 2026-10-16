//! Per-capture correction. Only the mixer consumer touches DSP or diagnostics.
use super::link::{self, Direction, Link, Plan, Timing};
use super::{Frame, GENERATION_SHIFT, MUTED, OPENING, RUNNING, Shared};
use crate::model::{TimingObservation, TimingPhase, TimingResetReason as Reason};
use rtrb::Consumer;
use std::sync::atomic::Ordering;

pub(super) struct SourceLink {
    enabled: bool,
    epoch: [u64; 2],
    link: Link,
    samples: Vec<[f32; 2]>,
    origins: Vec<u64>,
    timing: Timing,
    seen_fence: Option<(u64, u64, u64)>,
    last_ns: Option<u64>,
    blocked: Option<Reason>,
}
impl SourceLink {
    pub(super) fn new() -> Result<Self, &'static str> {
        Ok(Self {
            enabled: false,
            epoch: [0, 0],
            link: Link::new(Direction::Capture, 240)?,
            samples: vec![[0.0; 2]; 512],
            origins: vec![0; 512],
            timing: Timing::new(false, [0, 0, 0]),
            seen_fence: None,
            last_ns: None,
            blocked: None,
        })
    }
    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(super) fn configure(
        &mut self,
        enabled: bool,
        lifetime: u64,
        mode: u64,
        consumer: &mut Consumer<Frame>,
        shared: &Shared,
    ) {
        if self.enabled == enabled && self.epoch == [lifetime, mode] {
            return;
        }
        shared.processing_epoch.fetch_add(1, Ordering::AcqRel);
        shared.correction_enabled.store(enabled, Ordering::Release);
        self.enabled = enabled;
        self.epoch = [lifetime, mode];
        self.link.reset(false);
        self.last_ns = None;
        self.seen_fence = None;
        self.blocked = None;
        self.timing = Timing::new(enabled, [lifetime, mode, 0]);
        self.timing.state_ms = Some(super::observation_clock_ms());
        let observed = consumer.slots().min(link::CAPACITY);
        discard(consumer, shared, observed);
    }
    pub(super) fn snapshot(&self) -> TimingObservation {
        self.timing.snapshot()
    }
    pub(super) fn legacy_observed(&mut self, queued: usize, fresh: bool) {
        self.timing.phase = TimingPhase::Off;
        self.timing.state_ms = Some(super::observation_clock_ms());
        self.timing.audio_ms = Some(super::observation_clock_ms());
        let _ = fresh;
        self.timing.queued = Some(queued.min(link::LEGACY_CAPACITY) as u32);
        self.timing.target = None;
    }
    fn reset(&mut self, reason: Reason, now_ms: f64) {
        self.link.reset(true);
        self.timing.reset(reason, now_ms);
    }
    pub(super) fn drain(
        &mut self,
        consumer: &mut Consumer<Frame>,
        shared: &Shared,
        output: &mut [[f32; 2]],
    ) -> bool {
        output.fill([0.0; 2]);
        let now = shared.clock.now();
        let observed_ms = super::observation_clock_ms();
        let fence = (
            shared.input_epoch.load(Ordering::Acquire),
            shared.processing_epoch.load(Ordering::Acquire),
            shared.output.load(Ordering::Acquire) >> GENERATION_SHIFT,
        );
        self.timing.state_ms = Some(observed_ms);
        if !matches!(shared.status.load(Ordering::Acquire), OPENING | RUNNING) {
            self.timing.audio_ms = None;
            self.timing.applied = None;
            return false;
        }
        if self
            .last_ns
            .is_some_and(|last| now < last || now - last > link::STALL_NS)
        {
            let dropped = discard(consumer, shared, consumer.slots().min(link::CAPACITY));
            self.timing.dropped = self.timing.dropped.saturating_add(dropped as u64);
            self.reset(Reason::PausedOutput, observed_ms);
        }
        self.last_ns = Some(now);
        if self.seen_fence.is_some_and(|seen| seen != fence) {
            self.reset(Reason::Discontinuity, observed_ms);
        }
        self.seen_fence = Some(fence);
        let mut dropped = 0usize;
        for _ in 0..link::CAPACITY {
            let Ok(frame) = consumer.peek() else {
                break;
            };
            if valid(frame, fence, now) {
                break;
            }
            if consumer.pop().is_err() {
                break;
            }
            shared.popped.fetch_add(1, Ordering::Release);
            dropped += 1;
        }
        if dropped > 0 {
            shared.dropped.fetch_add(dropped as u64, Ordering::Relaxed);
            self.timing.dropped = self.timing.dropped.saturating_add(dropped as u64);
            if self.link.primed() {
                self.reset(Reason::Discontinuity, observed_ms);
            }
        }
        let muted = shared.output.load(Ordering::Acquire) & MUTED != 0;
        let stalled = shared.peak_bits.load(Ordering::Acquire) != u32::MAX
            && now.saturating_sub(shared.last_callback_ns.load(Ordering::Acquire)) > link::STALL_NS;
        if muted || stalled {
            let dropped = discard(consumer, shared, consumer.slots().min(link::CAPACITY));
            self.timing.dropped = self.timing.dropped.saturating_add(dropped as u64);
            if self.blocked != Some(Reason::StalledInput) {
                self.reset(Reason::StalledInput, observed_ms);
            }
            self.blocked = Some(Reason::StalledInput);
            self.timing.queued = Some(0);
            return self.gap(consumer, shared, output.len(), !muted);
        }
        self.blocked = None;
        let queue = consumer.slots().min(link::CAPACITY);
        let clock = shared.native_clock.snapshot(now, fence.0);
        let was_running = self.link.primed();
        let plan = self
            .link
            .plan(queue, output.len(), clock.packet_frames, clock.rate, now);
        self.timing.target = Some(self.link.target() as u32);
        match plan {
            Plan::Prime => {
                if was_running {
                    self.reset(Reason::Starved, observed_ms);
                }
                self.timing.phase = TimingPhase::Priming;
                self.timing.queued = Some(queue as u32);
                self.gap(consumer, shared, output.len(), true)
            }
            Plan::Cut { frames } => {
                let dropped = discard(consumer, shared, frames.min(queue));
                self.timing.dropped = self.timing.dropped.saturating_add(dropped as u64);
                self.timing.reset(Reason::Overrun, observed_ms);
                self.timing.queued = Some(consumer.slots().min(link::CAPACITY) as u32);
                self.gap(consumer, shared, output.len(), true)
            }
            Plan::Unsupported => {
                shared.fail(super::TIMING_UNSUPPORTED);
                self.timing.audio_ms = None;
                self.timing.applied = None;
                false
            }
            Plan::Pull { input_frames } => {
                if input_frames > self.samples.len() {
                    shared.fail(super::TIMING_UNSUPPORTED);
                    return false;
                }
                let mut copied = 0;
                let mut valid_block = true;
                for index in 0..input_frames {
                    let Ok(frame) = consumer.pop() else {
                        break;
                    };
                    shared.popped.fetch_add(1, Ordering::Release);
                    copied += 1;
                    if !valid(&frame, fence, now) {
                        valid_block = false;
                        break;
                    }
                    self.samples[index] = frame.samples;
                    self.origins[index] = frame.exposed_ns;
                }
                let unchanged = fence
                    == (
                        shared.input_epoch.load(Ordering::Acquire),
                        shared.processing_epoch.load(Ordering::Acquire),
                        shared.output.load(Ordering::Acquire) >> GENERATION_SHIFT,
                    );
                if copied != input_frames || !valid_block || !unchanged {
                    shared.dropped.fetch_add(copied as u64, Ordering::Relaxed);
                    self.timing.dropped = self.timing.dropped.saturating_add(copied as u64);
                    self.reset(Reason::Discontinuity, observed_ms);
                    return self.gap(consumer, shared, output.len(), true);
                }
                let target = self.link.target().saturating_sub(input_frames);
                let processed = self.link.process_timed(
                    &self.samples[..input_frames],
                    &self.origins[..input_frames],
                    now,
                    || shared.clock.now(),
                );
                match processed {
                    Ok(processed) => {
                        let oldest = processed.oldest_ns;
                        let gain = f32::from_bits(shared.output.load(Ordering::Acquire) as u32);
                        for (out, frame) in output.iter_mut().zip(processed.frames) {
                            *out = [frame[0] * gain, frame[1] * gain];
                        }
                        let finished = shared.clock.now();
                        let current = (
                            shared.input_epoch.load(Ordering::Acquire),
                            shared.processing_epoch.load(Ordering::Acquire),
                            shared.output.load(Ordering::Acquire) >> GENERATION_SHIFT,
                        );
                        if current != fence
                            || finished.saturating_sub(oldest) > link::FRESH_NS
                            || finished.saturating_sub(now) > link::STALL_NS
                            || !matches!(shared.status.load(Ordering::Acquire), OPENING | RUNNING)
                        {
                            output.fill([0.0; 2]);
                            self.seen_fence = Some(current);
                            shared
                                .dropped
                                .fetch_add(input_frames as u64, Ordering::Relaxed);
                            self.timing.dropped =
                                self.timing.dropped.saturating_add(input_frames as u64);
                            self.reset(Reason::Discontinuity, super::observation_clock_ms());
                            return self.gap(consumer, shared, output.len(), true);
                        }
                    }
                    Err(_) => {
                        shared
                            .dropped
                            .fetch_add(input_frames as u64, Ordering::Relaxed);
                        self.timing.dropped =
                            self.timing.dropped.saturating_add(input_frames as u64);
                        self.reset(Reason::StalledInput, observed_ms);
                        return self.gap(consumer, shared, output.len(), true);
                    }
                }
                self.timing.phase = TimingPhase::Running;
                self.timing.started = true;
                self.timing.applied = self.link.applied_ppm().map(f64::from);
                self.timing.queued = Some(consumer.slots().min(link::CAPACITY) as u32);
                self.timing.target = Some(target as u32);
                self.timing.audio_ms = Some(super::observation_clock_ms());
                true
            }
        }
    }
    fn gap(
        &mut self,
        consumer: &Consumer<Frame>,
        shared: &Shared,
        frames: usize,
        count: bool,
    ) -> bool {
        let observed = super::observation_clock_ms();
        self.timing.queued = Some(consumer.slots().min(link::CAPACITY) as u32);
        self.timing.audio_ms = Some(observed);
        self.timing.state_ms = Some(observed);
        self.timing.applied = None;
        if count && self.timing.started {
            self.timing.gaps = self.timing.gaps.saturating_add(frames as u64);
            shared.underrun.fetch_add(frames as u64, Ordering::Relaxed);
        }
        false
    }
}
fn valid(frame: &Frame, fence: (u64, u64, u64), now: u64) -> bool {
    frame.input_epoch == fence.0
        && frame.processing_epoch == fence.1
        && frame.generation == fence.2
        && frame.exposed_ns <= now
        && now.saturating_sub(frame.exposed_ns) <= link::FRESH_NS
}
fn discard(consumer: &mut Consumer<Frame>, shared: &Shared, frames: usize) -> usize {
    let mut dropped = 0;
    for _ in 0..frames.min(link::CAPACITY) {
        if consumer.pop().is_err() {
            break;
        }
        dropped += 1;
    }
    shared.popped.fetch_add(dropped as u64, Ordering::Release);
    shared.dropped.fetch_add(dropped as u64, Ordering::Relaxed);
    dropped
}
