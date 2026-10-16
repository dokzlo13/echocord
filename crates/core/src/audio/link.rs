//! Bounded clock estimation and continuous-phase correction for native audio links.
//! Device timestamps and frame boundaries are observed once per native callback.
use rubato::{
    Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

pub(crate) const CAPACITY: usize = 4096;
pub(crate) const LEGACY_CAPACITY: usize = 1920;
pub(crate) const FRESH_NS: u64 = 80_000_000;
pub(crate) const STALL_NS: u64 = 40_000_000;
const FS: f64 = 48_000.0;
const MAX_CORRECTION: f64 = 0.001;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DriftMode {
    pub enabled: bool,
    pub epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Capture,
    Playback,
}

/// A second-order delay-locked loop over cumulative native-frame boundaries.
/// The timestamp belongs to the first frame of the packet. The *previous*
/// packet's frame count therefore determines this observation's nominal span.
/// Counts are normalized by native sample rate before nominal format conversion.
#[derive(Clone, Debug)]
pub(crate) struct RateEstimator {
    anchor_ns: Option<u64>,
    previous_ns: u64,
    position: u64,
    phase_s: f64,
    period: f64,
    valid: bool,
}
impl Default for RateEstimator {
    fn default() -> Self {
        Self {
            anchor_ns: None,
            previous_ns: 0,
            position: 0,
            phase_s: 0.0,
            period: 1.0,
            valid: false,
        }
    }
}
impl RateEstimator {
    pub(crate) fn reanchor(&mut self, timestamp_ns: u64, position: u64) {
        self.anchor_ns = Some(timestamp_ns);
        self.previous_ns = timestamp_ns;
        self.position = position;
        self.phase_s = 0.0;
    }
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn rate(&self) -> Option<f64> {
        self.valid.then_some(1.0 / self.period)
    }
    pub(crate) fn observe(
        &mut self,
        timestamp_ns: u64,
        position: u64,
        native_rate: u32,
    ) -> Option<f64> {
        if native_rate == 0 {
            return None;
        }
        let Some(anchor) = self.anchor_ns else {
            self.reanchor(timestamp_ns, position);
            return None;
        };
        let frames = position.checked_sub(self.position);
        let elapsed = timestamp_ns.checked_sub(self.previous_ns);
        let (Some(frames), Some(elapsed)) = (frames, elapsed) else {
            self.reanchor(timestamp_ns, position);
            return None;
        };
        if frames == 0 || elapsed == 0 || elapsed > FRESH_NS {
            self.reanchor(timestamp_ns, position);
            return None;
        }
        let dt = frames as f64 / f64::from(native_rate);
        if dt > FRESH_NS as f64 / 1e9 {
            self.reanchor(timestamp_ns, position);
            return None;
        }
        // Subtract the integer origin before conversion; long QPC uptime cannot
        // erase sub-millisecond precision in the loop's phase error.
        let now = timestamp_ns.saturating_sub(anchor) as f64 / 1e9;
        let predicted = self.phase_s + dt * self.period;
        let error = now - predicted;
        if error.abs() > STALL_NS as f64 / 1e9 {
            self.reanchor(timestamp_ns, position);
            return None;
        }
        let w = std::f64::consts::TAU * 0.05 * dt;
        self.phase_s = predicted + 2.0_f64.sqrt() * w * error;
        let period = self.period + w * w * error / dt;
        if !period.is_finite() || !(0.95..=1.05).contains(&period) {
            self.reanchor(timestamp_ns, position);
            return None;
        }
        self.period = period;
        self.previous_ns = timestamp_ns;
        self.position = position;
        self.valid = true;
        self.rate()
    }
}

/// One callback writer, bounded single-attempt snapshots on the mixer thread.
#[derive(Default)]
pub(crate) struct ClockPublication {
    sequence: std::sync::atomic::AtomicU64,
    rate_bits: std::sync::atomic::AtomicU64,
    packet: std::sync::atomic::AtomicU64,
    observed_ns: std::sync::atomic::AtomicU64,
    epoch: std::sync::atomic::AtomicU64,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClockObservation {
    pub rate: Option<f64>,
    pub packet_frames: usize,
}
impl ClockPublication {
    pub(crate) fn publish(&self, rate: Option<f64>, packet: usize, observed_ns: u64, epoch: u64) {
        use std::sync::atomic::Ordering::SeqCst;
        self.sequence.fetch_add(1, SeqCst);
        self.rate_bits
            .store(rate.unwrap_or(f64::NAN).to_bits(), SeqCst);
        self.packet.store(packet.min(CAPACITY) as u64, SeqCst);
        self.observed_ns.store(observed_ns, SeqCst);
        self.epoch.store(epoch, SeqCst);
        self.sequence.fetch_add(1, SeqCst);
    }
    pub(crate) fn snapshot(&self, now_ns: u64, epoch: u64) -> ClockObservation {
        use std::sync::atomic::Ordering::SeqCst;
        let sequence = self.sequence.load(SeqCst);
        let rate = f64::from_bits(self.rate_bits.load(SeqCst));
        let packet = self.packet.load(SeqCst) as usize;
        let observed = self.observed_ns.load(SeqCst);
        let observed_epoch = self.epoch.load(SeqCst);
        let valid = sequence != 0
            && sequence.is_multiple_of(2)
            && sequence == self.sequence.load(SeqCst)
            && epoch == observed_epoch
            && now_ns.saturating_sub(observed) <= STALL_NS;
        ClockObservation {
            rate: (valid && rate.is_finite() && (0.95..=1.05).contains(&rate)).then_some(rate),
            packet_frames: if valid {
                packet.clamp(1, CAPACITY)
            } else {
                240
            },
        }
    }
}

/// Fixed scalar envelope over recent native callback sizes. Alternating small
/// and large packets must not shrink the reserve immediately before a long gap.
#[derive(Clone, Debug, Default)]
pub(crate) struct PacketSizes {
    recent: [usize; 16],
    next: usize,
}
impl PacketSizes {
    pub(crate) fn observe(&mut self, native_frames: usize, native_rate: u32) -> usize {
        if native_rate == 0 {
            return self.maximum();
        }
        let frames = (native_frames as u128 * 48_000)
            .div_ceil(u128::from(native_rate))
            .min(CAPACITY as u128) as usize;
        self.recent[self.next] = frames;
        self.next = (self.next + 1) % self.recent.len();
        self.maximum()
    }
    pub(crate) fn maximum(&self) -> usize {
        self.recent.iter().copied().max().unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Plan {
    Prime,
    Pull {
        input_frames: usize,
    },
    /// Filter history is already cleared. Caller drops only this observed
    /// prefix, counts it, and emits a recovery gap before the next plan.
    Cut {
        frames: usize,
    },
    Unsupported,
}

/// Uniform origins are valid only when every input frame genuinely shares
/// that exposure (for example one timestamped capture-converter block). Bus
/// reads normally span several writes and must supply their per-frame slice.
#[derive(Clone, Copy)]
pub(crate) enum InputOrigins<'a> {
    Uniform(Option<u64>),
    Frames(&'a [u64]),
}
impl From<Option<u64>> for InputOrigins<'_> {
    fn from(origin: Option<u64>) -> Self {
        Self::Uniform(origin)
    }
}
impl<'a> From<&'a [u64]> for InputOrigins<'a> {
    fn from(origins: &'a [u64]) -> Self {
        Self::Frames(origins)
    }
}
impl InputOrigins<'_> {
    fn valid(self, frames: usize, now: u64) -> bool {
        match self {
            Self::Uniform(origin) => origin.map_or(frames == 0, |value| value <= now),
            Self::Frames(origins) => {
                origins.len() == frames && origins.iter().all(|origin| *origin <= now)
            }
        }
    }
    fn oldest(self) -> Option<u64> {
        match self {
            Self::Uniform(value) => value,
            Self::Frames(values) => values.iter().copied().min(),
        }
    }
    fn at(self, index: usize) -> Option<u64> {
        match self {
            Self::Uniform(value) => value,
            Self::Frames(values) => values.get(index).copied(),
        }
    }
}

const HISTORY_FRAMES: usize = 260; // conservative two complete 128-tap windows + phase neighbours

#[derive(Clone)]
struct HistoryLease {
    origins: [Option<u64>; HISTORY_FRAMES],
    next: usize,
}
impl Default for HistoryLease {
    fn default() -> Self {
        Self {
            origins: [None; HISTORY_FRAMES],
            next: 0,
        }
    }
}
impl HistoryLease {
    fn oldest(&self) -> Option<u64> {
        self.origins.iter().flatten().copied().min()
    }
    fn append(&mut self, frames: usize, origins: InputOrigins<'_>) {
        if frames >= HISTORY_FRAMES {
            for (index, stored) in self.origins.iter_mut().enumerate() {
                *stored = origins.at(frames - HISTORY_FRAMES + index);
            }
            self.next = 0;
        } else {
            for index in 0..frames {
                self.origins[self.next] = origins.at(index);
                self.next = (self.next + 1) % HISTORY_FRAMES;
            }
        }
    }
}
pub(crate) struct Processed<'a> {
    pub frames: &'a [[f32; 2]],
    pub oldest_ns: u64,
}

