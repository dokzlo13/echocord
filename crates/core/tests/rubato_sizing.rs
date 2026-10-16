//! Exercise the prepared dependency's real buffer copying, ratio setters and
//! lifetime methods. A custom interpolator audits every actual read window;
//! this tests initialized input, not merely the oversized backing allocation.
use rubato::{Resampler, SincFixedOut, SincInterpolationType, sinc_interpolator::SincInterpolator};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const SINC_LENGTH: usize = 128;
const MAX_RELATIVE_RATIO: f64 = 1.004;

#[derive(Default)]
struct ReadAudit {
    initialized_end: AtomicUsize,
    largest_read_end: AtomicUsize,
    reads: AtomicUsize,
}

struct AuditedInterpolator(Arc<ReadAudit>);
impl SincInterpolator<f32> for AuditedInterpolator {
    fn len(&self) -> usize {
        SINC_LENGTH
    }
    fn nbr_sincs(&self) -> usize {
        256
    }
    fn get_sinc_interpolated(&self, wave: &[f32], index: usize, subindex: usize) -> f32 {
        let end = index.checked_add(SINC_LENGTH).unwrap();
        let initialized = self.0.initialized_end.load(Ordering::Relaxed);
        assert!(
            end <= initialized,
            "read ends at {end}, initialized data ends at {initialized}"
        );
        assert!(end <= wave.len());
        assert!(subindex < self.nbr_sincs());
        self.0.largest_read_end.fetch_max(end, Ordering::Relaxed);
        self.0.reads.fetch_add(1, Ordering::Relaxed);
        // The real index selection and buffer lifetime are under test, not FIR
        // quality (covered by separate audio fixtures).
        wave[index]
    }
}

fn make(chunk: usize, interpolation: SincInterpolationType) -> (SincFixedOut<f32>, Arc<ReadAudit>) {
    let audit = Arc::new(ReadAudit::default());
    let resampler = SincFixedOut::new_with_interpolator(
        1.0,
        MAX_RELATIVE_RATIO,
        interpolation,
        Box::new(AuditedInterpolator(audit.clone())),
        chunk,
        2,
    )
    .unwrap();
    (resampler, audit)
}

fn process(
    resampler: &mut SincFixedOut<f32>,
    audit: &ReadAudit,
    input: &[Vec<f32>; 2],
    output: &mut [Vec<f32>; 2],
) -> (usize, usize) {
    let needed = resampler.input_frames_next();
    assert!(
        needed <= resampler.input_frames_max(),
        "next {needed} exceeds published maximum {}",
        resampler.input_frames_max()
    );
    assert!(needed <= input[0].len());
    let valid_end = 2 * SINC_LENGTH + needed;
    audit.initialized_end.store(valid_end, Ordering::Relaxed);
    audit.largest_read_end.store(0, Ordering::Relaxed);
    audit.reads.store(0, Ordering::Relaxed);
    let exact_input = [&input[0][..needed], &input[1][..needed]];
    let expected_output = resampler.output_frames_next();
    let result = resampler
        .process_into_buffer(&exact_input, output, None)
        .unwrap();
    assert_eq!(result, (needed, expected_output));
    assert!(audit.reads.load(Ordering::Relaxed) > 0);
    assert!(audit.largest_read_end.load(Ordering::Relaxed) <= valid_end);
    for channel in output {
        assert!(
            channel[..expected_output]
                .iter()
                .all(|sample| sample.is_finite())
        );
    }
    result
}

