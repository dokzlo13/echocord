//! Explicitly selected Windows capture validation; no playback or network.
//!
//! --list emits source descriptors as JSON for caller-owned private output.
//! --source-id ID emits generic counters and local Read timing only. It never
//! prints the selection, sample values, descriptions, or raw backend errors.

use std::{ffi::OsString, io};

#[cfg(windows)]
#[path = "capture_probe/application_suite.rs"]
mod application_suite;

const USAGE: &str = "Usage: capture-probe --list | --source-id ID | --application-suite | --mixing-suite | --playback-list | --playback-suite ENDPOINT_ID | --browser-suite FIREFOX_EXE OWNED_PROFILE FIRST_SYNTHETIC_URL SECOND_SYNTHETIC_URL | --help";
type ProbeResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Help,
    List,
    Source(String),
}

fn parse(mut args: impl Iterator<Item = OsString>) -> io::Result<Mode> {
    let argument = args.next().ok_or_else(|| io::Error::other(USAGE))?;
    let mode = match argument.to_str() {
        Some("--help") => Mode::Help,
        Some("--list") => Mode::List,
        Some("--source-id") => {
            let id = args
                .next()
                .and_then(|arg| arg.into_string().ok())
                .filter(|id| !id.is_empty() && !id.starts_with("--"))
                .ok_or_else(|| io::Error::other(USAGE))?;
            Mode::Source(id)
        }
        _ => return Err(io::Error::other(USAGE)),
    };
    if args.next().is_some() {
        return Err(io::Error::other(USAGE));
    }
    Ok(mode)
}

#[cfg(any(windows, test))]
mod measurement {
    use serde::Serialize;
    use std::{
        io::{self, Read},
        time::{Duration, Instant},
    };

    pub const FRAMES_PER_PACKET: usize = 960;
    pub const FRAME_BYTES: usize = 8;
    pub const PACKET_BYTES: usize = FRAMES_PER_PACKET * FRAME_BYTES;
    const MAX_READ_BYTES: usize = 240 * FRAME_BYTES;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Timing {
        pub min_ms: f64,
        pub p50_ms: f64,
        pub p95_ms: f64,
        pub max_ms: f64,
    }

    impl Timing {
        pub fn summarize(mut durations: Vec<Duration>) -> Self {
            // Every measured stage has a positive fixed packet count.
            durations.sort_unstable();
            let percentile = |percent: usize| {
                durations[(durations.len() * percent).div_ceil(100) - 1].as_secs_f64() * 1_000.0
            };
            Self {
                min_ms: durations[0].as_secs_f64() * 1_000.0,
                p50_ms: percentile(50),
                p95_ms: percentile(95),
                max_ms: durations[durations.len() - 1].as_secs_f64() * 1_000.0,
            }
        }
    }

    #[derive(Default, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Samples {
        pub frames: usize,
        pub nonzero_frames: usize,
        pub differing_channel_frames: usize,
    }

    impl Samples {
        pub fn inspect(&mut self, bytes: &[u8], muted: bool) -> io::Result<()> {
            let (frames, trailing) = bytes.as_chunks::<FRAME_BYTES>();
            if !trailing.is_empty() {
                return Err(io::Error::other(
                    "PCM is not aligned to complete stereo frames.",
                ));
            }
            for frame in frames {
                let left = f32::from_le_bytes(frame[..4].try_into().unwrap());
                let right = f32::from_le_bytes(frame[4..].try_into().unwrap());
                if !left.is_finite() || !right.is_finite() {
                    return Err(io::Error::other("PCM contains a non-finite sample."));
                }
                if left.abs() > 1.0 || right.abs() > 1.0 {
                    return Err(io::Error::other("PCM exceeds the supported sample range."));
                }
                if muted && (left != 0.0 || right != 0.0) {
                    return Err(io::Error::other("Muted output contains nonzero PCM."));
                }
                self.frames += 1;
                self.nonzero_frames += usize::from(left != 0.0 || right != 0.0);
                self.differing_channel_frames += usize::from(left != right);
            }
            Ok(())
        }
    }

    // Keep the real caller's repeated short-read behavior visible. read_exact
    // would hide individual read wait times inside a 20 ms packet operation.
    pub fn read_packet(
        input: &mut impl Read,
        packet: &mut [u8; PACKET_BYTES],
        read_times: &mut Vec<Duration>,
    ) -> io::Result<()> {
        let mut filled = 0;
        while filled < packet.len() {
            let start = Instant::now();
            let n = input.read(&mut packet[filled..])?;
            read_times.push(start.elapsed());
            if n == 0 {
                return Err(io::Error::other(
                    "Capture returned EOF before explicit stop.",
                ));
            }
            if n > MAX_READ_BYTES || n > packet.len() - filled || n % FRAME_BYTES != 0 {
                return Err(io::Error::other(
                    "Capture read violated its stereo/short-read bound.",
                ));
            }
            filled += n;
        }
        Ok(())
    }
}

