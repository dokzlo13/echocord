use super::*;
use crate::audio::recording_format::tests::allocations_during;
use std::cell::Cell;

const START_NS: u64 = 1_000_000_000;
fn control(enabled: bool) -> Arc<RenderControl> {
    Arc::new(RenderControl::new(
        7,
        &PlaybackControls::new(
            Output {
                volume_linear: Some(1.0),
                volume_percent: 100,
                muted: false,
            },
            enabled,
        ),
    ))
}
fn tick(bus: &Bus, renderer: &mut Renderer, output: &mut [f32], now: u64) {
    renderer.render_clocked(
        bus,
        output,
        now,
        Some(now + 10_000_000),
        48_000,
        || now,
        |v| v,
    );
}
fn write_until(bus: &Bus, next: &mut u64, now: u64) {
    while *next <= now {
        bus.write(&[[0.125, -0.25]; 240], *next / 1_000_000);
        *next += 5_000_000;
    }
}
fn warm(bus: &Bus, renderer: &mut Renderer, now: &mut u64, next: &mut u64) {
    let mut output = [0.0; 960];
    for _ in 0..20 {
        *now += 10_000_000;
        write_until(bus, next, *now);
        tick(bus, renderer, &mut output, *now);
    }
    assert_eq!(renderer.timing.phase, TimingPhase::Running);
    assert!(
        output
            .as_chunks::<2>()
            .0
            .iter()
            .all(|f| (f[0] - 0.125).abs() < 0.002 && (f[1] + 0.25).abs() < 0.004)
    );
}

#[derive(Debug)]
struct ResultStats {
    gaps: u64,
    drops: u64,
    resets: u64,
    ppm: f64,
    max_queue: u32,
    callbacks: usize,
}
fn simulate(ppm: f64, sizes: &[usize], seconds: u64, jitter: bool, enabled: bool) -> ResultStats {
    let bus = Bus::test_bus();
    let control = control(enabled);
    let mut renderer = Renderer::attach(&bus, control.clone()).unwrap();
    let mut output = vec![0.0; sizes.iter().max().unwrap() * 2];
    let mut producer = START_NS + 5_000_000;
    let mut position = 0_u64;
    let mut callbacks = 0;
    let mut max_queue = 0;
    let mut settled = None;
    loop {
        let device_ns = START_NS + (position as f64 * 1e9 / (48_000.0 * (1.0 + ppm / 1e6))) as u64;
        let jitter_ns = if jitter {
            [0_i64, 2_000_000, -1_000_000, 1_000_000, -2_000_000][callbacks % 5]
        } else {
            0
        };
        let now = (device_ns as i64 + 10_000_000 + jitter_ns) as u64;
        if now > START_NS + seconds * 1_000_000_000 {
            break;
        }
        write_until(&bus, &mut producer, now);
        let frames = sizes[callbacks % sizes.len()];
        renderer.render_clocked(
            &bus,
            &mut output[..frames * 2],
            now,
            Some(device_ns + 30_000_000),
            48_000,
            || now,
            |v| v,
        );
        assert!(!control.failed.load(SeqCst));
        assert_eq!(renderer.native_position, position + frames as u64);
        max_queue = max_queue.max(renderer.timing.queued.unwrap_or(0));
        assert!(renderer.timing.queued.unwrap_or(0) <= if enabled { 4096 } else { 1920 });
        assert!(
            output[..frames * 2]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1.0)
        );
        if enabled && now > START_NS + 500_000_000 {
            let counters = (
                renderer.timing.gaps,
                renderer.timing.dropped,
                renderer.timing.resyncs,
            );
            assert_eq!(
                *settled.get_or_insert(counters),
                counters,
                "no recurring trim or gaps after startup settles"
            );
            assert_eq!(
                renderer.timing.phase,
                TimingPhase::Running,
                "ppm={ppm} sizes={sizes:?} time={} timing={:?}",
                now - START_NS,
                renderer.timing
            );
            assert!(
                output[..frames * 2]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|f| (f[0] - 0.125).abs() < 0.002 && (f[1] + 0.25).abs() < 0.004),
                "ppm={ppm} sizes={sizes:?} frames={frames} time={} firstbad={:?} timing={:?}",
                now - START_NS,
                output[..frames * 2]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .enumerate()
                    .find(|(_, f)| (f[0] - 0.125).abs() >= 0.002 || (f[1] + 0.25).abs() >= 0.004),
                renderer.timing
            );
        }
        callbacks += 1;
        position += frames as u64;
    }
    let measured = control.snapshot((START_NS + seconds * 1_000_000_000) / 1_000_000);
    let timing = measured.timing.unwrap();
    assert_eq!(measured.queued_frames, timing.queued_frames.unwrap());
    assert_eq!(
        u64::from(measured.dropped_frames),
        timing.dropped_input_frames
    );
    assert_eq!(
        u64::from(measured.underrun_frames),
        timing.gap_output_frames
    );
    ResultStats {
        gaps: timing.gap_output_frames,
        drops: timing.dropped_input_frames,
        resets: timing.resync_count,
        ppm: timing.applied_adjustment_ppm.unwrap_or(0.0),
        max_queue,
        callbacks,
    }
}

