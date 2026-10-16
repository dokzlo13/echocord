//! Optional stereo-linked sample-peak protection for the 48 kHz shared mix.
//!
//! Enabled processing delays PCM by exactly [`LATENCY_FRAMES`], including zero
//! priming after reset. Only the peak detector interpolates; emitted PCM is the
//! original delayed frame times one gain shared by both channels. No makeup gain
//! or reconstructed/codec true-peak guarantee is provided. A detected overload
//! can briefly lower other audio in the mix, including native samples below the
//! ceiling whose interpolated peaks exceed it.
//! The envelope is tuned for the audible band; sustained overload below 20 Hz
//! can produce substantial gain modulation. It is not a subsonic compressor.
//!
//! Ordinary source edits pass once through this fixed delay: resetting on every
//! fader, mute or source revision would interrupt unaffected peers. The owner
//! must reset on whole-engine stop, discontinuity or stale-clock recovery BEFORE
//! publishing resumed audio. Destination mute remains downstream of this module.

mod coefficients;

/// Fixed latency while enabled: 32 detector frames plus 144 gain-lookahead frames.
pub const LATENCY_FRAMES: usize = 176;
/// Native f32 sample ceiling, not a reconstructed or encoded-output peak bound.
pub const SAMPLE_CEILING: f32 = 0.98;
const DETECTOR_RADIUS: usize = 32;
const DETECTOR_TAPS: usize = 2 * DETECTOR_RADIUS + 1;
const OVERSAMPLING: usize = 8;
const PEAK_WINDOW: usize = 1_200; // 25 ms covers one absolute-peak period at 20 Hz.
const QUEUE_CAPACITY: usize = PEAK_WINDOW + 1;
const ATTACK_FRAMES: f64 = 72.0; // Stable lower targets are reached within 1.5 ms.
const RELEASE: f64 = 0.995_842_001_845_11; // exp(-1 / (48000 * 0.005)): 5 ms e-fold.
const MAX_REDUCTION_DB: f32 = 800.0;

#[derive(Clone, Copy, Default)]
struct Peak {
    sequence: u64,
    value: f64,
}

/// Fixed-capacity, thread-owned processor. Processing and reset never allocate.
pub struct Limiter {
    enabled: bool,
    pcm: [[f32; 2]; LATENCY_FRAMES + 1],
    pcm_head: usize,
    // Duplicated ring makes each FIR window contiguous without per-tap modulo.
    history: [[f64; 2]; DETECTOR_TAPS * 2],
    history_head: usize,
    previous_phase: [f64; 2],
    peaks: [Peak; QUEUE_CAPACITY],
    peak_head: usize,
    peak_len: usize,
    sequence: u64,
    gain: f64,
    #[cfg(test)]
    guard_hits: u64,
}