pub(crate) struct Link {
    direction: Direction,
    resampler: SincFixedOut<f32>,
    input: [Vec<f32>; 2],
    output: [Vec<f32>; 2],
    interleaved: Vec<[f32; 2]>,
    primed: bool,
    filtered: f64,
    integral: f64,
    consumption: f64,
    target: usize,
    excess_ns: u64,
    last_ns: Option<u64>,
    applied_ppm: Option<f32>,
    history: HistoryLease,
}
impl Link {
    pub(crate) fn new(direction: Direction, maximum_output: usize) -> Result<Self, &'static str> {
        if maximum_output == 0 || maximum_output > CAPACITY {
            return Err("Invalid bounded correction block size");
        }
        let resampler = SincFixedOut::new(
            1.0,
            1.004,
            SincInterpolationParameters {
                sinc_len: 128,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 256,
                window: WindowFunction::BlackmanHarris2,
            },
            maximum_output,
            2,
        )
        .map_err(|_| "Cannot initialize clock correction")?;
        let maximum_input = resampler.input_frames_max();
        Ok(Self {
            direction,
            resampler,
            input: std::array::from_fn(|_| vec![0.0; maximum_input]),
            output: std::array::from_fn(|_| vec![0.0; maximum_output]),
            interleaved: vec![[0.0; 2]; maximum_output],
            primed: false,
            filtered: 0.0,
            integral: 0.0,
            consumption: 1.0,
            target: 0,
            excess_ns: 0,
            last_ns: None,
            applied_ppm: None,
            history: HistoryLease::default(),
        })
    }
    pub(crate) fn reset(&mut self, keep_rate: bool) {
        self.resampler.reset();
        self.primed = false;
        self.filtered = 0.0;
        self.excess_ns = 0;
        self.last_ns = None;
        self.applied_ppm = None;
        self.history.origins.fill(None);
        self.history.next = 0;
        self.interleaved.fill([0.0; 2]);
        if !keep_rate {
            self.integral = 0.0;
            self.consumption = 1.0;
        }
    }
    pub(crate) fn target(&self) -> usize {
        self.target
    }
    pub(crate) fn primed(&self) -> bool {
        self.primed
    }
    pub(crate) fn applied_ppm(&self) -> Option<f32> {
        self.applied_ppm
    }
    /// One plan per actual render callback, or per 240-frame source drain.
    /// `device_rate` is native frames per nominal frame, not a UI clock fact.
    pub(crate) fn plan(
        &mut self,
        queued: usize,
        output_frames: usize,
        producer_packet: usize,
        device_rate: Option<f64>,
        now_ns: u64,
    ) -> Plan {
        if output_frames == 0 || output_frames > self.interleaved.len() || queued > CAPACITY {
            return Plan::Unsupported;
        }
        if self
            .last_ns
            .is_some_and(|last| now_ns.saturating_sub(last) > STALL_NS)
        {
            self.reset(true);
            self.last_ns = Some(now_ns);
            // A resumed consumer must attach to fresh input; an 80 ms sample
            // lease is not permission to replay its pre-pause queue.
            return Plan::Cut { frames: queued };
        }
        self.last_ns = Some(now_ns);
        let packet = producer_packet.clamp(240, CAPACITY);
        self.target = match self.direction {
            Direction::Capture => 720 + packet / 2,
            Direction::Playback => output_frames + 144 + (packet + output_frames) / 2,
        };
        if self.target >= CAPACITY {
            return Plan::Unsupported;
        }
        let dt = output_frames as f64 / FS;
        if self.primed {
            if queued > self.target.saturating_add(480) {
                self.excess_ns = self.excess_ns.saturating_add((dt * 1e9) as u64);
            } else {
                self.excess_ns = 0;
            }
            if self.excess_ns >= STALL_NS {
                // This callback will emit a bounded recovery gap. Retaining a
                // PRE-consumption target here would add an entire callback's
                // production again on the next invocation and cause repeated
                // cuts for 20/30 ms render callbacks. Keep the post-consumption
                // reserve, compensating for the exact sinc reset lookahead.
                let steady_input = (output_frames as f64 * self.consumption).ceil() as usize;
                let post_target = self.target.saturating_sub(steady_input);
                self.reset(true);
                if self.resampler.set_chunk_size(output_frames).is_err()
                    || self
                        .resampler
                        .set_resample_ratio(1.0 / self.consumption, true)
                        .is_err()
                {
                    return Plan::Unsupported;
                }
                let reset_extra = self
                    .resampler
                    .input_frames_next()
                    .saturating_sub(steady_input);
                let retained = post_target.saturating_add(reset_extra).min(queued);
                return Plan::Cut {
                    frames: queued - retained,
                };
            }
            self.filtered += (1.0 - (-dt / 0.1).exp()) * (queued as f64 - self.filtered);
            let error = (self.filtered - self.target as f64) / FS;
            let omega = std::f64::consts::TAU * 0.005;
            let kp = 2.0 * 0.707 * omega;
            let ki = omega * omega;
            let rate = device_rate
                .filter(|r| r.is_finite() && (0.95..=1.05).contains(r))
                .unwrap_or(1.0);
            let base_consumption = match self.direction {
                Direction::Capture => rate,
                Direction::Playback => 1.0 / rate,
            };
            let trial = self.integral + ki * error * dt;
            let requested = base_consumption * (1.0 + kp * error + trial);
            if (1.0 - MAX_CORRECTION..=1.0 + MAX_CORRECTION).contains(&requested) {
                self.integral = trial;
            }
            let requested = (base_consumption * (1.0 + kp * error + self.integral))
                .clamp(1.0 - MAX_CORRECTION, 1.0 + MAX_CORRECTION);
            self.consumption += (1.0 - (-dt).exp()) * (requested - self.consumption);
            self.consumption = self
                .consumption
                .clamp(1.0 - MAX_CORRECTION, 1.0 + MAX_CORRECTION);
        }
        if self.resampler.set_chunk_size(output_frames).is_err()
            || self
                .resampler
                .set_resample_ratio(1.0 / self.consumption, true)
                .is_err()
        {
            return Plan::Unsupported;
        }
        let needed = self.resampler.input_frames_next();
        if !self.primed {
            let threshold = match self.direction {
                Direction::Capture => needed.saturating_add(480 + packet / 2),
                Direction::Playback => needed.max(self.target.saturating_sub(packet / 2)),
            };
            if queued < threshold {
                return Plan::Prime;
            }
            self.primed = true;
            self.filtered = self.target as f64;
        }
        if queued < needed {
            self.reset(true);
            return Plan::Prime;
        }
        Plan::Pull {
            input_frames: needed,
        }
    }
    /// The lease includes retained sinc input, not merely the newly copied
    /// block. Completion is sampled again so a suspended DSP call cannot renew
    /// old content. The caller still checks its source generation at handoff.
    pub(crate) fn process_timed<'a>(
        &mut self,
        frames: &[[f32; 2]],
        input_origins: impl Into<InputOrigins<'a>>,
        started_ns: u64,
        completed_ns: impl FnOnce() -> u64,
    ) -> Result<Processed<'_>, &'static str> {
        let origins = input_origins.into();
        if !origins.valid(frames.len(), started_ns) {
            self.reset(true);
            return Err("Clock correction input has no valid lease");
        }
        let oldest = self
            .history
            .oldest()
            .into_iter()
            .chain(origins.oldest())
            .min();
        let Some(oldest) = oldest else {
            return Err("Clock correction has no input lease");
        };
        if oldest > started_ns || started_ns.saturating_sub(oldest) > FRESH_NS {
            self.reset(true);
            return Err("Clock correction history expired");
        }
        self.process_inner(frames)?;
        let completed = completed_ns();
        if completed < started_ns
            || completed.saturating_sub(started_ns) > STALL_NS
            || completed.saturating_sub(oldest) > FRESH_NS
        {
            self.reset(true);
            return Err("Clock correction processing expired");
        }
        self.history.append(frames.len(), origins);
        let produced = self.resampler.output_frames_next();
        Ok(Processed {
            frames: &self.interleaved[..produced],
            oldest_ns: oldest,
        })
    }
    #[cfg(test)]
    fn process(&mut self, frames: &[[f32; 2]]) -> Result<&[[f32; 2]], &'static str> {
        self.process_timed(frames, Some(0), 0, || 0)
            .map(|processed| processed.frames)
    }
    fn process_inner(&mut self, frames: &[[f32; 2]]) -> Result<&[[f32; 2]], &'static str> {
        let needed = self.resampler.input_frames_next();
        if frames.len() != needed || needed > self.input[0].len() {
            return Err("Incomplete clock correction input");
        }
        for (index, frame) in frames.iter().enumerate() {
            for (channel, sample) in frame.iter().copied().enumerate() {
                self.input[channel][index] = if sample.is_finite() {
                    sample.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
            }
        }
        let (_, produced) = self
            .resampler
            .process_into_buffer(&self.input, &mut self.output, None)
            .map_err(|_| "Clock correction failed")?;
        for (index, frame) in self.interleaved[..produced].iter_mut().enumerate() {
            *frame = [self.output[0][index], self.output[1][index]];
        }
        self.applied_ppm = Some(((self.consumption - 1.0) * 1e6) as f32);
        Ok(&self.interleaved[..produced])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::recording_format::tests::allocations_during;

    #[test]
    fn fresh_input_cannot_renew_expired_retained_history_or_a_paused_process() {
        let mut link = Link::new(Direction::Capture, 240).unwrap();
        let Plan::Pull { input_frames } = link.plan(1920, 240, 480, None, 0) else {
            panic!()
        };
        let input = vec![[0.25, -0.5]; input_frames];
        let first = link.process_timed(&input, Some(0), 0, || 0).unwrap();
        assert_eq!(first.oldest_ns, 0);
        let Plan::Pull { input_frames } = link.plan(link.target(), 240, 480, None, 20_000_000)
        else {
            panic!()
        };
        let fresh = vec![[0.0; 2]; input_frames];
        assert!(
            link.process_timed(&fresh, Some(79_000_000), 79_000_000, || 81_000_000)
                .is_err()
        );
        assert!(!link.primed());
        assert_eq!(link.applied_ppm(), None);
        let Plan::Pull { input_frames } = link.plan(1920, 240, 480, None, 100_000_000) else {
            panic!()
        };
        assert!(
            link.process_timed(
                &vec![[0.25; 2]; input_frames],
                Some(100_000_000),
                100_000_000,
                || 141_000_000
            )
            .is_err()
        );
        let Plan::Pull { input_frames } = link.plan(1920, 240, 480, None, 142_000_000) else {
            panic!()
        };
        let output = link
            .process_timed(
                &vec![[0.0; 2]; input_frames],
                Some(142_000_000),
                142_000_000,
                || 142_000_000,
            )
            .unwrap();
        assert!(output.frames.iter().all(|frame| *frame == [0.0; 2]));
    }

    #[test]
    fn every_new_input_origin_is_required_and_cannot_hide_behind_older_history() {
        for invalid in [None, Some(11)] {
            let mut link = Link::new(Direction::Capture, 240).unwrap();
            let Plan::Pull { input_frames } = link.plan(1920, 240, 480, None, 0) else {
                panic!()
            };
            link.process_timed(&vec![[0.25; 2]; input_frames], Some(0), 0, || 0)
                .unwrap();
            let Plan::Pull { input_frames } = link.plan(link.target(), 240, 480, None, 10) else {
                panic!()
            };
            assert!(
                link.process_timed(&vec![[0.0; 2]; input_frames], invalid, 10, || 10)
                    .is_err()
            );
            assert!(!link.primed());
            assert_eq!(link.applied_ppm(), None);
        }
        let mut link = Link::new(Direction::Capture, 240).unwrap();
        let Plan::Pull { input_frames } = link.plan(1920, 240, 480, None, 10) else {
            panic!()
        };
        let mut origins = vec![0; input_frames];
        origins[input_frames - 1] = 11;
        assert!(
            link.process_timed(&vec![[0.0; 2]; input_frames], origins.as_slice(), 10, || 10)
                .is_err()
        );
    }

    #[test]
    fn thirty_ms_real_bus_callbacks_preserve_exact_tail_leases_without_false_expiration() {
        let bus = crate::mix_bus::Bus::test_bus();
        let mut link = Link::new(Direction::Playback, CAPACITY).unwrap();
        let mut cursor = bus.cursor();
        let mut input = vec![[0.0; 2]; CAPACITY];
        let mut origins = vec![0u64; CAPACITY];
        let mut calls = 0;
        let allocations = allocations_during(|| {
            for tick in 1..=1200 {
                let ms = tick * 5;
                bus.write(&[[0.125, -0.25]; 240], ms);
                if !tick.is_multiple_of(6) {
                    continue;
                }
                let window = bus.correction_window(cursor, ms);
                match link.plan(window.available, 1440, 240, Some(1.0), ms * 1_000_000) {
                    Plan::Prime => assert!(tick <= 12),
                    Plan::Pull { input_frames } => {
                        let read = bus.read_correction_with_origins(
                            &mut cursor,
                            &mut input[..input_frames],
                            &mut origins[..input_frames],
                            ms,
                        );
                        assert_eq!(read.valid_frames, input_frames);
                        assert_eq!(read.dropped_frames, 0);
                        let output = link
                            .process_timed(
                                &input[..input_frames],
                                &origins[..input_frames],
                                ms * 1_000_000,
                                || ms * 1_000_000,
                            )
                            .unwrap();
                        assert_eq!(output.frames.len(), 1440);
                        assert!(ms * 1_000_000 - output.oldest_ns <= FRESH_NS);
                        assert!(bus.correction_lease_valid(
                            read.continuity,
                            output.oldest_ns / 1_000_000,
                            ms
                        ));
                        calls += 1;
                    }
                    plan => panic!("Unexpected plan: {plan:?}"),
                }
            }
        });
        assert_eq!(allocations, 0);
        assert!(calls >= 199);
    }

    #[test]
    fn zero_input_upsampling_steps_keep_the_prior_history_lease() {
        let mut link = Link::new(Direction::Playback, 240).unwrap();
        link.consumption = 0.999;
        let mut zero_steps = 0;
        for tick in 0..4000 {
            let now = tick * 20_834;
            let queued = if link.primed() { link.target() } else { 1920 };
            let Plan::Pull { input_frames } = link.plan(queued, 1, 240, Some(1.001), now) else {
                panic!()
            };
            let input = vec![[0.125, -0.25]; input_frames];
            if input_frames == 0 {
                zero_steps += 1;
            }
            let output = link
                .process_timed(&input, (input_frames != 0).then_some(now), now, || now)
                .unwrap();
            assert_eq!(output.frames.len(), 1);
            assert!(output.oldest_ns <= now);
        }
        assert!(zero_steps > 0);
    }

    #[test]
    fn native_rate_normalization_and_variable_packet_boundaries_preserve_the_clock() {
        for native_rate in [44_100, 48_000, 96_000] {
            for ppm in [-500.0, 0.0, 500.0] {
                let mut dll = RateEstimator::default();
                let mut position = 0u64;
                let mut time = 9_000_000_000_000_000u64;
                let mut previous = None;
                for callback in 0..30_000 {
                    // The timestamp is the first sample's position, independent
                    // of when a native API happens to deliver the callback.
                    let observed = dll.observe(time, position, native_rate);
                    let frames =
                        [native_rate / 200, native_rate / 100, native_rate / 50][callback % 3];
                    time += (f64::from(frames) / f64::from(native_rate) / (1.0 + ppm / 1e6) * 1e9)
                        .round() as u64;
                    position += u64::from(frames);
                    previous = observed;
                }
                assert!((previous.unwrap() - (1.0 + ppm / 1e6)).abs() < 0.2e-6);
            }
        }
    }

    #[test]
    fn invalid_timestamps_reanchor_without_inventing_a_clock_failure() {
        let mut dll = RateEstimator::default();
        assert_eq!(dll.observe(10, 0, 48_000), None);
        assert_eq!(dll.observe(10, 480, 48_000), None);
        assert_eq!(dll.observe(9, 960, 48_000), None);
        assert_eq!(dll.observe(100_000_000, 1440, 48_000), None);
        assert_eq!(dll.observe(110_000_000, 1920, 0), None);
        assert!(dll.observe(110_000_000, 1920, 48_000).is_some());
        dll.reset();
        assert_eq!(dll.rate(), None);
    }

    #[test]
    fn packet_reserve_uses_native_rate_and_survives_alternating_sizes() {
        let mut sizes = PacketSizes::default();
        assert_eq!(sizes.observe(882, 44_100), 960);
        for _ in 0..15 {
            assert_eq!(sizes.observe(240, 48_000), 960);
        }
        assert_eq!(sizes.observe(240, 48_000), 240);
        assert_eq!(sizes.observe(usize::MAX, 1), CAPACITY);
        assert_eq!(LEGACY_CAPACITY, 1920);
    }

    #[test]
    fn rate_direction_and_consumption_clamp_are_correct() {
        for direction in [Direction::Capture, Direction::Playback] {
            for rate in [0.9995, 1.0005, 0.95, 1.05] {
                let mut link = Link::new(direction, 240).unwrap();
                let mut scratch = [[0.0; 2]; 310];
                let initial = link.plan(1920, 240, 480, Some(rate), 0);
                assert!(matches!(initial, Plan::Pull { .. }));
                let target = link.target();
                for tick in 1..1000 {
                    let plan = link.plan(target, 240, 480, Some(rate), tick * 5_000_000);
                    let Plan::Pull { input_frames } = plan else {
                        panic!("{plan:?}");
                    };
                    scratch[..input_frames].fill([0.0; 2]);
                    link.process(&scratch[..input_frames]).unwrap();
                }
                let ppm = link.applied_ppm().unwrap();
                assert!((-1000.01..=1000.01).contains(&ppm));
                assert_eq!(
                    ppm.is_sign_positive(),
                    match direction {
                        Direction::Capture => rate > 1.0,
                        Direction::Playback => rate < 1.0,
                    }
                );
            }
        }
    }

    #[test]
    fn whole_callback_demand_is_bounded_and_resets_remove_signal_history() {
        for callback in [240, 480, 960, 1440] {
            let mut link = Link::new(Direction::Playback, CAPACITY).unwrap();
            let plan = link.plan(CAPACITY, callback, 240, Some(1.0), 0);
            let Plan::Pull { input_frames } = plan else {
                panic!("{plan:?}");
            };
            let input = vec![[0.25, -0.5]; input_frames];
            let output = link.process(&input).unwrap();
            assert_eq!(output.len(), callback);
            assert!(output.iter().flatten().all(|v| v.is_finite()));
            link.reset(false);
            assert!(!link.primed());
            assert_eq!(link.applied_ppm(), None);
            let Plan::Pull { input_frames } = link.plan(CAPACITY, callback, 240, None, 1) else {
                panic!()
            };
            assert!(
                link.process(&vec![[0.0; 2]; input_frames])
                    .unwrap()
                    .iter()
                    .all(|frame| *frame == [0.0; 2])
            );
            assert!(matches!(
                link.plan(0, CAPACITY + 1, 240, None, 2),
                Plan::Unsupported
            ));
        }
    }

    #[test]
    fn overrun_recovery_keeps_a_post_consumption_reserve_instead_of_cutting_forever() {
        let mut link = Link::new(Direction::Playback, 1440).unwrap();
        let mut cut = None;
        for callback in 0..3 {
            match link.plan(3294, 1440, 240, Some(1.0), callback * 30_000_000) {
                Plan::Pull { input_frames } => {
                    link.process_timed(
                        &vec![[0.0; 2]; input_frames],
                        Some(callback * 30_000_000),
                        callback * 30_000_000,
                        || callback * 30_000_000,
                    )
                    .unwrap();
                }
                Plan::Cut { frames } => {
                    cut = Some(frames);
                    break;
                }
                plan => panic!("{plan:?}"),
            }
        }
        let cut = cut.expect("sustained excess must be recovered");
        assert!(
            cut > 3294 - 2424,
            "merely keeping pre-consumption target causes oscillation"
        );
        let mut queue = 3294 - cut;
        for callback in 3..203 {
            queue += 1440;
            let now = callback * 30_000_000;
            let Plan::Pull { input_frames } = link.plan(queue, 1440, 240, Some(1.0), now) else {
                panic!("repeated recovery at {callback}");
            };
            queue -= input_frames;
            link.process_timed(&vec![[0.0; 2]; input_frames], Some(now), now, || now)
                .unwrap();
            assert!(queue < 1500);
        }
    }

    #[test]
    fn resumed_reader_requires_a_bounded_prefix_discard_before_reprime() {
        let mut link = Link::new(Direction::Capture, 240).unwrap();
        assert!(matches!(
            link.plan(1920, 240, 480, None, 1),
            Plan::Pull { .. }
        ));
        assert_eq!(
            link.plan(3000, 240, 480, None, 50_000_002),
            Plan::Cut { frames: 3000 }
        );
        assert!(!link.primed());
        assert_eq!(link.plan(0, 240, 480, None, 50_000_003), Plan::Prime);
    }

    #[test]
    fn callback_processing_and_reset_allocate_nothing() {
        let mut link = Link::new(Direction::Capture, 240).unwrap();
        let mut input = [[0.0; 2]; 310];
        let count = allocations_during(|| {
            for tick in 0..2000 {
                if tick % 17 == 0 {
                    link.reset(true);
                }
                let queued = if link.primed() { link.target() } else { 1920 };
                let plan = link.plan(queued, 240, 480, Some(1.0005), tick * 5_000_000);
                let Plan::Pull { input_frames } = plan else {
                    panic!("{plan:?}");
                };
                input[..input_frames].fill([0.125, -0.25]);
                link.process(&input[..input_frames]).unwrap();
            }
        });
        assert_eq!(count, 0);
    }
}