#[test]
fn real_bus_and_sinc_track_independent_playback_clocks_without_gaps() {
    for ppm in [-500.0, -100.0, 100.0, 500.0] {
        let result = simulate(ppm, &[480], 15, true, true);
        assert_eq!(
            (result.gaps, result.drops, result.resets),
            (0, 0, 0),
            "{result:?}"
        );
        assert!(result.ppm.abs() <= 1000.01);
        assert!(result.callbacks > 1400 && result.max_queue > 0);
    }
}

#[test]
fn actual_variable_five_to_thirty_ms_callbacks_are_admitted_as_complete_blocks() {
    for sizes in [&[1440][..], &[240, 480, 960, 1440][..]] {
        let result = simulate(500.0, sizes, 8, true, true);
        assert!(
            result.gaps <= 1440 && result.drops <= 4096 && result.resets <= 1,
            "only one bounded startup trim may be required: {result:?}"
        );
    }
}

#[test]
#[ignore = "600 virtual seconds per rate; real bus + sinc, no devices or network"]
fn measure_ten_minute_playback_clock_correction() {
    for ppm in [-500.0, -100.0, 100.0, 500.0] {
        let result = simulate(ppm, &[480], 600, true, true);
        println!("playback {ppm:+} ppm, 600 virtual seconds: {result:?}");
        assert_eq!((result.gaps, result.drops, result.resets), (0, 0, 0));
        assert!(
            (result.ppm + ppm).abs() < 100.0,
            "actual consumption corrects the sink's clock in the opposite direction"
        );
        let legacy = simulate(ppm, &[480], 600, false, false);
        println!("legacy playback {ppm:+} ppm, 600 virtual seconds: {legacy:?}");
        if ppm > 0.0 {
            assert!(legacy.gaps > 0);
        } else {
            assert!(legacy.drops > 0);
        }
    }
}

#[test]
fn initial_oversized_fill_recovers_but_recurring_or_alternating_oversizes_fault() {
    let bus = Bus::test_bus();
    let c = control(true);
    let mut r = Renderer::attach(&bus, c.clone()).unwrap();
    let mut large = [9.0; 10_000];
    bus.write(&[[0.125, -0.25]; 240], 1005);
    tick(&bus, &mut r, &mut large, START_NS + 5_000_000);
    assert!(large.iter().all(|v| *v == 0.0));
    assert!(!c.failed.load(SeqCst));
    let mut now = START_NS + 5_000_000;
    let mut next = now + 5_000_000;
    warm(&bus, &mut r, &mut now, &mut next);
    assert_eq!(r.timing.resyncs, 0);
    for _ in 0..3 {
        now += 10_000_000;
        write_until(&bus, &mut next, now);
        tick(&bus, &mut r, &mut large[..2882], now);
    }
    assert!(c.failed.load(SeqCst) && c.correction_failed.load(SeqCst));
    let c = control(true);
    let mut r = Renderer::attach(&bus, c.clone()).unwrap();
    for i in 0..16 {
        now += 10_000_000;
        write_until(&bus, &mut next, now);
        tick(
            &bus,
            &mut r,
            &mut large[..if i % 2 == 0 { 4000 } else { 960 }],
            now,
        );
    }
    assert!(
        c.failed.load(SeqCst),
        "alternating oversizes must not remain Running with zeros forever"
    );
}