#[test]
fn published_input_max_covers_ramps_fractional_history_and_chunk_changes() {
    let ratios = [
        1.0 / 1.004,
        1.0 / 1.001,
        1.0 / 1.0000005,
        1.0,
        1.0 / 0.9999995,
        1.0 / 0.999,
        1.004,
    ];
    for maximum_chunk in [1, 2, 127, 128, 240, 960, 4096] {
        for cubic in [false, true] {
            let interpolation = if cubic {
                SincInterpolationType::Cubic
            } else {
                SincInterpolationType::Linear
            };
            let (mut resampler, audit) = make(maximum_chunk, interpolation);
            let maximum_input = resampler.input_frames_max();
            let input = [vec![0.25; maximum_input], vec![-0.5; maximum_input]];
            let mut output = [vec![0.0; maximum_chunk], vec![0.0; maximum_chunk]];
            process(&mut resampler, &audit, &input, &mut output);
            for iteration in 0..196 {
                let chunk = [1, maximum_chunk.min(240), maximum_chunk][iteration % 3];
                resampler.set_chunk_size(chunk).unwrap();
                let ratio = ratios[(iteration * 3) % ratios.len()];
                resampler
                    .set_resample_ratio_relative(ratio, iteration % 4 != 0)
                    .unwrap();
                assert_eq!(resampler.input_frames_max(), maximum_input);
                process(&mut resampler, &audit, &input, &mut output);
                if iteration % 31 == 0 {
                    resampler.reset();
                    assert_eq!(resampler.output_frames_next(), maximum_chunk);
                    process(&mut resampler, &audit, &input, &mut output);
                }
            }
        }
    }
}

#[test]
fn constructor_and_reset_have_identical_counts_phase_and_empty_history() {
    for chunk in [1, 128, 240, 4096] {
        let (mut used, used_audit) = make(chunk, SincInterpolationType::Linear);
        let (mut fresh, fresh_audit) = make(chunk, SincInterpolationType::Linear);
        let initial_needed = fresh.input_frames_next();
        let maximum = fresh.input_frames_max();
        let mut input = [vec![1.0; maximum], vec![-0.5; maximum]];
        let mut used_output = [vec![0.0; chunk], vec![0.0; chunk]];
        let mut fresh_output = used_output.clone();
        for iteration in 0..128 {
            used.set_resample_ratio_relative(
                if iteration % 2 == 0 {
                    1.0 / 1.001
                } else {
                    1.0 / 0.999
                },
                true,
            )
            .unwrap();
            process(&mut used, &used_audit, &input, &mut used_output);
        }
        used.set_chunk_size(1).unwrap();
        used.reset();
        assert_eq!(used.input_frames_next(), initial_needed);
        assert_eq!(used.input_frames_max(), fresh.input_frames_max());
        assert_eq!(used.output_frames_next(), fresh.output_frames_next());
        assert_eq!(used.output_delay(), fresh.output_delay());
        input.iter_mut().for_each(|channel| channel.fill(0.0));
        for _ in 0..4 {
            assert_eq!(
                process(&mut used, &used_audit, &input, &mut used_output),
                process(&mut fresh, &fresh_audit, &input, &mut fresh_output)
            );
            assert_eq!(used_output, fresh_output);
            assert!(used_output.iter().flatten().all(|sample| *sample == 0.0));
            assert_eq!(used.input_frames_next(), fresh.input_frames_next());
        }
    }
}

#[test]
fn sustained_one_frame_chunks_at_the_upper_ratio_remain_bounded() {
    let (mut resampler, audit) = make(1, SincInterpolationType::Linear);
    resampler
        .set_resample_ratio_relative(MAX_RELATIVE_RATIO, false)
        .unwrap();
    let input = [
        vec![0.25; resampler.input_frames_max()],
        vec![-0.5; resampler.input_frames_max()],
    ];
    let mut output = [vec![0.0; 1], vec![0.0; 1]];
    let mut consumed = 0;
    let mut zero_input_steps = 0;
    for _ in 0..40_000 {
        let (used, _) = process(&mut resampler, &audit, &input, &mut output);
        consumed += used;
        zero_input_steps += usize::from(used == 0);
    }
    assert!(zero_input_steps > 0);
    let ideal = 40_000.0 / MAX_RELATIVE_RATIO + SINC_LENGTH as f64 / 2.0;
    assert!((consumed as f64 - ideal).abs() <= 2.0);
}
