//! Explicit, isolated Windows playback destinations and a nonblocking bus renderer.
use crate::{
    mix_bus::Bus,
    model::{AppError, ErrorCode, PlaybackDevice},
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::SeqCst},
};

const MUTED: u64 = 1 << 32;
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
#[derive(Default)]
pub struct RenderControl {
    output: AtomicU64,
    stopped: AtomicBool,
    failed: AtomicBool,
    meter_sequence: AtomicU64,
    meter_output: AtomicU64,
    meter_peak: AtomicU32,
    meter_ms: AtomicU64,
    queued: AtomicU32,
    dropped: AtomicU64,
    underrun: AtomicU64,
    callback_frames: AtomicU32,
}
impl RenderControl {
    pub fn set_output(&self, gain: f32, muted: bool) {
        let gain = if gain.is_finite() {
            gain.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut previous = self.output.load(SeqCst);
        loop {
            let generation =
                (previous >> 33).wrapping_add(u64::from((previous & MUTED != 0) != muted));
            let next =
                (generation << 33) | u64::from(gain.to_bits()) | if muted { MUTED } else { 0 };
            match self
                .output
                .compare_exchange_weak(previous, next, SeqCst, SeqCst)
            {
                Ok(_) => break,
                Err(actual) => previous = actual,
            }
        }
    }
    pub fn stop(&self) {
        self.stopped.store(true, SeqCst);
    }
    pub fn snapshot(&self, now_ms: u64) -> RenderObservation {
        let sequence = self.meter_sequence.load(SeqCst);
        let peak = f32::from_bits(self.meter_peak.load(SeqCst));
        let output = self.meter_output.load(SeqCst);
        let measured = self.meter_ms.load(SeqCst);
        let valid = sequence != 0
            && sequence.is_multiple_of(2)
            && sequence == self.meter_sequence.load(SeqCst)
            && output == self.output.load(SeqCst)
            && !self.stopped.load(SeqCst)
            && !self.failed.load(SeqCst)
            && now_ms.saturating_sub(measured) <= 200;
        let observed_at_ms =
            crate::audio::clean_observation_time(valid.then_some(measured as f64), now_ms as f64);
        RenderObservation {
            timing: None,
            observed_at_ms,
            level_dbfs: observed_at_ms.map(|_| {
                if peak == 0.0 {
                    -96.0
                } else {
                    (20.0 * peak.log10()).max(-96.0)
                }
            }),
            queued_frames: self.queued.load(SeqCst),
            dropped_frames: self.dropped.load(SeqCst).min(u32::MAX as u64) as u32,
            underrun_frames: self.underrun.load(SeqCst).min(u32::MAX as u64) as u32,
            callback_frames: self.callback_frames.load(SeqCst),
        }
    }
}
pub struct Renderer {
    cursor: u64,
    cursor_generation: u64,
    pub control: Arc<RenderControl>,
    held_peak: f32,
    held_ms: u64,
    held_output: u64,
}
impl Renderer {
    pub fn attach(bus: &Bus, control: Arc<RenderControl>) -> Self {
        Self {
            cursor: bus.cursor(),
            cursor_generation: control.output.load(SeqCst) >> 33,
            control,
            held_peak: 0.0,
            held_ms: 0,
            held_output: 0,
        }
    }
    /// The endpoint callback owns pacing. No waits, locks, allocation or I/O occur here.
    pub fn render<T: Copy>(
        &mut self,
        bus: &Bus,
        data: &mut [T],
        now_ms: u64,
        convert: impl Fn(f32) -> T,
    ) {
        if data.is_empty() {
            return;
        }
        let output = self.control.output.load(SeqCst);
        let reset = self.cursor_generation != output >> 33;
        if reset {
            self.cursor = bus.cursor();
            self.cursor_generation = output >> 33;
        }
        let gain = if output & MUTED != 0
            || self.control.stopped.load(SeqCst)
            || self.control.failed.load(SeqCst)
        {
            0.0
        } else {
            f32::from_bits(output as u32)
        };
        let mut peak = 0.0_f32;
        let mut frames = [[0.0; 2]; 240];
        for chunk in data.chunks_mut(480) {
            let count = chunk.len() / 2;
            let stats = bus.read_counted(&mut self.cursor, &mut frames[..count], now_ms);
            self.control.queued.store(stats.queued_frames, SeqCst);
            self.control
                .dropped
                .fetch_add(u64::from(stats.dropped_frames), SeqCst);
            if !reset && output & MUTED == 0 {
                self.control
                    .underrun
                    .fetch_add(u64::from(stats.underrun_frames), SeqCst);
            }
            for (out, frame) in chunk
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .zip(frames[..count].iter())
            {
                for channel in 0..2 {
                    let sample = frame[channel] * gain;
                    out[channel] = convert(sample);
                    peak = peak.max(sample.abs());
                }
            }
            if chunk.len() % 2 != 0 {
                *chunk.last_mut().expect("odd chunk") = convert(0.0);
            }
        }
        self.control
            .callback_frames
            .fetch_max((data.len() / 2).min(u32::MAX as usize) as u32, SeqCst);
        if output != self.held_output || now_ms.saturating_sub(self.held_ms) >= 100 {
            self.held_peak = peak;
            self.held_ms = now_ms;
            self.held_output = output;
        } else {
            self.held_peak = self.held_peak.max(peak);
        }
        self.control.meter_sequence.fetch_add(1, SeqCst);
        self.control
            .meter_peak
            .store(self.held_peak.to_bits(), SeqCst);
        self.control.meter_output.store(output, SeqCst);
        self.control.meter_ms.store(now_ms, SeqCst);
        self.control.meter_sequence.fetch_add(1, SeqCst);
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
    #[test]
    fn callback_timestamp_is_not_refreshed_by_status_polling() {
        let bus = Bus::test_bus();
        let control = Arc::new(RenderControl::default());
        control.set_output(1.0, false);
        let mut renderer = Renderer::attach(&bus, control.clone());
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
        let a = Arc::new(RenderControl::default());
        a.set_output(0.125, false);
        let b = Arc::new(RenderControl::default());
        b.set_output(0.5, false);
        let mut first = Renderer::attach(&bus, a.clone());
        let mut second = Renderer::attach(&bus, b.clone());
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
        let control = Arc::new(RenderControl::default());
        control.set_output(1.0, false);
        let mut renderer = Renderer::attach(&bus, control.clone());
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
        let slow = Arc::new(RenderControl::default());
        slow.set_output(1.0, false);
        let fast = Arc::new(RenderControl::default());
        fast.set_output(1.0, false);
        let mut first = Renderer::attach(&bus, slow.clone());
        let mut second = Renderer::attach(&bus, fast.clone());
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