#[test]
fn off_uses_the_original_1920_frame_window_and_has_no_correction_ratio() {
    let bus = Bus::test_bus();
    let on = control(true);
    let off = control(false);
    let mut a = Renderer::attach(&bus, on.clone()).unwrap();
    let mut b = Renderer::attach(&bus, off.clone()).unwrap();
    bus.write(&[[0.125, -0.25]; 3000], 1000);
    let mut out = [0.0; 960];
    tick(&bus, &mut a, &mut out, START_NS);
    tick(&bus, &mut b, &mut out, START_NS);
    let enabled = on.snapshot(1000).timing.unwrap();
    let disabled = off.snapshot(1000).timing.unwrap();
    assert_eq!(enabled.capacity_frames, 4096);
    assert_eq!(enabled.dropped_input_frames, 0);
    assert_eq!(disabled.capacity_frames, 1920);
    assert_eq!(disabled.dropped_input_frames, 1080);
    assert_eq!(disabled.phase, TimingPhase::Off);
    assert_eq!(
        (disabled.applied_adjustment_ppm, disabled.target_frames),
        (None, None)
    );
    tick(&bus, &mut b, &mut out, START_NS + 41_000_000);
    assert_eq!(out, [0.0; 960]);
}

#[test]
fn long_producer_outage_keeps_counter_observations_fresh_without_old_amplitude_or_ratio() {
    let bus = Bus::test_bus();
    let c = control(true);
    let mut r = Renderer::attach(&bus, c.clone()).unwrap();
    let mut now = START_NS;
    let mut next = now + 5_000_000;
    warm(&bus, &mut r, &mut now, &mut next);
    let mut out = [0.0; 960];
    for i in 0..130 {
        now += 10_000_000;
        tick(&bus, &mut r, &mut out, now);
        let observation = c.snapshot(now / 1_000_000);
        let t = observation.timing.unwrap();
        assert_eq!(t.audio_observed_at_ms, Some((now / 1_000_000) as f64));
        assert_eq!(t.state_observed_at_ms, t.audio_observed_at_ms);
        if i > 8 {
            assert_eq!(t.applied_adjustment_ppm, None);
            assert_eq!(observation.level_dbfs, None);
        }
    }
    assert!(r.timing.gaps > 48_000);
    let before = r.timing.gaps;
    // A poll cannot make the last callback's counters look new.
    assert_eq!(
        c.snapshot(now / 1_000_000 + 500)
            .timing
            .unwrap()
            .audio_observed_at_ms,
        Some((now / 1_000_000) as f64)
    );
    next = now + 5_000_000;
    warm(&bus, &mut r, &mut now, &mut next);
    assert!(r.timing.gaps >= before);
    assert_eq!(r.timing.resyncs, 1);
}

