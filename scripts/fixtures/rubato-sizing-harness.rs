//! Tests the verified dependency's estimator and actual index/lookup loop.
//! Only sinc convolution is mocked: every requested input window is recorded.
use std::cell::Cell;

type T = f64;
trait SampleValues {
    fn zero() -> Self;
    fn coerce(value: f64) -> Self;
}
impl SampleValues for f64 {
    fn zero() -> Self {
        0.0
    }
    fn coerce(value: f64) -> Self {
        value
    }
}

// __INTERPOLATION_TYPES__
// __NEAREST_TIMES__

fn interp_cubic(_: T, points: &[T; 4]) -> T {
    points[0]
}
fn interp_quad(_: T, points: &[T; 3]) -> T {
    points[0]
}
fn interp_lin(_: T, points: &[T; 2]) -> T {
    points[0]
}

struct Output(Vec<T>);
impl AsMut<[T]> for Output {
    fn as_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

struct AuditedSinc {
    length: usize,
    maximum_exclusive: Cell<usize>,
}
impl AuditedSinc {
    fn len(&self) -> usize {
        self.length
    }
    fn nbr_sincs(&self) -> usize {
        256
    }
    fn get_sinc_interpolated(&self, wave: &[T], index: usize, subindex: usize) -> T {
        assert!(subindex < self.nbr_sincs());
        let end = index.checked_add(self.length).unwrap();
        assert!(end <= wave.len());
        self.maximum_exclusive
            .set(self.maximum_exclusive.get().max(end));
        wave[index]
    }
}

struct State {
    chunk_size: usize,
    last_index: f64,
    resample_ratio: f64,
    target_ratio: f64,
    needed_input_size: usize,
    interpolator: AuditedSinc,
    interpolation: SincInterpolationType,
    channel_mask: [bool; 1],
    buffer: [Vec<T>; 1],
}
impl State {
    fn new(
        chunk_size: usize,
        last_index: f64,
        start: f64,
        target: f64,
        interpolation: SincInterpolationType,
    ) -> Self {
        Self {
            chunk_size,
            last_index,
            resample_ratio: start,
            target_ratio: target,
            needed_input_size: 0,
            interpolator: AuditedSinc {
                length: 128,
                maximum_exclusive: Cell::new(0),
            },
            interpolation,
            channel_mask: [true],
            buffer: [vec![0.0; 6 * chunk_size + 1024]],
        }
    }

    // __PATCHED_ESTIMATE__
    // __ORIGINAL_ESTIMATE__

    fn actual_reads(&self) -> (usize, usize) {
        self.interpolator.maximum_exclusive.set(0);
        let sinc_len = self.interpolator.len();
        let oversampling_factor = self.interpolator.nbr_sincs();
        let mut wave_out = [Output(vec![0.0; self.chunk_size])];
        // __ACTUAL_INDEX_AND_LOOKUP_LOOP__
        let initialized_input_needed = self
            .interpolator
            .maximum_exclusive
            .get()
            .saturating_sub(2 * sinc_len);
        // Also retain the stricter upstream ceil-position safety contract.
        // This is measured from the actual loop, not recomputed from its formula.
        let conservative_needed = (idx + sinc_len as f64).ceil() as usize;
        (initialized_input_needed, conservative_needed)
    }

    fn assert_bounds(&mut self) {
        self.update_needed_len();
        let (read_needed, conservative_needed) = self.actual_reads();
        assert!(
            self.needed_input_size >= read_needed,
            "actual read requires {read_needed}, estimate {}, N {}, phase {}, ratios {} -> {}",
            self.needed_input_size,
            self.chunk_size,
            self.last_index,
            self.resample_ratio,
            self.target_ratio
        );
        assert!(
            self.needed_input_size >= conservative_needed,
            "stepped position requires {conservative_needed}, estimate {}, N {}, phase {}, ratios {} -> {}",
            self.needed_input_size,
            self.chunk_size,
            self.last_index,
            self.resample_ratio,
            self.target_ratio
        );
    }
}

#[test]
fn known_old_ramp_estimate_reads_beyond_initialized_input() {
    for interpolation in [SincInterpolationType::Linear, SincInterpolationType::Cubic] {
        let mut state = State::new(240, -64.0, 1.0, 0.2, interpolation);
        state.old_update_needed_len();
        let old = state.needed_input_size;
        let (actual, _) = state.actual_reads();
        assert!(
            old < actual,
            "negative control must expose a real read deficit"
        );
        state.assert_bounds();
    }
}

#[test]
fn guard_covers_increment_loop_rounding_at_an_integer_boundary() {
    let mut state = State::new(
        128,
        -127.064,
        1.0 / 1.0005,
        1.0 / 1.0005,
        SincInterpolationType::Linear,
    );
    state.update_needed_len();
    let (_, conservative) = state.actual_reads();
    assert_eq!(conservative, 130);
    assert_eq!(state.needed_input_size, 130);
    assert!(state.needed_input_size - 1 < conservative);
    state.assert_bounds();
}

#[test]
fn guard_does_not_force_input_when_history_already_covers_the_frame() {
    let mut state = State::new(1, -130.0, 1.004, 1.004, SincInterpolationType::Linear);
    state.update_needed_len();
    assert_eq!(state.actual_reads().0, 0);
    assert_eq!(state.needed_input_size, 0);
}

fn fractional_sweep(phases: usize) {
    let ratios = [
        1.0 / 1.004,
        1.0 / 1.001,
        1.0 / 1.0000005,
        1.0,
        1.0 / 0.9999995,
        1.0 / 0.999,
        1.004,
    ];
    let mut cases = 0;
    for chunk in [1, 2, 127, 128, 239, 240, 241, 480, 960, 4096] {
        for &start in &ratios {
            for &end in &ratios {
                for interpolation in [
                    SincInterpolationType::Linear,
                    SincInterpolationType::Cubic,
                    SincInterpolationType::Quadratic,
                    SincInterpolationType::Nearest,
                ] {
                    let mut state = State::new(chunk, -64.0, start, end, interpolation);
                    state.assert_bounds(); // Constructor's half-filter initial position.
                    for phase in 0..phases {
                        // Include the steady guard-shifted history range and
                        // phases on each side of exact integral boundaries.
                        state.last_index = -130.0 + 3.0 * phase as f64 / phases as f64;
                        state.assert_bounds();
                        cases += 1;
                    }
                }
            }
        }
    }
    println!("{cases} actual-loop fractional phase/read-bound cases passed");
}

#[test]
fn fractional_phases_ramps_and_maximum_chunks_are_bounded() {
    fractional_sweep(16);
}

#[test]
#[ignore = "dense source-extracted phase sweep; ECHOCORD_RUBATO_DENSE=1 runs it via the Python test"]
fn dense_fractional_phases_are_bounded() {
    fractional_sweep(1024);
}