/// Raw observed scalars. Strings and serialized payloads are built only when
/// the worker publishes its 100 ms status window.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Timing {
    pub epoch: [u64; 3],
    pub phase: crate::model::TimingPhase,
    pub applied: Option<f64>,
    pub queued: Option<u32>,
    pub target: Option<u32>,
    pub capacity: u32,
    pub gaps: u64,
    pub dropped: u64,
    pub resyncs: u64,
    pub reason: Option<crate::model::TimingResetReason>,
    pub reset_ms: Option<f64>,
    pub state_ms: Option<f64>,
    pub audio_ms: Option<f64>,
    pub started: bool,
}
impl Timing {
    pub(crate) fn new(enabled: bool, epoch: [u64; 3]) -> Self {
        Self {
            epoch,
            phase: if enabled {
                crate::model::TimingPhase::Priming
            } else {
                crate::model::TimingPhase::Off
            },
            applied: None,
            queued: None,
            target: None,
            capacity: if enabled {
                CAPACITY as u32
            } else {
                LEGACY_CAPACITY as u32
            },
            gaps: 0,
            dropped: 0,
            resyncs: 0,
            reason: None,
            reset_ms: None,
            state_ms: None,
            audio_ms: None,
            started: false,
        }
    }
    pub(crate) fn reset(&mut self, reason: crate::model::TimingResetReason, now_ms: f64) {
        self.phase = crate::model::TimingPhase::Priming;
        self.applied = None;
        self.audio_ms = None;
        self.resyncs = self.resyncs.saturating_add(1);
        self.reason = Some(reason);
        self.reset_ms = Some(now_ms);
        self.state_ms = Some(now_ms);
    }
    pub(crate) fn snapshot(&self) -> crate::model::TimingObservation {
        const SAFE: u64 = 9_007_199_254_740_991;
        crate::model::TimingObservation {
            epoch: format!("{}:{}:{}", self.epoch[0], self.epoch[1], self.epoch[2]),
            phase: self.phase,
            applied_adjustment_ppm: self.applied,
            queued_frames: self.queued,
            target_frames: self.target,
            capacity_frames: self.capacity,
            gap_output_frames: self.gaps.min(SAFE),
            dropped_input_frames: self.dropped.min(SAFE),
            resync_count: self.resyncs.min(SAFE),
            last_reset_reason: self.reason,
            last_reset_at_ms: self.reset_ms,
            state_observed_at_ms: self.state_ms,
            audio_observed_at_ms: self.audio_ms,
        }
    }
}