#[cfg(windows)]
mod native {
    use super::{ProbeResult, measurement::*};
    use echocord_core::{
        audio::{self, CaptureControl, CaptureSession},
        model::{Capture, CapturePhase},
    };
    use serde::Serialize;
    use std::{
        io::{self, Read},
        time::{Duration, Instant},
    };

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Counters {
        phase: CapturePhase,
        dropped_frames: u32,
        underrun_frames: u32,
        queued_frames: u32,
    }

    impl From<Capture> for Counters {
        fn from(snapshot: Capture) -> Self {
            Self {
                phase: snapshot.phase,
                dropped_frames: snapshot.dropped_frames,
                underrun_frames: snapshot.underrun_frames,
                queued_frames: snapshot.queued_frames,
            }
        }
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Stage {
        packets: usize,
        nominal_pcm_ms: f64,
        wall_ms: f64,
        wall_minus_nominal_pcm_ms: f64,
        read_calls: usize,
        read_call_timing: Timing,
        packet_20ms_fill_timing: Timing,
        packet_fills_over_20ms: usize,
        samples: Samples,
        capture_callback_seen: bool,
        max_queued_frames: u32,
        counters: Counters,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Muted {
        consumer_pause_ms: f64,
        queued_frames_before_mute: u32,
        output_zero: bool,
        queue_drained: bool,
        stage: Stage,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Pass {
        open_ms: f64,
        live: Stage,
        muted: Option<Muted>,
        stop_ms: f64,
        eof_read_ms: f64,
        stop_eof: bool,
        stop_counters: Counters,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Report {
        sample_rate: u32,
        channels: u32,
        timing_boundary: &'static str,
        pacing_policy: &'static str,
        initial: Pass,
        reopened: Pass,
        reopened_same_selection: bool,
        functional_checks_passed: bool,
    }

    struct StopOnDrop(CaptureControl);

    impl Drop for StopOnDrop {
        fn drop(&mut self) {
            self.0.stop();
        }
    }

    fn check(condition: bool, message: &'static str) -> io::Result<()> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message))
        }
    }

    fn measure(session: &mut CaptureSession, packets: usize, muted: bool) -> io::Result<Stage> {
        let mut packet = [0; PACKET_BYTES];
        // Counts are bounded by the fixed stage frame count: even an 8-byte
        // short read cannot create more timing entries than stereo frames.
        let mut read_times = Vec::with_capacity(packets * 4);
        let mut packet_times = Vec::with_capacity(packets);
        let mut samples = Samples::default();
        let mut callback_seen = false;
        let mut max_queued_frames = 0;
        let start = Instant::now();
        for _ in 0..packets {
            let packet_start = Instant::now();
            read_packet(&mut session.input, &mut packet, &mut read_times)?;
            packet_times.push(packet_start.elapsed());
            samples.inspect(&packet, muted)?;
            let snapshot = session.control.snapshot();
            check(
                snapshot.phase == CapturePhase::Running,
                "Capture is no longer running.",
            )?;
            callback_seen |= snapshot.level_dbfs.is_some();
            max_queued_frames = max_queued_frames.max(snapshot.queued_frames);
        }
        let wall_ms = start.elapsed().as_secs_f64() * 1_000.0;
        let nominal_pcm_ms = samples.frames as f64 * 1_000.0 / f64::from(audio::SAMPLE_RATE);
        let over_20ms = packet_times
            .iter()
            .filter(|&&t| t > Duration::from_millis(20))
            .count();
        Ok(Stage {
            packets,
            nominal_pcm_ms,
            wall_ms,
            wall_minus_nominal_pcm_ms: wall_ms - nominal_pcm_ms,
            read_calls: read_times.len(),
            read_call_timing: Timing::summarize(read_times),
            packet_20ms_fill_timing: Timing::summarize(packet_times),
            packet_fills_over_20ms: over_20ms,
            samples,
            capture_callback_seen: callback_seen,
            max_queued_frames,
            counters: session.control.snapshot().into(),
        })
    }

    fn capture_pass(source_id: &str, packets: usize, test_mute: bool) -> ProbeResult<Pass> {
        let open_start = Instant::now();
        let mut session = audio::start_capture(source_id)?;
        let open_ms = open_start.elapsed().as_secs_f64() * 1_000.0;
        let _stop_on_error = StopOnDrop(session.control.clone());
        let live = measure(&mut session, packets, false)?;
        check(
            live.capture_callback_seen,
            "No native capture callback was observed.",
        )?;

        let muted = if test_mute {
            // Create an opportunity for real queued frames before muting; report
            // the observed occupancy rather than claiming a nonempty queue.
            let pause = Instant::now();
            std::thread::sleep(Duration::from_millis(25));
            let pause_ms = pause.elapsed().as_secs_f64() * 1_000.0;
            let queued_before = session.control.snapshot().queued_frames;
            session.control.set_output(100, true);
            let stage = measure(&mut session, 5, true)?;
            check(
                stage.counters.queued_frames == 0,
                "Mute did not drain queued frames.",
            )?;
            Some(Muted {
                consumer_pause_ms: pause_ms,
                queued_frames_before_mute: queued_before,
                output_zero: true,
                queue_drained: true,
                stage,
            })
        } else {
            None
        };

        let stop_start = Instant::now();
        session.control.stop();
        let stop_ms = stop_start.elapsed().as_secs_f64() * 1_000.0;
        let eof_start = Instant::now();
        for _ in 0..2 {
            check(
                session.input.read(&mut [0; FRAME_BYTES])? == 0,
                "Stopped capture did not return EOF.",
            )?;
        }
        let eof_read_ms = eof_start.elapsed().as_secs_f64() * 1_000.0;
        let stopped = session.control.snapshot();
        check(
            stopped.phase == CapturePhase::Stopped && stopped.queued_frames == 0,
            "Capture stop state is inconsistent.",
        )?;
        Ok(Pass {
            open_ms,
            live,
            muted,
            stop_ms,
            eof_read_ms,
            stop_eof: true,
            stop_counters: stopped.into(),
        })
    }

    pub fn run(source_id: &str) -> ProbeResult<()> {
        let initial = capture_pass(source_id, 50, true)?;
        // The old native stream has been stopped and all session handles dropped
        // before the exact same caller-provided ID is reopened. No new selection.
        let reopened = capture_pass(source_id, 10, false)?;
        let report = Report {
            sample_rate: audio::SAMPLE_RATE,
            channels: audio::CHANNELS,
            timing_boundary: "Local PCM Read completion, not pipeline or listener latency.",
            pacing_policy: "Report wall time and 20 ms packet fill timing; no timing pass/fail threshold.",
            initial,
            reopened,
            reopened_same_selection: true,
            functional_checks_passed: true,
        };
        let mut out = io::stdout().lock();
        serde_json::to_writer(&mut out, &report)?;
        use io::Write;
        out.write_all(b"\n")?;
        Ok(())
    }
}

fn run() -> ProbeResult<()> {
    #[cfg(windows)]
    if let Some(result) = application_suite::dispatch() {
        return result;
    }
    let mode = parse(std::env::args_os().skip(1))?;
    if mode == Mode::Help {
        println!("{USAGE}");
        return Ok(());
    }
    #[cfg(windows)]
    {
        match mode {
            Mode::List => {
                let sources = echocord_core::audio::enumerate_sources()?;
                let mut out = io::stdout().lock();
                serde_json::to_writer(&mut out, &sources)?;
                use io::Write;
                out.write_all(b"\n")?;
                Ok(())
            }
            Mode::Source(id) => native::run(&id),
            Mode::Help => unreachable!(),
        }
    }
    #[cfg(not(windows))]
    Err(io::Error::other("capture-probe requires native Windows execution.").into())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("capture-probe: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{measurement::*, *};
    use std::{io::Read, time::Duration};

    fn args(args: &[&str]) -> impl Iterator<Item = OsString> {
        args.iter().map(OsString::from)
    }

    #[test]
    fn cli_requires_exact_explicit_modes_without_echoing_arguments() {
        assert_eq!(parse(args(&["--list"])).unwrap(), Mode::List);
        assert_eq!(parse(args(&["--help"])).unwrap(), Mode::Help);
        assert_eq!(
            parse(args(&["--source-id", "synthetic recording ID"])).unwrap(),
            Mode::Source("synthetic recording ID".into())
        );
        for invalid in [
            &[][..],
            &["--source-id"][..],
            &["--source-id", ""][..],
            &["--list", "synthetic-private-value"][..],
            &["--source-id", "--list"][..],
            &["synthetic-private-value"][..],
        ] {
            let message = parse(args(invalid)).unwrap_err().to_string();
            assert_eq!(message, USAGE);
            assert!(!message.contains("synthetic-private-value"));
        }
    }

    struct ShortReader {
        calls: usize,
        bytes: usize,
    }

    impl Read for ShortReader {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            let count = self.bytes.min(out.len());
            out[..count].fill(0);
            Ok(count)
        }
    }

    #[test]
    fn packet_measurement_reads_every_short_block_and_rejects_early_eof() {
        let mut reader = ShortReader {
            calls: 0,
            bytes: 240 * FRAME_BYTES,
        };
        let mut timings = Vec::new();
        read_packet(&mut reader, &mut [0; PACKET_BYTES], &mut timings).unwrap();
        assert_eq!(reader.calls, 4);
        assert_eq!(timings.len(), 4);
        let mut eof = io::empty();
        assert!(read_packet(&mut eof, &mut [0; PACKET_BYTES], &mut timings).is_err());
    }

    #[test]
    fn packet_measurement_rejects_unaligned_or_uncapped_readers() {
        for bytes in [7, PACKET_BYTES] {
            let mut reader = ShortReader { calls: 0, bytes };
            assert!(read_packet(&mut reader, &mut [0; PACKET_BYTES], &mut Vec::new()).is_err());
        }
    }

    #[test]
    fn sample_validation_accepts_silence_and_rejects_invalid_or_audible_muted_pcm() {
        let mut samples = Samples::default();
        samples.inspect(&[0; FRAME_BYTES], true).unwrap();
        assert_eq!(samples.frames, 1);
        assert_eq!(samples.nonzero_frames, 0);
        let stereo = [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat();
        samples.inspect(&stereo, false).unwrap();
        assert_eq!(samples.nonzero_frames, 1);
        assert_eq!(samples.differing_channel_frames, 1);
        assert!(samples.inspect(&stereo, true).is_err());
        for invalid in [f32::NAN, f32::INFINITY, -2.0, 2.0] {
            assert!(
                samples
                    .inspect(
                        &[invalid.to_le_bytes(), 0.0_f32.to_le_bytes()].concat(),
                        false
                    )
                    .is_err()
            );
        }
        assert!(samples.inspect(&[0; 7], false).is_err());
    }

    #[test]
    fn timings_summarize_real_durations_as_local_milliseconds() {
        let timing = Timing::summarize([5, 1, 3, 2, 4].map(Duration::from_millis).to_vec());
        assert_eq!(timing.min_ms, 1.0);
        assert_eq!(timing.p50_ms, 3.0);
        assert_eq!(timing.p95_ms, 5.0);
        assert_eq!(timing.max_ms, 5.0);
    }
}