impl Limiter {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            pcm: [[0.0; 2]; LATENCY_FRAMES + 1],
            pcm_head: 0,
            history: [[0.0; 2]; DETECTOR_TAPS * 2],
            history_head: 0,
            previous_phase: [0.0; 2],
            peaks: [Peak::default(); QUEUE_CAPACITY],
            peak_head: 0,
            peak_len: 0,
            sequence: 0,
            gain: 1.0,
            #[cfg(test)]
            guard_hits: 0,
        }
    }

    /// Forget all delayed PCM and envelope history, preserving enabled state.
    pub fn reset(&mut self) {
        self.pcm.fill([0.0; 2]);
        self.pcm_head = 0;
        self.history.fill([0.0; 2]);
        self.history_head = 0;
        self.previous_phase = [0.0; 2];
        self.peak_head = 0;
        self.peak_len = 0;
        self.sequence = 0;
        self.gain = 1.0;
        #[cfg(test)]
        {
            self.guard_hits = 0;
        }
    }

    /// A mode change clears pending audio. Repeating the current mode is inert.
    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled != self.enabled {
            self.reset();
            self.enabled = enabled;
        }
    }

    /// Process any block size, returning its maximum nonnegative reduction in dB.
    ///
    /// Disabled processing preserves every input bit and returns zero. Enabled
    /// processing replaces nonfinite samples with zero. With unity detector gain,
    /// finite PCM is bit-exact after the fixed delay. Empty blocks return zero
    /// without advancing state. Telemetry is finite even for extreme finite input.
    pub fn process_block(&mut self, frames: &mut [[f32; 2]]) -> f32 {
        if !self.enabled || frames.is_empty() {
            return 0.0;
        }
        let mut minimum_gain = 1.0_f64;
        for frame in frames {
            *frame = self.process_frame(*frame);
            minimum_gain = minimum_gain.min(self.gain);
        }
        // One logarithm per block, not per sample. Zero has positive sign.
        if minimum_gain >= 1.0 {
            0.0
        } else {
            (-20.0 * minimum_gain.max(f64::MIN_POSITIVE).log10()).min(f64::from(MAX_REDUCTION_DB))
                as f32
        }
    }

    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let input = frame.map(|sample| if sample.is_finite() { sample } else { 0.0 });
        self.pcm[self.pcm_head] = input;
        self.pcm_head = (self.pcm_head + 1) % self.pcm.len();
        let delayed = self.pcm[self.pcm_head];

        let wide = input.map(f64::from);
        self.history[self.history_head] = wide;
        self.history[self.history_head + DETECTOR_TAPS] = wide;
        self.history_head = (self.history_head + 1) % DETECTOR_TAPS;
        let history = &self.history[self.history_head..self.history_head + DETECTOR_TAPS];
        let mut phases = [[0.0; 2]; OVERSAMPLING];
        phases[0] = history[DETECTOR_RADIUS].map(f64::abs);
        for (phase, coefficients) in phases[1..].iter_mut().zip(&coefficients::COEFFICIENTS) {
            let mut sum = [0.0; 2];
            for (sample, coefficient) in history.iter().zip(coefficients) {
                sum[0] += sample[0] * coefficient;
                sum[1] += sample[1] * coefficient;
            }
            *phase = sum.map(f64::abs);
        }
        let next_phase = history[DETECTOR_RADIUS + 1].map(f64::abs);
        let mut peak = 0.0_f64;
        for (index, phase) in phases.iter().enumerate() {
            for (channel, &value) in phase.iter().enumerate() {
                let previous = if index == 0 {
                    self.previous_phase[channel]
                } else {
                    phases[index - 1][channel]
                };
                let next = if index + 1 == OVERSAMPLING {
                    next_phase[channel]
                } else {
                    phases[index + 1][channel]
                };
                let denominator = previous - 2.0 * value + next;
                let refined = if value >= previous && value >= next && denominator < -1e-20 {
                    value - (previous - next).powi(2) / (8.0 * denominator)
                } else {
                    value
                };
                peak = peak.max(refined);
            }
        }
        self.previous_phase = phases[OVERSAMPLING - 1];

        // Scalar-only sliding maximum. A single frame can pop at most PEAK_WINDOW
        // entries; across frames every entry is inserted and removed only once.
        while self.peak_len > 0
            && self
                .sequence
                .wrapping_sub(self.peaks[self.peak_head].sequence)
                >= PEAK_WINDOW as u64
        {
            self.peak_head = (self.peak_head + 1) % QUEUE_CAPACITY;
            self.peak_len -= 1;
        }
        while self.peak_len > 0
            && self.peaks[(self.peak_head + self.peak_len - 1) % QUEUE_CAPACITY].value <= peak
        {
            self.peak_len -= 1;
        }
        debug_assert!(self.peak_len < QUEUE_CAPACITY);
        self.peaks[(self.peak_head + self.peak_len) % QUEUE_CAPACITY] = Peak {
            sequence: self.sequence,
            value: peak,
        };
        self.peak_len += 1;
        self.sequence = self.sequence.wrapping_add(1);
        let target = gain_for_peak(self.peaks[self.peak_head].value);
        self.gain = if target < self.gain {
            target.max(self.gain - (1.0 - target) / ATTACK_FRAMES)
        } else {
            target.min(1.0 - (1.0 - self.gain) * RELEASE)
        };

        // The final bound uses this emitted frame, never a different detector
        // timestamp. f64 keeps even extreme f32 finite inputs safe throughout.
        let native_peak = f64::from(delayed[0]).abs().max(f64::from(delayed[1]).abs());
        let bound = gain_for_peak(native_peak);
        #[cfg(test)]
        if self.gain > bound + 1e-12 {
            self.guard_hits += 1;
        }
        self.gain = self.gain.min(bound);
        delayed.map(|sample| {
            ((f64::from(sample) * self.gain) as f32).clamp(-SAMPLE_CEILING, SAMPLE_CEILING)
        })
    }
}

