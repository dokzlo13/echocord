//! Persistent tokenless capture worker and common 48 kHz stereo mix.
#[cfg(any(windows, test))]
use crate::model::*;

pub fn linear(db: Option<f32>) -> f32 {
    db.map_or(0.0, |db| 10.0_f32.powf(db / 20.0))
}
pub fn valid_gain(db: Option<f32>) -> bool {
    db.is_none_or(|db| db.is_finite() && (-60.0..=0.0).contains(&db))
}
pub fn sum_source(mix: &mut [[f32; 2]], input: &[[f32; 2]], db: Option<f32>, muted: bool) -> f32 {
    let gain = linear(db);
    let mut peak = 0.0_f32;
    for (mixed, source) in mix.iter_mut().zip(input) {
        for channel in 0..2 {
            let sample = if source[channel].is_finite() {
                source[channel] * gain
            } else {
                0.0
            };
            peak = peak.max(sample.abs());
            if !muted {
                mixed[channel] += sample;
            }
        }
    }
    if peak > 0.0 {
        (20.0 * peak.log10()).max(-96.0)
    } else {
        -96.0
    }
}
pub fn clamp_mix(mix: &mut [[f32; 2]]) {
    for frame in mix {
        for sample in frame {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

#[cfg(any(windows, test))]
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AudioSnapshot {
    pub observed_at_ms: Option<f64>,
    pub mix_pre_clip: bool,
    pub limiter_reduction_db: Option<f32>,
    pub revision: u64,
    pub inputs: Vec<InputStrip>,
    pub telemetry: Vec<InputTelemetry>,
}

/// Retain identifiers and meter hold state between 5 ms blocks. Owned telemetry
/// is cloned only for the 100 ms publication boundary.
#[cfg(any(windows, test))]
struct SourceMeters {
    latest: InputTelemetry,
    held: Option<(f32, bool)>,
}
#[cfg(any(windows, test))]
impl SourceMeters {
    fn new(strip_id: String) -> Self {
        Self {
            latest: InputTelemetry {
                timing: None,
                observed_at_ms: None,
                strip_id,
                level_dbfs: None,
                pre_clip: false,
                dropped_frames: 0,
                underrun_frames: 0,
                queued_frames: 0,
            },
            held: None,
        }
    }
    fn observe(&mut self, level: f32, pre_clip: bool, dropped: u32, underrun: u32, queued: u32) {
        let held = self.held.get_or_insert((-96.0, false));
        held.0 = held.0.max(level);
        held.1 |= pre_clip;
        self.latest.observed_at_ms = Some(crate::audio::observation_clock_ms());
        self.latest.level_dbfs = Some(held.0);
        self.latest.pre_clip = held.1;
        self.latest.dropped_frames = dropped;
        self.latest.underrun_frames = underrun;
        self.latest.queued_frames = queued;
    }
    fn reset_window(&mut self) {
        self.held = None;
    }
    fn unavailable(&mut self) {
        self.held = None;
        self.latest.observed_at_ms = None;
        self.latest.level_dbfs = None;
        self.latest.pre_clip = false;
        self.latest.dropped_frames = 0;
        self.latest.underrun_frames = 0;
        self.latest.queued_frames = 0;
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use crate::{
        audio::{self, CaptureSession},
        mix_bus::{self, BusHandle, Mapping},
        process::EngineProcess,
        protocol::{self, Frame, Validator},
    };
    use serde::{Deserialize, Serialize};
    use std::{
        collections::BTreeMap,
        io::{BufReader, Read},
        os::windows::io::{AsHandle, AsRawHandle},
        path::PathBuf,
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };

    #[derive(Serialize, Deserialize)]
    enum AudioCommand {
        Sources {
            revision: u64,
            inputs: Vec<InputStrip>,
        },
        Stop,
    }
    #[cfg(test)]
    #[test]
    fn largest_supported_eight_card_status_and_command_fit_bounded_protocol() {
        let inputs: Vec<_> = (0..MAX_INPUTS)
            .map(|index| InputStrip {
                strip_id: format!("input-{index}"),
                source_id: "\\".repeat(1024),
                kind: SourceKind::Recording,
                display_name: "é".repeat(512),
                detail: Some("\"".repeat(1024)),
                phase: InputPhase::Capturing,
                fault: None,
                gain_db: Some(-59.5),
                muted: true,
                colour_slot: 5,
                restored: false,
            })
            .collect();
        let command = Frame {
            version: protocol::VERSION,
            generation: 1,
            sequence: 1,
            body: AudioCommand::Sources {
                revision: 1,
                inputs: inputs.clone(),
            },
        };
        assert!(protocol::encode(&command).is_ok());
        let telemetry = inputs
            .iter()
            .map(|i| InputTelemetry {
                timing: None,
                observed_at_ms: None,
                strip_id: i.strip_id.clone(),
                level_dbfs: Some(-59.5),
                pre_clip: true,
                dropped_frames: u32::MAX,
                underrun_frames: u32::MAX,
                queued_frames: 1920,
            })
            .collect();
        let status = Frame {
            version: protocol::VERSION,
            generation: 1,
            sequence: 1,
            body: AudioSnapshot {
                limiter_reduction_db: None,
                observed_at_ms: Some(0.0),
                revision: 1,
                inputs,
                telemetry,
                mix_pre_clip: true,
            },
        };
        assert!(protocol::encode(&status).is_ok());
    }

    struct Mailbox {
        sources: Option<(u64, Vec<InputStrip>)>,
        stop: Option<Instant>,
    }
    pub struct AudioHost {
        pub bus: Arc<BusHandle>,
        revision: AtomicU64,
        mailbox: Arc<(Mutex<Mailbox>, Condvar)>,
        snapshot: Arc<Mutex<Option<AudioSnapshot>>>,
        /// 0 running, 1 observed exit, 2 cleanup unconfirmed.
        pub exit: Arc<AtomicU8>,
    }
    impl AudioHost {
        pub fn launch(executable: PathBuf) -> Result<Self, AppError> {
            let bus = Arc::new(BusHandle::create().map_err(|_| failure())?);
            let inherited = bus.inheritable().map_err(|_| failure())?;
            let mailbox = Arc::new((
                Mutex::new(Mailbox {
                    sources: Some((0, Vec::new())),
                    stop: None,
                }),
                Condvar::new(),
            ));
            let snapshot = Arc::new(Mutex::new(None));
            let exit = Arc::new(AtomicU8::new(0));
            let host = Self {
                bus,
                revision: AtomicU64::new(0),
                mailbox: mailbox.clone(),
                snapshot: snapshot.clone(),
                exit: exit.clone(),
            };
            std::thread::spawn(move || {
                let args = [
                    "--audio-worker".into(),
                    (inherited.as_raw_handle() as usize).to_string().into(),
                ];
                let Ok(mut process) =
                    EngineProcess::spawn_with_handles(&executable, &args, &[inherited.as_handle()])
                else {
                    exit.store(1, Ordering::Release);
                    return;
                };
                drop(inherited);
                let (Some(mut input), Some(output), Some(mut errors)) = (
                    process.take_stdin(),
                    process.take_stdout(),
                    process.take_stderr(),
                ) else {
                    let _ = process.terminate();
                    exit.store(2, Ordering::Release);
                    return;
                };
                let mail = mailbox.clone();
                std::thread::spawn(move || {
                    let mut sequence = 0;
                    loop {
                        let (lock, wake) = &*mail;
                        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                        while state.stop.is_none() && state.sources.is_none() {
                            state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                        }
                        let command = if state.stop.is_some() {
                            AudioCommand::Stop
                        } else {
                            {
                                let (revision, inputs) = state.sources.take().unwrap_or_default();
                                AudioCommand::Sources { revision, inputs }
                            }
                        };
                        let stop = matches!(command, AudioCommand::Stop);
                        drop(state);
                        sequence += 1;
                        if protocol::write(
                            &mut input,
                            &Frame {
                                version: protocol::VERSION,
                                generation: 1,
                                sequence,
                                body: command,
                            },
                        )
                        .is_err()
                        {
                            request_stop(&mail);
                            break;
                        }
                        if stop {
                            break;
                        }
                    }
                });
                let read_mail = mailbox.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(output);
                    let mut validator = Validator::new(1).expect("fixed generation");
                    while let Ok(Some(frame)) = protocol::read::<AudioSnapshot>(&mut reader) {
                        if validator.accept(&frame).is_err() {
                            break;
                        }
                        *snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame.body);
                    }
                    request_stop(&read_mail);
                });
                std::thread::spawn(move || {
                    let mut bytes = [0; 4096];
                    while errors.read(&mut bytes).is_ok_and(|n| n > 0) {}
                });
                loop {
                    match process.try_wait() {
                        Ok(Some(_)) => {
                            request_stop(&mailbox);
                            exit.store(1, Ordering::Release);
                            break;
                        }
                        Err(_) => {
                            request_stop(&mailbox);
                        }
                        _ => {}
                    }
                    let stopped = mailbox.0.lock().unwrap_or_else(|e| e.into_inner()).stop;
                    if let Some(at) = stopped {
                        if at.elapsed() >= Duration::from_millis(750) {
                            let _ = process.terminate();
                        }
                        if at.elapsed() >= Duration::from_millis(3750) {
                            exit.store(2, Ordering::Release);
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            Ok(host)
        }
        pub fn sources(&self, inputs: Vec<InputStrip>) {
            let (lock, wake) = &*self.mailbox;
            let mut mail = lock.lock().unwrap_or_else(|e| e.into_inner());
            if mail.stop.is_none() {
                mail.sources = Some((self.revision.fetch_add(1, Ordering::AcqRel) + 1, inputs));
                wake.notify_one();
            }
        }
        pub fn snapshot(&self) -> Option<AudioSnapshot> {
            self.snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
        }
        pub fn revision(&self) -> u64 {
            self.revision.load(Ordering::Acquire)
        }
        pub fn stop(&self) {
            request_stop(&self.mailbox);
        }
    }
    impl Drop for AudioHost {
        fn drop(&mut self) {
            self.stop();
        }
    }
    fn request_stop(mail: &Arc<(Mutex<Mailbox>, Condvar)>) {
        let (lock, wake) = &**mail;
        lock.lock()
            .unwrap_or_else(|e| e.into_inner())
            .stop
            .get_or_insert_with(Instant::now);
        wake.notify_one();
    }
    fn failure() -> AppError {
        AppError::new(
            ErrorCode::EngineStopped,
            "The native audio worker is unavailable. Choose Quit from the tray, then reopen Echocord.",
        )
    }

    struct Slot {
        desired: Option<InputStrip>,
        session: Option<CaptureSession>,
        operation: bool,
        cancel: Arc<AtomicBool>,
        source: String,
        deadline: Option<Instant>,
        meters: SourceMeters,
    }
    enum Completed {
        Open(String, Result<CaptureSession, AppError>),
        Stopped(String, bool),
    }
    fn retire(id: String, session: CaptureSession, done: mpsc::SyncSender<Completed>) {
        std::thread::spawn(move || {
            let stopped = session.control.stop();
            drop(session);
            let _ = done.send(Completed::Stopped(id, stopped));
        });
    }
    fn open_pending_sources(
        slots: &mut BTreeMap<String, Slot>,
        done: &mpsc::SyncSender<Completed>,
    ) {
        if !slots.values().any(|slot| {
            !slot.operation
                && slot.session.is_none()
                && slot.desired.as_ref().is_some_and(|input| {
                    matches!(input.phase, InputPhase::Opening | InputPhase::Capturing)
                })
        }) {
            return;
        }
        let mut occupied: Vec<_> = slots
            .iter()
            .filter(|(_, slot)| slot.operation || slot.session.is_some())
            .map(|(id, slot)| (id.clone(), slot.source.clone()))
            .collect();
        for (id, slot) in slots.iter_mut() {
            if slot.operation
                || slot.session.is_some()
                || !slot
                    .desired
                    .as_ref()
                    .is_some_and(|i| matches!(i.phase, InputPhase::Opening | InputPhase::Capturing))
            {
                continue;
            }
            let blocked = slot.desired.as_ref().is_some_and(|input| {
                occupied.iter().any(|(other, source)| {
                    other != id
                        && (source == &input.source_id
                            || (audio::application::is_application_id(source)
                                && audio::application::is_application_id(&input.source_id)
                                && audio::application::roots_overlap(source, &input.source_id)
                                    .unwrap_or(true)))
                })
            });
            if blocked {
                continue;
            }
            if !slot.operation
                && slot.session.is_none()
                && let Some(input) = &mut slot.desired
                && matches!(input.phase, InputPhase::Opening | InputPhase::Capturing)
            {
                slot.source = input.source_id.clone();
                slot.cancel = Arc::new(AtomicBool::new(false));
                slot.operation = true;
                slot.deadline = Some(Instant::now() + Duration::from_secs(5));
                input.phase = InputPhase::Opening;
                occupied.push((id.clone(), slot.source.clone()));
                let source = slot.source.clone();
                let id = id.clone();
                let cancel = slot.cancel.clone();
                let done = done.clone();
                std::thread::spawn(move || {
                    let result = audio::start_capture_with_cancel(&source, &|| {
                        cancel.load(Ordering::Acquire)
                    });
                    let _ = done.send(Completed::Open(id, result));
                });
            }
        }
    }

    fn mix_sources(
        slots: &mut BTreeMap<String, Slot>,
        done: &mpsc::SyncSender<Completed>,
    ) -> ([[f32; 2]; 240], bool) {
        let mut mix = [[0.0; 2]; 240];
        for (id, slot) in slots {
            let Some(input) = &mut slot.desired else {
                continue;
            };
            let Some(session) = &mut slot.session else {
                slot.meters.unavailable();
                continue;
            };
            let capture = session.control.snapshot();
            if matches!(
                capture.phase,
                CapturePhase::Error | CapturePhase::Missing | CapturePhase::Stopped
            ) {
                input.phase = InputPhase::Fault;
                input.fault = Some(AppError::new(
                    session.control.failure_code(),
                    capture
                        .message
                        .unwrap_or_else(|| "Capture stopped. Select the source again.".into()),
                ));
                let session = slot.session.take().expect("present");
                slot.operation = true;
                slot.deadline = Some(Instant::now() + Duration::from_millis(750));
                slot.cancel.store(true, Ordering::Release);
                retire(id.clone(), session, done.clone());
                slot.meters.unavailable();
                continue;
            }
            let mut frames = [[0.0; 2]; 240];
            session.input.drain_frames(&mut frames);
            let level = sum_source(&mut mix, &frames, input.gain_db, input.muted);
            let pre_clip = session.control.take_pre_clip();
            if capture.level_dbfs.is_some() {
                slot.meters.observe(
                    level,
                    pre_clip,
                    capture.dropped_frames,
                    capture.underrun_frames,
                    capture.queued_frames,
                );
            } else {
                slot.meters.unavailable();
                slot.meters.latest.dropped_frames = capture.dropped_frames;
                slot.meters.latest.underrun_frames = capture.underrun_frames;
                slot.meters.latest.queued_frames = capture.queued_frames;
            }
        }
        let pre_clip = mix.iter().flatten().any(|sample| sample.abs() >= 1.0);
        clamp_mix(&mut mix);
        (mix, pre_clip)
    }

    #[cfg(test)]
    #[test]
    fn exited_application_becomes_an_invalid_selection_without_opening_a_device() {
        let session = audio::bus_capture(|frames| frames.fill([0.0; 2]));
        session.control.simulate_application_exit();
        let input = InputStrip {
            strip_id: "fixture".into(),
            source_id: "app:42:0000000000000001".into(),
            display_name: "Owned fixture".into(),
            kind: SourceKind::Application,
            detail: None,
            phase: InputPhase::Capturing,
            fault: None,
            gain_db: Some(0.0),
            muted: false,
            colour_slot: 0,
            restored: false,
        };
        let mut slots = BTreeMap::from([(
            "fixture".into(),
            Slot {
                source: input.source_id.clone(),
                desired: Some(input),
                session: Some(session),
                operation: false,
                cancel: Arc::new(AtomicBool::new(false)),
                deadline: None,
                meters: SourceMeters::new("fixture".into()),
            },
        )]);
        let (done, completed) = mpsc::sync_channel(1);
        mix_sources(&mut slots, &done);
        let failed = slots["fixture"].desired.as_ref().unwrap();
        assert_eq!(failed.phase, InputPhase::Fault);
        assert_eq!(
            failed.fault.as_ref().unwrap().code,
            ErrorCode::InvalidSelection
        );
        assert_eq!(failed.source_id, "app:42:0000000000000001");
        assert!(matches!(
            completed.recv_timeout(Duration::from_secs(1)).unwrap(),
            Completed::Stopped(_, true)
        ));
    }

    #[cfg(test)]
    #[test]
    fn eight_owned_synthetic_captures_mix_without_steady_allocations() {
        use crate::audio::recording_format::tests::allocations_during;
        struct StopAll(Vec<audio::CaptureControl>);
        impl Drop for StopAll {
            fn drop(&mut self) {
                for control in &self.0 {
                    let _ = control.stop();
                }
            }
        }
        let mut controls = StopAll(Vec::new());
        let mut pumps = Vec::new();
        let mut slots = BTreeMap::new();
        for index in 0..MAX_INPUTS {
            // Feed the real capture rings directly, without relying on native
            // thread scheduling. No endpoint, network or physical output opens.
            let (session, pump) = audio::tests::synchronous_mixer_fixture();
            pumps.push(pump);
            controls.0.push(session.control.clone());
            let id = format!("fixture-{index}");
            let input = InputStrip {
                strip_id: id.clone(),
                source_id: id.clone(),
                kind: SourceKind::Recording,
                display_name: id.clone(),
                detail: None,
                phase: InputPhase::Capturing,
                fault: None,
                gain_db: Some(-6.0),
                muted: index.is_multiple_of(2),
                colour_slot: index as u8,
                restored: false,
            };
            slots.insert(
                id.clone(),
                Slot {
                    desired: Some(input),
                    session: Some(session),
                    operation: false,
                    cancel: Arc::new(AtomicBool::new(false)),
                    source: id.clone(),
                    deadline: None,
                    meters: SourceMeters::new(id),
                },
            );
        }
        let (done, _completed) = mpsc::sync_channel(16);
        let bus = mix_bus::Bus::test_bus();
        let allocations = allocations_during(|| {
            for tick in 0..1000 {
                for pump in &mut pumps {
                    pump();
                }
                open_pending_sources(&mut slots, &done);
                let (mix, _) = mix_sources(&mut slots, &done);
                bus.write(&mix, tick * 5);
            }
        });
        assert_eq!(allocations, 0);
        assert!(
            slots
                .values()
                .all(|slot| slot.meters.latest.level_dbfs.is_some())
        );
        println!("steady mix: 8 owned synthetic sources, 1000 ticks, {allocations} allocations");
    }

    pub fn run_worker(raw: usize) -> Result<(), AppError> {
        let mapping = Mapping::inherited(raw).map_err(|_| failure())?;
        mapping.bus().initialize();
        let (desired, latest) = std::sync::mpsc::sync_channel::<(u64, Vec<InputStrip>)>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let input_stop = stop.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(std::io::stdin());
            let mut validator = Validator::new(1).expect("fixed generation");
            while let Ok(Some(frame)) = protocol::read::<AudioCommand>(&mut reader) {
                if validator.accept(&frame).is_err() {
                    break;
                }
                match frame.body {
                    AudioCommand::Stop => break,
                    AudioCommand::Sources { revision, inputs } => {
                        if desired.send((revision, inputs)).is_err() {
                            break;
                        }
                    }
                }
            }
            input_stop.store(true, Ordering::Release);
        });
        let (statuses, observations) = mpsc::sync_channel::<AudioSnapshot>(1);
        std::thread::spawn(move || {
            let mut output = std::io::stdout();
            let mut sequence = 0;
            while let Ok(snapshot) = observations.recv() {
                sequence += 1;
                if protocol::write(
                    &mut output,
                    &Frame {
                        version: protocol::VERSION,
                        generation: 1,
                        sequence,
                        body: snapshot,
                    },
                )
                .is_err()
                {
                    break;
                }
            }
        });
        let (done, completed) = mpsc::sync_channel(16);
        let mut slots: BTreeMap<String, Slot> = BTreeMap::new();
        let mut waiting = BTreeMap::<String, InputStrip>::new();
        let mut revision = 0;
        let mut mix_pre_clip = false;
        let mut next = Instant::now();
        let mut publish = next;
        while !stop.load(Ordering::Acquire) {
            if slots.values().any(|slot| {
                slot.deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
            }) {
                // A native destructor/open cannot be cancelled safely in-process.
                // Exit immediately; the parent observes exit through its owned job.
                std::process::exit(1);
            }
            let mut lifecycle_changed = false;
            while let Ok((new_revision, inputs)) = latest.try_recv() {
                lifecycle_changed = true;
                revision = new_revision;
                for slot in slots.values_mut() {
                    slot.meters.reset_window();
                }
                waiting.retain(|id, _| inputs.iter().any(|i| &i.strip_id == id));
                if inputs.len() > MAX_INPUTS || inputs.iter().any(|i| !valid_gain(i.gain_db)) {
                    std::process::exit(1);
                }
                for (id, slot) in &mut slots {
                    let new = inputs.iter().find(|i| &i.strip_id == id).cloned();
                    let needs_stop = new.as_ref().is_none_or(|i| {
                        i.source_id != slot.source
                            || matches!(i.phase, InputPhase::Stopped | InputPhase::Fault)
                    });
                    slot.desired = new;
                    if needs_stop {
                        slot.cancel.store(true, Ordering::Release);
                        if let Some(session) = slot.session.take() {
                            slot.operation = true;
                            slot.deadline = Some(Instant::now() + Duration::from_millis(750));
                            retire(id.clone(), session, done.clone());
                        }
                    }
                }
                for input in inputs {
                    if !slots.contains_key(&input.strip_id) {
                        // Physical owners (including retired/opening slots) stay bounded.
                        if slots.len() >= MAX_INPUTS {
                            waiting.insert(input.strip_id.clone(), input);
                            continue;
                        }
                        slots.insert(
                            input.strip_id.clone(),
                            Slot {
                                meters: SourceMeters::new(input.strip_id.clone()),
                                source: input.source_id.clone(),
                                deadline: None,
                                desired: Some(input),
                                session: None,
                                operation: false,
                                cancel: Arc::new(AtomicBool::new(false)),
                            },
                        );
                    }
                }
            }
            while let Ok(result) = completed.try_recv() {
                lifecycle_changed = true;
                match result {
                    Completed::Stopped(id, clean) => {
                        if !clean {
                            std::process::exit(1);
                        }
                        if let Some(slot) = slots.get_mut(&id) {
                            slot.operation = false;
                            slot.deadline = None;
                        }
                    }
                    Completed::Open(id, result) => {
                        if let Some(slot) = slots.get_mut(&id) {
                            match result {
                                Ok(session) if !slot.cancel.load(Ordering::Acquire) => {
                                    slot.operation = false;
                                    slot.deadline = None;
                                    slot.session = Some(session);
                                    if let Some(i) = &mut slot.desired {
                                        i.phase = InputPhase::Capturing;
                                        i.fault = None;
                                        i.restored = false;
                                    }
                                }
                                Ok(session) => {
                                    slot.deadline =
                                        Some(Instant::now() + Duration::from_millis(750));
                                    retire(id, session, done.clone());
                                }
                                Err(error) => {
                                    if error.code == ErrorCode::CleanupIncomplete {
                                        std::process::exit(1);
                                    }
                                    slot.operation = false;
                                    slot.deadline = None;
                                    if !slot.cancel.load(Ordering::Acquire)
                                        && let Some(i) = &mut slot.desired
                                    {
                                        i.phase = InputPhase::Fault;
                                        i.fault = Some(error);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if lifecycle_changed {
                slots.retain(|_, slot| {
                    slot.desired.is_some() || slot.operation || slot.session.is_some()
                });
                while slots.len() < MAX_INPUTS {
                    let Some((id, input)) = waiting.pop_first() else {
                        break;
                    };
                    slots.insert(
                        id,
                        Slot {
                            meters: SourceMeters::new(input.strip_id.clone()),
                            source: input.source_id.clone(),
                            deadline: None,
                            desired: Some(input),
                            session: None,
                            operation: false,
                            cancel: Arc::new(AtomicBool::new(false)),
                        },
                    );
                }
                open_pending_sources(&mut slots, &done);
            }
            let (mix, clipped) = mix_sources(&mut slots, &done);
            mix_pre_clip |= clipped;
            mapping.bus().write(&mix, mix_bus::now_ms());
            if Instant::now() >= publish {
                publish = Instant::now() + Duration::from_millis(100);
                let _ = statuses.try_send(AudioSnapshot {
                    limiter_reduction_db: None,
                    observed_at_ms: Some(audio::observation_clock_ms()),
                    mix_pre_clip,
                    revision,
                    inputs: slots.values().filter_map(|s| s.desired.clone()).collect(),
                    telemetry: slots
                        .values()
                        .filter(|slot| slot.desired.is_some())
                        .map(|slot| slot.meters.latest.clone())
                        .collect(),
                });
                for slot in slots.values_mut() {
                    slot.meters.reset_window();
                }
                mix_pre_clip = false;
            }
            next += Duration::from_millis(5);
            let now = Instant::now();
            if now < next {
                std::thread::sleep(next - now);
            } else if now.duration_since(next) > Duration::from_millis(5) {
                next = now;
            }
        }
        for slot in slots.values_mut() {
            slot.cancel.store(true, Ordering::Release);
            if let Some(session) = slot.session.take() {
                retire(String::new(), session, done.clone());
            }
        }
        // Main returns and exits this worker process; parent confirms actual exit.
        Ok(())
    }
}
#[cfg(windows)]
pub use native::*;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_mix_sums_stereo_clamps_and_preserves_muted_meter() {
        let source = [[0.8, -0.6]; 240];
        let mut mix = [[0.0; 2]; 240];
        sum_source(&mut mix, &source, Some(0.0), false);
        let peak = sum_source(&mut mix, &source, Some(-6.0206), true);
        assert!((peak - 20.0 * 0.4_f32.log10()).abs() < 0.001);
        assert_eq!(mix, source);
        sum_source(&mut mix, &source, Some(0.0), false);
        clamp_mix(&mut mix);
        assert_eq!(mix, [[1.0, -1.0]; 240]);
    }
    #[test]
    fn fractional_gain_and_off_are_distinct_without_percent_quantization() {
        assert!((linear(Some(-59.5)) - 0.00105925).abs() < 0.000001);
        assert_eq!(linear(None), 0.0);
        assert!(!valid_gain(Some(f32::NAN)));
    }

    #[test]
    fn retained_source_meters_preserve_window_hold_and_latest_counts() {
        let mut meters = SourceMeters::new("fixture".into());
        meters.observe(-12.0, true, 10, 20, 240);
        meters.observe(-30.0, false, 11, 22, 480);
        assert_eq!(meters.latest.level_dbfs, Some(-12.0));
        assert!(meters.latest.pre_clip);
        assert_eq!(
            (
                meters.latest.dropped_frames,
                meters.latest.underrun_frames,
                meters.latest.queued_frames
            ),
            (11, 22, 480)
        );
        meters.reset_window();
        meters.observe(-48.0, false, 12, 24, 120);
        assert_eq!(meters.latest.level_dbfs, Some(-48.0));
        assert!(!meters.latest.pre_clip);
        meters.unavailable();
        assert_eq!(meters.latest.level_dbfs, None);
        assert_eq!(meters.latest.queued_frames, 0);
        assert_eq!(meters.latest.strip_id, "fixture");
        meters.observe(-60.0, false, 0, 0, 240);
        assert_eq!(meters.latest.level_dbfs, Some(-60.0));
    }

    #[test]
    fn steady_meter_updates_allocate_only_at_publication() {
        use crate::audio::recording_format::tests::allocations_during;
        let mut meters: Vec<_> = (0..MAX_INPUTS)
            .map(|index| SourceMeters::new(format!("fixture-{index}")))
            .collect();
        let steady = allocations_during(|| {
            for tick in 0..1000 {
                for source in &mut meters {
                    source.observe(-24.0, false, tick, 0, 240);
                    if tick % 20 == 0 {
                        source.reset_window();
                    }
                }
            }
        });
        assert_eq!(steady, 0);
        let publication = allocations_during(|| {
            std::hint::black_box(
                meters
                    .iter()
                    .map(|source| source.latest.clone())
                    .collect::<Vec<_>>(),
            );
        });
        assert!(publication > MAX_INPUTS);
        println!(
            "source telemetry: 1000 ticks, {steady} allocations; one publication, {publication} allocations"
        );
    }
}