#[test]
fn paused_reader_and_pauses_during_read_dsp_or_handoff_cannot_replay_queued_history() {
    for boundary in 0..4 {
        let bus = Bus::test_bus();
        let c = control(true);
        let mut r = Renderer::attach(&bus, c.clone()).unwrap();
        let mut now = START_NS;
        let mut next = now + 5_000_000;
        warm(&bus, &mut r, &mut now, &mut next);
        let mut out = [9.0; 960];
        if boundary == 0 {
            now += 70_000_000;
            write_until(&bus, &mut next, now);
            tick(&bus, &mut r, &mut out, now);
        } else {
            now += 10_000_000;
            write_until(&bus, &mut next, now);
            let calls = Cell::new(0);
            r.render_clocked(
                &bus,
                &mut out,
                now,
                Some(now + 10_000_000),
                48_000,
                || {
                    let call = calls.get() + 1;
                    calls.set(call);
                    now + if call >= boundary { 41_000_000 } else { 0 }
                },
                |v| v,
            );
        }
        assert_eq!(out, [0.0; 960]);
        assert_eq!(r.timing.applied, None);
        assert_eq!(r.timing.resyncs, 1);
        assert_eq!(
            r.timing.gaps, 480,
            "one discarded callback must not be counted twice"
        );
        now += 50_000_000;
        next = now + 5_000_000;
        warm(&bus, &mut r, &mut now, &mut next);
    }
}

#[test]
fn coalesced_mode_and_mute_edges_fence_reports_and_filters_but_gain_does_not_flush() {
    let bus = Bus::test_bus();
    let c = control(true);
    let mut r = Renderer::attach(&bus, c.clone()).unwrap();
    let mut now = START_NS;
    let mut next = now + 5_000_000;
    warm(&bus, &mut r, &mut now, &mut next);
    let original = c.snapshot(now / 1_000_000).timing.unwrap().epoch;
    let mut desired = PlaybackControls::new(
        Output {
            volume_linear: Some(1.0),
            volume_percent: 100,
            muted: false,
        },
        true,
    );
    desired.update(desired.output.clone(), false);
    desired.update(desired.output.clone(), true);
    desired.update(
        Output {
            muted: true,
            ..desired.output.clone()
        },
        true,
    );
    desired.update(
        Output {
            muted: false,
            ..desired.output.clone()
        },
        true,
    );
    c.apply(&desired);
    assert!(c.snapshot(now / 1_000_000).timing.is_none());
    now += 10_000_000;
    write_until(&bus, &mut next, now);
    let mut out = [9.0; 960];
    tick(&bus, &mut r, &mut out, now);
    assert_eq!(out, [0.0; 960]);
    assert_eq!(r.timing.epoch, [7, 2, 2]);
    assert_ne!(c.snapshot(now / 1_000_000).timing.unwrap().epoch, original);
    assert_eq!(
        (r.timing.gaps, r.timing.dropped, r.timing.resyncs),
        (0, 0, 0)
    );
    warm(&bus, &mut r, &mut now, &mut next);
    let epoch = r.timing.epoch;
    let cursor = r.cursor;
    c.set_output(0.5, false);
    now += 10_000_000;
    write_until(&bus, &mut next, now);
    tick(&bus, &mut r, &mut out, now);
    assert_eq!(r.timing.epoch, epoch);
    assert!(r.cursor > cursor && r.link.primed());
    assert!(
        out.as_chunks::<2>()
            .0
            .iter()
            .all(|f| (f[0] - 0.0625).abs() < 0.001)
    );
    assert_eq!(r.timing.resyncs, 0);
}

#[test]
fn concurrent_mute_edges_or_stop_before_handoff_zero_the_whole_callback() {
    for stop in [false, true] {
        let bus = Bus::test_bus();
        let c = control(true);
        let mut r = Renderer::attach(&bus, c.clone()).unwrap();
        let mut now = START_NS;
        let mut next = now + 5_000_000;
        warm(&bus, &mut r, &mut now, &mut next);
        now += 10_000_000;
        write_until(&bus, &mut next, now);
        let calls = Cell::new(0);
        let mut out = [0.0; 960];
        r.render_clocked(
            &bus,
            &mut out,
            now,
            Some(now + 10_000_000),
            48_000,
            || now,
            |v| {
                calls.set(calls.get() + 1);
                if calls.get() == 31 {
                    if stop {
                        c.stop();
                    } else {
                        c.set_output(1.0, true);
                        c.set_output(1.0, false);
                    }
                }
                v
            },
        );
        assert_eq!(out, [0.0; 960]);
        assert_eq!(c.snapshot(now / 1_000_000).observed_at_ms, None);
    }
}