fn gain_for_peak(peak: f64) -> f64 {
    if peak <= f64::from(SAMPLE_CEILING) {
        1.0
    } else {
        f64::from(SAMPLE_CEILING) / peak
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::recording_format::tests::allocations_during;
    use std::{f64::consts::TAU, hint::black_box, time::Instant};

    const RATE: usize = 48_000;
    const FREQUENCIES: [f64; 28] = [
        20.0, 20.1, 25.0, 31.0, 40.0, 63.0, 80.0, 125.0, 250.0, 440.0, 997.0, 1000.0, 2000.0,
        4000.0, 7999.0, 8000.0, 8001.0, 11999.0, 12000.0, 12001.0, 12345.67, 15999.0, 16000.0,
        16001.0, 19997.0, 19999.0, 19999.3, 20000.0,
    ];

    fn random(seed: &mut u64) -> f32 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*seed >> 32) as u32 as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
    }

    fn tone(frequency: f64, amplitude: f64, phase: f64, frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|index| {
                let sample = (amplitude
                    * (index as f64 * frequency * TAU / RATE as f64 + phase).sin())
                    as f32;
                [sample, sample * 0.5]
            })
            .collect()
    }

    fn frame(limiter: &mut Limiter, frame: [f32; 2]) -> ([f32; 2], f32) {
        let mut block = [frame];
        let reduction = limiter.process_block(&mut block);
        (block[0], reduction)
    }

    #[test]
    fn quiet_unity_gain_is_exactly_delayed_and_disabled_is_bit_exact() {
        let input = tone(1000.0, 0.9, 0.0, 3000);
        let mut output = input.clone();
        let mut limiter = Limiter::new(true);
        assert_eq!(limiter.process_block(&mut output), 0.0);
        assert!(
            output[..LATENCY_FRAMES]
                .iter()
                .all(|sample| *sample == [0.0; 2])
        );
        assert_eq!(
            &output[LATENCY_FRAMES..],
            &input[..input.len() - LATENCY_FRAMES]
        );
        limiter.set_enabled(false);
        let mut unusual = [
            [-0.0, f32::from_bits(1)],
            [f32::MAX, f32::INFINITY],
            [f32::from_bits(0x7fc01234), f32::NEG_INFINITY],
        ];
        let expected = unusual.map(|sample| sample.map(f32::to_bits));
        assert_eq!(limiter.process_block(&mut unusual), 0.0);
        assert_eq!(unusual.map(|sample| sample.map(f32::to_bits)), expected);
    }

    #[test]
    fn native_samples_below_ceiling_can_have_larger_interpolated_peaks() {
        // A 12 kHz wave at this phase has native samples +/-0.9 but a
        // reconstructed amplitude near 1.273. Do not promise universal bypass
        // merely because all native samples are below the ceiling.
        let mut input: Vec<_> = (0..3000)
            .map(|index| {
                let sample = if index % 4 < 2 { 0.9 } else { -0.9 };
                [sample, sample * 0.5]
            })
            .collect();
        let mut limiter = Limiter::new(true);
        assert!(limiter.process_block(&mut input) > 2.0);
        assert!(input[1500..].iter().all(|sample| sample[0].abs() < 0.7));
        assert_eq!(limiter.guard_hits, 0);
    }

    #[test]
    fn impulses_have_exact_delay_linked_gain_and_bounded_finite_telemetry() {
        for amplitude in [1.5, 3.0, 8.0, 1000.0, f32::MAX] {
            let mut limiter = Limiter::new(true);
            for index in 0..1000 {
                let (output, reduction) = frame(
                    &mut limiter,
                    if index == 300 {
                        [amplitude, amplitude * 0.5]
                    } else {
                        [0.0; 2]
                    },
                );
                assert!(reduction.is_finite() && (0.0..=MAX_REDUCTION_DB).contains(&reduction));
                if index == 300 + LATENCY_FRAMES {
                    assert!((0.979..=SAMPLE_CEILING).contains(&output[0]));
                    assert_eq!(output[1], output[0] * 0.5);
                } else {
                    assert_eq!(output, [0.0; 2]);
                }
            }
            assert_eq!(limiter.guard_hits, 0);
        }
    }

    #[test]
    fn block_partition_and_telemetry_are_invariant_through_counter_wrap() {
        let mut seed = 72;
        let input: Vec<_> = (0..12000)
            .map(|index| {
                if index % 127 == 0 {
                    [f32::MAX, -f32::MAX]
                } else if index % 53 == 0 {
                    [f32::NAN, f32::INFINITY]
                } else {
                    [random(&mut seed) * 6.0, random(&mut seed) * 6.0]
                }
            })
            .collect();
        let mut whole = input.clone();
        let mut partitioned = input;
        let mut one = Limiter::new(true);
        let mut two = Limiter::new(true);
        one.sequence = u64::MAX - 200;
        two.sequence = u64::MAX - 200;
        let whole_reduction = one.process_block(&mut whole);
        let chunks = [1, 3, 240, 17, 911, 0, 42];
        let (mut offset, mut index) = (0, 0);
        let mut partitioned_reduction = 0.0_f32;
        while offset < partitioned.len() {
            let end = (offset + chunks[index % chunks.len()]).min(partitioned.len());
            partitioned_reduction =
                partitioned_reduction.max(two.process_block(&mut partitioned[offset..end]));
            offset = end;
            index += 1;
        }
        assert_eq!(whole, partitioned);
        assert!(
            whole
                .iter()
                .flatten()
                .all(|sample| sample.is_finite() && sample.abs() <= SAMPLE_CEILING)
        );
        assert_eq!(whole_reduction, partitioned_reduction);
        assert!(whole_reduction.is_finite() && whole_reduction > 700.0);
        assert_eq!(one.process_block(&mut []), 0.0);
        assert_eq!(one.gain, two.gain);
    }

    #[test]
    fn reset_toggle_and_repeated_setting_obey_the_audio_lifetime() {
        let mut limiter = Limiter::new(true);
        for _ in 0..LATENCY_FRAMES {
            assert_eq!(frame(&mut limiter, [8.0, 4.0]).0, [0.0; 2]);
        }
        limiter.set_enabled(true); // Repeated snapshots must not discard pending audio.
        assert!(frame(&mut limiter, [0.0; 2]).0[0] > 0.0);
        limiter.reset();
        for _ in 0..LATENCY_FRAMES * 3 {
            assert_eq!(frame(&mut limiter, [0.0; 2]), ([0.0; 2], 0.0));
        }
        for _ in 0..LATENCY_FRAMES {
            frame(&mut limiter, [8.0, 4.0]);
        }
        limiter.set_enabled(false);
        assert_eq!(frame(&mut limiter, [0.2, 0.1]), ([0.2, 0.1], 0.0));
        limiter.set_enabled(true);
        for _ in 0..LATENCY_FRAMES * 3 {
            assert_eq!(frame(&mut limiter, [0.0; 2]), ([0.0; 2], 0.0));
        }
    }

    #[test]
    fn source_edits_keep_the_fixed_horizon_and_do_not_gap_a_peer() {
        let input: Vec<_> = (0..3000)
            .map(|index| {
                let peer = (index as f64 * 0.017).sin() as f32 * 0.1;
                // A stand-in for add/mute/unmute/replace/fader edits BEFORE summing.
                let edited = match index {
                    0..500 => 0.2,
                    500..501 => 0.0,
                    501..800 => 0.2,
                    800..1600 => -0.15,
                    1600..2000 => (2000 - index) as f32 * 0.0002,
                    _ => 0.0,
                };
                [peer + edited, peer]
            })
            .collect();
        let mut output = input.clone();
        output.extend([[0.0; 2]; LATENCY_FRAMES]);
        let mut limiter = Limiter::new(true);
        assert_eq!(limiter.process_block(&mut output), 0.0);
        assert_eq!(&output[LATENCY_FRAMES..], input.as_slice());

        limiter.reset();
        for index in 0..3000 {
            let (output, _) = frame(&mut limiter, [if index < 500 { 6.0 } else { 0.1 }, 0.1]);
            if index >= LATENCY_FRAMES {
                assert!(output[1] > 0.0 && output[1] <= 0.1);
            }
            if index >= 500 + LATENCY_FRAMES {
                assert_eq!(output[0], output[1]);
            }
        }
    }

    #[test]
    fn brief_overload_has_bounded_anticipation_and_release_tail() {
        let mut input = tone(330.0, 0.9, 0.0, RATE);
        let start = RATE / 2;
        let end = start + 200;
        for sample in &mut input[start..end] {
            sample[0] += 0.8;
            sample[1] += 0.4;
        }
        let mut output = input.clone();
        let mut limiter = Limiter::new(true);
        limiter.process_block(&mut output);
        let reduction = |index: usize| {
            -20.0
                * (output[index + LATENCY_FRAMES][0] / input[index][0])
                    .abs()
                    .log10()
        };
        assert!(reduction(start - 3 * 48).abs() < 0.001);
        assert!((4.7..4.9).contains(&reduction(start - 48)));
        assert!((4.7..4.9).contains(&reduction(end + 20 * 48)));
        assert!((2.0..2.2).contains(&reduction(end + 25 * 48)));
        assert!(reduction(end + 50 * 48).abs() < 0.02);
        assert!(reduction(end + 100 * 48).abs() < 0.001);
        assert!(
            output
                .iter()
                .flatten()
                .all(|sample| sample.abs() <= SAMPLE_CEILING)
        );
        assert_eq!(limiter.guard_hits, 0);
    }

    #[test]
    fn impulse_pairs_steps_and_all_source_counts_obey_the_ceiling() {
        for gap in [1, 31, 32, 64, 71, 72, 143, 144, 175, 176, 1199, 1200, 1300] {
            let mut input = vec![[0.0; 2]; 64 + gap + LATENCY_FRAMES + 1];
            input[64] = [8.0, 4.0];
            input[64 + gap] = [-8.0, 4.0];
            let mut limiter = Limiter::new(true);
            limiter.process_block(&mut input);
            assert!(
                input
                    .iter()
                    .flatten()
                    .all(|sample| sample.abs() <= SAMPLE_CEILING)
            );
            assert_eq!(limiter.guard_hits, 0, "impulse gap {gap}");
        }
        for frequency in [
            20.0, 31.0, 440.0, 7999.0, 8001.0, 11999.0, 12001.0, 16001.0, 20000.0,
        ] {
            for count in 1..=8 {
                let mut input = tone(frequency, 0.75 * f64::from(count), 0.0, 2400);
                let mut limiter = Limiter::new(true);
                limiter.process_block(&mut input);
                assert!(
                    input
                        .iter()
                        .flatten()
                        .all(|sample| sample.abs() <= SAMPLE_CEILING)
                );
                assert_eq!(
                    limiter.guard_hits, 0,
                    "frequency {frequency}, count {count}"
                );
            }
        }
        let mut limiter = Limiter::new(true);
        for index in 0..12000 {
            let sample = if (index / 53) % 2 == 0 { 6.0 } else { 0.1 };
            let (output, _) = frame(&mut limiter, [sample, -sample]);
            assert!(
                output
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= SAMPLE_CEILING)
            );
        }
        assert_eq!(limiter.guard_hits, 0);
    }

    #[test]
    fn processing_reset_and_toggle_allocate_nothing() {
        let mut limiter = Limiter::new(true);
        let mut seed = 77;
        let mut block = [[0.0; 2]; 240];
        let allocations = allocations_during(|| {
            for index in 0..200 {
                for frame in &mut block {
                    *frame = [random(&mut seed) * 6.0, random(&mut seed)];
                }
                black_box(limiter.process_block(black_box(&mut block)));
                if index % 13 == 0 {
                    limiter.reset();
                    limiter.set_enabled(false);
                    limiter.set_enabled(true);
                }
            }
        });
        assert_eq!(allocations, 0);
    }

    fn tone_residual(frequency: f64, phase: f64, sources: u32) -> f64 {
        let mut output = tone(
            frequency,
            0.75 * f64::from(sources),
            phase,
            RATE + RATE / 10 + LATENCY_FRAMES,
        );
        let mut limiter = Limiter::new(true);
        for chunk in output.chunks_mut(240) {
            limiter.process_block(chunk);
        }
        assert!(
            output
                .iter()
                .flatten()
                .all(|sample| sample.abs() <= SAMPLE_CEILING)
        );
        assert_eq!(limiter.guard_hits, 0);
        let range = RATE / 10 + LATENCY_FRAMES..output.len();
        let (mut ss, mut cc, mut sc, mut sy, mut cy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for index in range.clone() {
            let (sine, cosine) =
                ((index - LATENCY_FRAMES) as f64 * frequency * TAU / RATE as f64).sin_cos();
            let sample = f64::from(output[index][0]);
            ss += sine * sine;
            cc += cosine * cosine;
            sc += sine * cosine;
            sy += sine * sample;
            cy += cosine * sample;
        }
        let determinant = ss * cc - sc * sc;
        let sine_gain = (sy * cc - cy * sc) / determinant;
        let cosine_gain = (cy * ss - sy * sc) / determinant;
        let (mut residual, mut signal) = (0.0, 0.0);
        for index in range {
            let (sine, cosine) =
                ((index - LATENCY_FRAMES) as f64 * frequency * TAU / RATE as f64).sin_cos();
            let fitted = sine_gain * sine + cosine_gain * cosine;
            residual += (f64::from(output[index][0]) - fitted).powi(2);
            signal += fitted * fitted;
        }
        10.0 * (residual / signal).max(1e-30).log10()
    }

    #[test]
    fn full_band_and_near_rational_tones_avoid_gain_modulation() {
        for frequency in [20.0, 8001.0, 12001.0, 16001.0, 20000.0] {
            let residual = tone_residual(frequency, std::f64::consts::FRAC_PI_8, 7);
            assert!(residual <= -100.0, "{frequency} Hz: {residual} dB THD+N");
        }
    }

    #[test]
    #[ignore = "dense DSP measurement; run with --release --ignored --nocapture"]
    fn measure_limiter_full_band_quality() {
        let mut worst = f64::NEG_INFINITY;
        for frequency in FREQUENCIES {
            for phase in [0.0, std::f64::consts::FRAC_PI_8] {
                for sources in 1..=8 {
                    let residual = tone_residual(frequency, phase, sources);
                    assert!(
                        residual <= -100.0,
                        "{frequency} Hz, phase {phase}, {sources} sources: {residual} dB THD+N"
                    );
                    worst = worst.max(residual);
                }
            }
        }
        println!(
            "448 tones, exact {LATENCY_FRAMES}-frame alignment: worst THD+N {worst:.4} dB; sample ceiling {SAMPLE_CEILING}"
        );
    }

    #[test]
    #[ignore = "multitone DSP measurement; run with --release --ignored --nocapture"]
    fn measure_limiter_multitone_gain_modulation() {
        for tones in [2, 8, 24] {
            for peak in [1.5, 6.0] {
                let mut input: Vec<_> = (0..RATE * 2 + LATENCY_FRAMES)
                    .map(|index| {
                        let sample = (0..tones)
                            .map(|tone| {
                                let frequency = 137.0 + f64::from(tone) * 787.0;
                                (index as f64 * frequency * TAU / RATE as f64
                                    + f64::from(tone) * 0.37)
                                    .sin()
                            })
                            .sum::<f64>() as f32;
                        [sample, sample * 0.5]
                    })
                    .collect();
                let maximum = input
                    .iter()
                    .map(|frame| frame[0].abs())
                    .fold(0.0_f32, f32::max);
                for sample in input.iter_mut().flatten() {
                    *sample *= peak / maximum;
                }
                let mut output = input.clone();
                let mut limiter = Limiter::new(true);
                limiter.process_block(&mut output);
                let range = RATE..RATE * 2;
                let (mut cross, mut power) = (0.0, 0.0);
                for index in range.clone() {
                    let sample = f64::from(input[index][0]);
                    cross += sample * f64::from(output[index + LATENCY_FRAMES][0]);
                    power += sample * sample;
                }
                let gain = cross / power;
                let residual: f64 = range
                    .map(|index| {
                        (f64::from(output[index + LATENCY_FRAMES][0])
                            - gain * f64::from(input[index][0]))
                        .powi(2)
                    })
                    .sum();
                // Includes deliberate gain modulation: this is not pure-tone THD.
                let residual_db = 10.0 * (residual / (gain * gain * power)).log10();
                assert!(
                    residual_db < -60.0,
                    "{tones} tones, peak {peak}: {residual_db} dB"
                );
                assert!(
                    output
                        .iter()
                        .flatten()
                        .all(|sample| sample.abs() <= SAMPLE_CEILING)
                );
                assert_eq!(limiter.guard_hits, 0);
                println!(
                    "{tones} tones, input peak {peak}: gain-modulation residual {residual_db:.4} dB"
                );
            }
        }
    }

    #[test]
    #[ignore = "host-dependent benchmark; run with --release --ignored --nocapture"]
    fn benchmark_limiter_processing() {
        for mode in 0..3 {
            let mut limiter = Limiter::new(true);
            let input: Vec<_> = (0..RATE)
                .map(|index| match mode {
                    0 => [0.0; 2],
                    1 => {
                        let sample = (index as f64 * 997.0 * TAU / RATE as f64).sin() as f32 * 6.0;
                        [sample, sample * 0.5]
                    }
                    _ => {
                        let sample = (1200 - index % 1200) as f32 / 200.0;
                        [sample, -sample]
                    }
                })
                .collect();
            let mut block = [[0.0; 2]; 240];
            let started = Instant::now();
            for _ in 0..100 {
                for chunk in input.as_chunks::<240>().0 {
                    block.copy_from_slice(chunk);
                    black_box(limiter.process_block(black_box(&mut block)));
                }
            }
            println!(
                "mode {mode}, 100 seconds audio: {:.3} ns/frame; state {} bytes",
                started.elapsed().as_nanos() as f64 / (100 * RATE) as f64,
                std::mem::size_of::<Limiter>()
            );
            assert_eq!(limiter.guard_hits, 0);
        }
    }
}
