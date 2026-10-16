//! Recording-only conversion into the fixed 48 kHz stereo engine format.
//! Construction allocates; callback processing and discontinuity reset do not.

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

pub(super) const RECORDING_RATES: [u32; 3] = [48_000, 44_100, 96_000];
const INPUT_FRAMES: usize = 128;

pub(super) struct RecordingResampler {
    resampler: SincFixedIn<f32>,
    input: [Vec<f32>; 2],
    output: [Vec<f32>; 2],
    interleaved: Vec<f32>,
    channels: u16,
    filled: usize,
    partial_left: Option<f32>,
}

impl RecordingResampler {
    pub(super) fn new(rate: u32, channels: u16) -> Option<Self> {
        if !matches!(rate, 44_100 | 96_000) || !matches!(channels, 1 | 2) {
            return None;
        }
        let resampler = SincFixedIn::new(
            48_000.0 / f64::from(rate),
            1.0,
            SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Cubic,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            },
            INPUT_FRAMES,
            2,
        )
        .ok()?;
        let max_output = resampler.output_frames_max();
        Some(Self {
            resampler,
            input: std::array::from_fn(|_| vec![0.0; INPUT_FRAMES]),
            output: std::array::from_fn(|_| vec![0.0; max_output]),
            interleaved: vec![0.0; max_output * 2],
            channels,
            filled: 0,
            partial_left: None,
        })
    }

    pub(super) fn reset(&mut self) {
        self.resampler.reset();
        self.filled = 0;
        self.partial_left = None;
        // Staging is fully overwritten before processing the next complete block.
    }

    /// Native callback boundaries have no significance to sample timing: keep
    /// incomplete frames/blocks and never insert padding at a callback edge.
    pub(super) fn write<T: Copy>(
        &mut self,
        data: &[T],
        convert: impl Fn(T) -> f32,
        mut emit: impl FnMut(&[f32]),
    ) -> Result<bool, rubato::ResampleError> {
        let mut pre_clip = false;
        for &sample in data {
            let sample = convert(sample);
            pre_clip |= sample.is_finite() && sample.abs() >= 1.0;
            // A nonfinite value must not poison the filter's retained history.
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let frame = if self.channels == 1 {
                [sample; 2]
            } else if let Some(left) = self.partial_left.take() {
                [left, sample]
            } else {
                self.partial_left = Some(sample);
                continue;
            };
            self.input[0][self.filled] = frame[0];
            self.input[1][self.filled] = frame[1];
            self.filled += 1;
            if self.filled == INPUT_FRAMES {
                let (_, produced) =
                    self.resampler
                        .process_into_buffer(&self.input, &mut self.output, None)?;
                for (frame, (&left, &right)) in self.interleaved[..produced * 2]
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(self.output[0].iter().zip(&self.output[1]))
                {
                    *frame = [left, right];
                }
                emit(&self.interleaved[..produced * 2]);
                self.filled = 0;
            }
        }
        Ok(pre_clip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };

    struct CountingAllocator;
    thread_local! {
        static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
    }
    fn record_allocation() {
        let _ = ALLOCATIONS.try_with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value + 1));
            }
        });
    }
    // Counting is local to the test thread; unrelated parallel tests do not
    // influence the callback allocation assertion.
    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            record_allocation();
            unsafe { System.alloc(layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            record_allocation();
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            record_allocation();
            unsafe { System.realloc(ptr, layout, size) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: CountingAllocator = CountingAllocator;

    fn allocations_during(action: impl FnOnce()) -> usize {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                ALLOCATIONS.with(|count| count.set(None));
            }
        }
        ALLOCATIONS.with(|count| count.set(Some(0)));
        let _reset = Reset;
        action();
        ALLOCATIONS.with(|count| count.get().unwrap())
    }

    fn signal(rate: u32, channels: u16, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                (0..channels).map(move |channel| {
                    let frequency = if channel == 0 { 997.0 } else { 1357.0 };
                    (std::f64::consts::TAU * frequency * n as f64 / f64::from(rate)).sin() as f32
                        * 0.25
                })
            })
            .collect()
    }

    fn convert(rate: u32, channels: u16, input: &[f32], chunks: &[usize]) -> Vec<f32> {
        let mut converter = RecordingResampler::new(rate, channels).unwrap();
        let mut output = Vec::new();
        let mut offset = 0;
        for size in chunks.iter().cycle() {
            if offset == input.len() {
                break;
            }
            let end = (offset + size).min(input.len());
            converter
                .write(&input[offset..end], |s| s, |s| output.extend_from_slice(s))
                .unwrap();
            offset = end;
        }
        output
    }

    fn amplitude(stereo: &[f32], channel: usize, frequency: f64) -> f64 {
        let frames = stereo.len() / 2;
        let (sin, cos) = stereo.as_chunks::<2>().0.iter().enumerate().fold(
            (0.0, 0.0),
            |(sin, cos), (n, frame)| {
                let phase = std::f64::consts::TAU * frequency * n as f64 / 48_000.0;
                (
                    sin + f64::from(frame[channel]) * phase.sin(),
                    cos + f64::from(frame[channel]) * phase.cos(),
                )
            },
        );
        2.0 * sin.hypot(cos) / frames as f64
    }

    #[test]
    fn only_reviewed_recording_formats_are_accepted() {
        assert_eq!(RECORDING_RATES, [48_000, 44_100, 96_000]);
        for rate in [0, 8_000, 32_000, 48_000, 192_000, u32::MAX] {
            assert!(RecordingResampler::new(rate, 2).is_none());
        }
        for channels in [0, 3, 6, u16::MAX] {
            assert!(RecordingResampler::new(44_100, channels).is_none());
        }
    }

    #[test]
    fn irregular_and_split_stereo_callbacks_preserve_signal_and_frame_timing() {
        for rate in [44_100, 96_000] {
            for channels in [1, 2] {
                let source = signal(rate, channels, rate as usize);
                let contiguous = convert(rate, channels, &source, &[source.len()]);
                let fragmented = convert(rate, channels, &source, &[1, 17, 255, 3, 128, 961]);
                assert_eq!(fragmented, contiguous);
                let converter = RecordingResampler::new(rate, channels).unwrap();
                let delay = converter.resampler.output_delay();
                let staging = (INPUT_FRAMES as f64 * 48_000.0 / f64::from(rate)).ceil() as usize;
                let output_frames = fragmented.len() / 2;
                assert!(output_frames <= 48_000);
                assert!(48_000 - output_frames <= delay + staging + 2);
                assert!(delay + staging <= 280, "conversion bound is below 6 ms");
                let steady = &fragmented[4800..];
                assert!((amplitude(steady, 0, 997.0) - 0.25).abs() < 0.001);
                if channels == 1 {
                    assert!(fragmented.as_chunks::<2>().0.iter().all(|f| f[0] == f[1]));
                } else {
                    assert!((amplitude(steady, 1, 1357.0) - 0.25).abs() < 0.001);
                    assert!(amplitude(steady, 1, 997.0) < 0.001);
                    assert!(amplitude(steady, 0, 1357.0) < 0.001);
                }
            }
        }
    }

    #[test]
    fn downsampling_rejects_out_of_band_aliases() {
        let source: Vec<_> = (0..96_000)
            .map(|n| (std::f64::consts::TAU * 30_000.0 * n as f64 / 96_000.0).sin() as f32 * 0.25)
            .collect();
        let output = convert(96_000, 1, &source, &[191, 1, 256]);
        let steady = &output[4800..];
        // 30 kHz would fold to 18 kHz without a low-pass filter. Require 60 dB
        // rejection relative to the 0.25 input amplitude.
        assert!(amplitude(steady, 0, 18_000.0) < 0.00025);
        assert!(steady.iter().all(|s| s.abs() < 0.00025));
    }

    #[test]
    fn reset_discards_filter_history_staging_and_partial_stereo_frame() {
        for rate in [44_100, 96_000] {
            let mut converter = RecordingResampler::new(rate, 2).unwrap();
            converter.write(&[1.0; 2049], |s| s, |_| {}).unwrap();
            converter.reset();
            let mut frames = 0;
            converter
                .write(
                    &[0.0; 4096],
                    |s| s,
                    |data| {
                        frames += data.len() / 2;
                        assert!(data.iter().all(|s| *s == 0.0));
                    },
                )
                .unwrap();
            assert!(frames > 0);
        }
    }

    #[test]
    fn nonfinite_input_does_not_poison_later_audio() {
        let mut input = vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 100.0, -100.0];
        input.extend(signal(44_100, 1, 44_100));
        let output = convert(44_100, 1, &input, &[7, 1, 128]);
        assert!(output.iter().all(|s| s.is_finite()));
        assert!((amplitude(&output[4800..], 0, 997.0) - 0.25).abs() < 0.001);
    }

    #[test]
    fn conversion_and_discontinuity_reset_never_allocate() {
        for rate in [44_100, 96_000] {
            let mut converter = RecordingResampler::new(rate, 2).unwrap();
            let source = signal(rate, 2, rate as usize);
            let mut samples = 0;
            let allocations = allocations_during(|| {
                for chunk in source.chunks(173) {
                    converter
                        .write(chunk, |s| s, |data| samples += data.len())
                        .unwrap();
                }
                converter.reset();
                converter.write(&source, |s| s, |_| {}).unwrap();
            });
            assert!(samples > 0);
            assert_eq!(allocations, 0);
        }
    }

    #[test]
    #[ignore = "manual synthetic CPU measurement; no devices, network or acceptance timing gate"]
    fn benchmark_eight_recording_converters() {
        for rate in [44_100, 96_000] {
            let mut converters: Vec<_> = (0..8)
                .map(|_| RecordingResampler::new(rate, 2).unwrap())
                .collect();
            let source = signal(rate, 2, INPUT_FRAMES);
            let blocks = (rate as usize * 10).div_ceil(INPUT_FRAMES);
            let started = std::time::Instant::now();
            let mut samples = 0;
            for _ in 0..blocks {
                for converter in &mut converters {
                    converter
                        .write(&source, |s| s, |data| samples += data.len())
                        .unwrap();
                }
            }
            println!(
                "8 stereo converters, {rate} Hz, 10 synthetic seconds: {:.3} CPU-wall seconds, {samples} output samples",
                started.elapsed().as_secs_f64()
            );
        }
    }
}