#[test]
fn steady_callback_processing_and_control_resets_allocate_nothing() {
    let bus = Bus::test_bus();
    let c = control(true);
    let mut r = Renderer::attach(&bus, c.clone()).unwrap();
    let mut now = START_NS;
    let mut next = now + 5_000_000;
    let mut out = [0.0; 960];
    let allocations = allocations_during(|| {
        for tick_index in 0..1000 {
            now += 10_000_000;
            write_until(&bus, &mut next, now);
            if tick_index % 17 == 0 {
                c.set_output(0.5, true);
                c.set_output(0.5, false);
            }
            if tick_index % 31 == 0 {
                c.set_drift(false);
                c.set_drift(true);
            }
            tick(&bus, &mut r, &mut out, now);
        }
    });
    assert_eq!(allocations, 0);
}

#[test]
fn captured_silence_and_intentional_mute_are_not_gaps() {
    let bus = Bus::test_bus();
    let c = control(true);
    let mut r = Renderer::attach(&bus, c.clone()).unwrap();
    let mut out = [9.0; 960];
    for i in 1..25_u64 {
        bus.write(&[[0.0; 2]; 240], 1000 + i * 10 - 5);
        bus.write(&[[0.0; 2]; 240], 1000 + i * 10);
        tick(&bus, &mut r, &mut out, START_NS + i * 10_000_000);
    }
    let observation = c.snapshot(1240);
    assert_eq!(observation.level_dbfs, Some(-96.0));
    assert_eq!(observation.timing.unwrap().gap_output_frames, 0);
    c.set_output(1.0, true);
    for i in 25..50_u64 {
        bus.write(&[[1.0, -1.0]; 480], 1000 + i * 10);
        tick(&bus, &mut r, &mut out, START_NS + i * 10_000_000);
        assert_eq!(out, [0.0; 960]);
    }
    let observation = c.snapshot(1490);
    assert_eq!(observation.level_dbfs, Some(-96.0));
    let t = observation.timing.unwrap();
    assert_eq!(
        (t.gap_output_frames, t.dropped_input_frames, t.resync_count),
        (0, 0, 0)
    );
    assert_eq!(t.applied_adjustment_ppm, None);
}

#[test]
fn invalid_or_burst_device_timestamps_do_not_fail_or_silence_valid_pcm() {
    let bus = Bus::test_bus();
    let c = control(true);
    let mut r = Renderer::attach(&bus, c.clone()).unwrap();
    let mut now = START_NS;
    let mut next = now + 5_000_000;
    warm(&bus, &mut r, &mut now, &mut next);
    let mut out = [0.0; 960];
    for timestamp in [None, Some(0), Some(1), Some(1), Some(now + 90_000_000)] {
        now += 10_000_000;
        write_until(&bus, &mut next, now);
        r.render_clocked(&bus, &mut out, now, timestamp, 48_000, || now, |v| v);
        assert!(!c.failed.load(SeqCst));
        assert_eq!(r.timing.phase, TimingPhase::Running);
        assert!(
            out.as_chunks::<2>()
                .0
                .iter()
                .all(|f| (f[0] - 0.125).abs() < 0.002)
        );
    }
    assert_eq!(
        (r.timing.gaps, r.timing.dropped, r.timing.resyncs),
        (0, 0, 0)
    );
}