/// One render callback writer and a nonblocking worker-status reader. The fixed
/// array avoids callback-side strings, collections or locks.
pub(crate) struct TimingPublication {
    sequence: std::sync::atomic::AtomicU64,
    fields: [std::sync::atomic::AtomicU64; 16],
}
impl Default for TimingPublication {
    fn default() -> Self {
        Self {
            sequence: std::sync::atomic::AtomicU64::new(0),
            fields: std::array::from_fn(|_| std::sync::atomic::AtomicU64::new(0)),
        }
    }
}
impl TimingPublication {
    pub(crate) fn publish(&self, value: &Timing) {
        use crate::model::{TimingPhase as P, TimingResetReason as R};
        use std::sync::atomic::Ordering::SeqCst;
        let bits = |value: Option<f64>| value.unwrap_or(f64::NAN).to_bits();
        let fields = [
            value.epoch[0],
            value.epoch[1],
            value.epoch[2],
            match value.phase {
                P::Off => 0,
                P::Priming => 1,
                P::Running => 2,
            },
            bits(value.applied),
            value.queued.map_or(u64::MAX, u64::from),
            value.target.map_or(u64::MAX, u64::from),
            u64::from(value.capacity),
            value.gaps,
            value.dropped,
            value.resyncs,
            match value.reason {
                None => 0,
                Some(R::Discontinuity) => 1,
                Some(R::StalledInput) => 2,
                Some(R::PausedOutput) => 3,
                Some(R::Starved) => 4,
                Some(R::Overrun) => 5,
            },
            bits(value.reset_ms),
            bits(value.state_ms),
            bits(value.audio_ms),
            u64::from(value.started),
        ];
        self.sequence.fetch_add(1, SeqCst);
        for (slot, value) in self.fields.iter().zip(fields) {
            slot.store(value, SeqCst);
        }
        self.sequence.fetch_add(1, SeqCst);
    }
    pub(crate) fn snapshot(&self) -> Option<crate::model::TimingObservation> {
        use crate::model::{TimingPhase as P, TimingResetReason as R};
        use std::sync::atomic::Ordering::SeqCst;
        let sequence = self.sequence.load(SeqCst);
        if sequence == 0 || !sequence.is_multiple_of(2) {
            return None;
        }
        let values = self.fields.each_ref().map(|field| field.load(SeqCst));
        if sequence != self.sequence.load(SeqCst) {
            return None;
        }
        let number = |value: u64| {
            let v = f64::from_bits(value);
            v.is_finite().then_some(v)
        };
        let frames = |value: u64| u32::try_from(value).ok();
        let value = Timing {
            epoch: [values[0], values[1], values[2]],
            phase: match values[3] {
                0 => P::Off,
                1 => P::Priming,
                2 => P::Running,
                _ => return None,
            },
            applied: number(values[4]),
            queued: frames(values[5]),
            target: frames(values[6]),
            capacity: frames(values[7])?,
            gaps: values[8],
            dropped: values[9],
            resyncs: values[10],
            reason: match values[11] {
                0 => None,
                1 => Some(R::Discontinuity),
                2 => Some(R::StalledInput),
                3 => Some(R::PausedOutput),
                4 => Some(R::Starved),
                5 => Some(R::Overrun),
                _ => return None,
            },
            reset_ms: number(values[12]),
            state_ms: number(values[13]),
            audio_ms: number(values[14]),
            started: values[15] != 0,
        };
        Some(value.snapshot())
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    #[test]
    fn timing_publication_is_coherent_bounded_and_callback_allocation_free() {
        use crate::model::{TimingPhase, TimingResetReason};
        let mut timing = Timing::new(true, [u64::MAX, u64::MAX - 1, 5]);
        timing.reset(TimingResetReason::Starved, 10.0);
        timing.phase = TimingPhase::Running;
        timing.applied = Some(42.5);
        timing.queued = Some(128);
        timing.target = Some(384);
        timing.gaps = u64::MAX;
        timing.dropped = u64::MAX;
        timing.resyncs = u64::MAX;
        timing.state_ms = Some(11.0);
        timing.audio_ms = Some(12.0);
        timing.started = true;
        let expected = timing.snapshot();
        assert_eq!(
            expected.epoch,
            "18446744073709551615:18446744073709551614:5"
        );
        assert_eq!(expected.gap_output_frames, 9_007_199_254_740_991);
        let publication = TimingPublication::default();
        assert_eq!(publication.snapshot(), None);
        assert_eq!(
            crate::audio::recording_format::tests::allocations_during(|| {
                for _ in 0..1000 {
                    publication.publish(&timing);
                }
            }),
            0
        );
        assert_eq!(publication.snapshot(), Some(expected));
        publication
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(publication.snapshot(), None);
    }
}