#[test]
fn gain_mute_mode_changes_and_faults_do_not_change_an_independent_reader() {
    fn peer_result(change_first: bool) -> (u64, u64, u64, [u64; 3]) {
        let bus = Bus::test_bus();
        let a = control(true);
        let b = control(true);
        let mut first = Renderer::attach(&bus, a.clone()).unwrap();
        let mut peer = Renderer::attach(&bus, b).unwrap();
        let mut output = [0.0_f32; 960];
        let mut large = [0.0_f32; 4000];
        let mut hash = 0_u64;
        for i in 1..301_u64 {
            let now = START_NS + i * 10_000_000;
            bus.write(&[[0.125, -0.25]; 240], now / 1_000_000 - 5);
            bus.write(&[[0.125, -0.25]; 240], now / 1_000_000);
            if change_first && i % 13 == 0 {
                a.set_output(0.25, true);
                a.set_output(0.5, false);
            }
            if change_first && i % 31 == 0 {
                a.set_drift(false);
                a.set_drift(true);
            }
            if change_first && i > 200 {
                tick(&bus, &mut first, &mut large, now);
            } else {
                tick(&bus, &mut first, &mut output, now);
            }
            tick(&bus, &mut peer, &mut output, now);
            for sample in output {
                hash = hash
                    .wrapping_mul(31)
                    .wrapping_add(u64::from(sample.to_bits()));
            }
        }
        (
            hash,
            peer.timing.gaps,
            peer.timing.dropped,
            peer.timing.epoch,
        )
    }
    assert_eq!(peer_result(false), peer_result(true));
}

#[test]
fn switching_to_legacy_and_back_starts_clean_counter_baselines() {
    let bus = Bus::test_bus();
    let c = control(true);
    let mut r = Renderer::attach(&bus, c.clone()).unwrap();
    let mut now = START_NS;
    let mut next = now + 5_000_000;
    warm(&bus, &mut r, &mut now, &mut next);
    let mut out = [9.0; 960];
    c.set_drift(false);
    now += 10_000_000;
    write_until(&bus, &mut next, now);
    tick(&bus, &mut r, &mut out, now);
    assert_eq!(out, [0.0; 960]);
    assert_eq!(r.timing.phase, TimingPhase::Off);
    assert_eq!(
        (r.timing.gaps, r.timing.dropped, r.timing.resyncs),
        (0, 0, 0)
    );
    now += 10_000_000;
    write_until(&bus, &mut next, now);
    tick(&bus, &mut r, &mut out, now);
    assert_eq!(&out[..2], &[0.125, -0.25]);
    c.set_drift(true);
    now += 10_000_000;
    write_until(&bus, &mut next, now);
    tick(&bus, &mut r, &mut out, now);
    assert_eq!(out, [0.0; 960]);
    assert_eq!(r.timing.phase, TimingPhase::Priming);
    assert_eq!(
        (r.timing.gaps, r.timing.dropped, r.timing.resyncs),
        (0, 0, 0)
    );
}

#[test]
fn lifetime_loss_totals_survive_mute_and_mode_timing_baselines() {
    let bus = Bus::test_bus();
    let c = control(true);
    let mut r = Renderer::attach(&bus, c.clone()).unwrap();
    let mut now = START_NS;
    let mut next = now + 5_000_000;
    warm(&bus, &mut r, &mut now, &mut next);
    let mut out = [0.0; 960];
    now += 60_000_000;
    write_until(&bus, &mut next, now);
    tick(&bus, &mut r, &mut out, now);
    let before = c.snapshot(now / 1_000_000);
    assert!(before.dropped_frames > 0 && before.underrun_frames > 0);
    for (muted, enabled) in [(true, true), (true, false), (false, false), (false, true)] {
        c.set_output(1.0, muted);
        c.set_drift(enabled);
        now += 10_000_000;
        write_until(&bus, &mut next, now);
        tick(&bus, &mut r, &mut out, now);
        let after = c.snapshot(now / 1_000_000);
        assert_eq!(after.dropped_frames, before.dropped_frames);
        assert_eq!(after.underrun_frames, before.underrun_frames);
        let timing = after.timing.unwrap();
        assert_eq!(
            (timing.dropped_input_frames, timing.gap_output_frames),
            (0, 0)
        );
    }
}
