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

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessingMode {
    enabled: bool,
    epoch: u64,
}

#[cfg(any(windows, test))]
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceIntent {
    input: InputStrip,
    epoch: u64,
}

#[cfg(any(windows, test))]
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceUpdate {
    revision: u64,
    sources: Vec<SourceIntent>,
    drift: ProcessingMode,
    limiter: ProcessingMode,
}

#[cfg(any(windows, test))]
#[derive(serde::Serialize, serde::Deserialize)]
enum AudioCommand {
    Sources(SourceUpdate),
    Stop,
}

#[cfg(any(windows, test))]
fn active(input: &InputStrip) -> bool {
    matches!(input.phase, InputPhase::Opening | InputPhase::Capturing)
}

/// Updated for every accepted parent request, before replacing a coalesced
/// mailbox value. Presentation, gain and mix-only mute do not change identity.
#[cfg(any(windows, test))]
#[derive(Default)]
struct RequestHistory {
    previous: Vec<SourceIntent>,
    next_epoch: u64,
    drift: ProcessingMode,
    limiter: ProcessingMode,
}
#[cfg(any(windows, test))]
impl RequestHistory {
    fn update(
        &mut self,
        revision: u64,
        inputs: Vec<InputStrip>,
        drift: bool,
        limiter: bool,
    ) -> SourceUpdate {
        for (mode, enabled) in [(&mut self.drift, drift), (&mut self.limiter, limiter)] {
            if mode.enabled != enabled {
                mode.epoch = mode
                    .epoch
                    .checked_add(1)
                    .expect("processing epoch exhausted");
                mode.enabled = enabled;
            }
        }
        let mut sources = Vec::with_capacity(inputs.len());
        for input in inputs {
            let unchanged = self.previous.iter().find(|previous| {
                previous.input.strip_id == input.strip_id
                    && previous.input.source_id == input.source_id
                    && previous.input.kind == input.kind
                    && active(&previous.input) == active(&input)
            });
            let epoch = unchanged.map_or_else(
                || {
                    self.next_epoch = self
                        .next_epoch
                        .checked_add(1)
                        .expect("source epoch exhausted");
                    self.next_epoch
                },
                |previous| previous.epoch,
            );
            sources.push(SourceIntent { input, epoch });
        }
        self.previous = sources.clone();
        SourceUpdate {
            revision,
            sources,
            drift: self.drift,
            limiter: self.limiter,
        }
    }
}

#[cfg(any(windows, test))]
fn valid_update(update: &SourceUpdate) -> bool {
    if update.sources.len() > MAX_INPUTS {
        return false;
    }
    let text =
        |value: &str, maximum| value.len() <= maximum && !value.chars().any(char::is_control);
    update.sources.iter().enumerate().all(|(index, source)| {
        let input = &source.input;
        source.epoch != 0
            && !input.strip_id.is_empty()
            && text(&input.strip_id, 64)
            && !input.source_id.is_empty()
            && text(&input.source_id, 1024)
            && !input.source_id.starts_with("mix-handle:")
            && (input.kind == SourceKind::Application)
                == crate::audio::application::is_application_id(&input.source_id)
            && text(&input.display_name, 1024)
            && input
                .detail
                .as_deref()
                .is_none_or(|value| text(value, 1024))
            && input
                .fault
                .as_ref()
                .is_none_or(|fault| text(&fault.message, 256))
            && valid_gain(input.gain_db)
            && !update.sources[..index].iter().any(|other| {
                other.input.strip_id == input.strip_id || other.input.source_id == input.source_id
            })
    })
}

/// Common mix processing owns only the fixed audio delay, never source capture
/// lifetimes. It resets for an engine gap or mode epoch, not an ordinary edit.
#[cfg(any(windows, test))]
struct MixOutput {
    limiter: crate::limiter::Limiter,
    mode: ProcessingMode,
    last_write_ms: Option<u64>,
    pre_clip: bool,
    reduction: Option<f32>,
}
#[cfg(any(windows, test))]
impl MixOutput {
    fn new() -> Self {
        Self {
            limiter: crate::limiter::Limiter::new(false),
            mode: ProcessingMode::default(),
            last_write_ms: None,
            pre_clip: false,
            reduction: None,
        }
    }
    fn configure(&mut self, mode: ProcessingMode) {
        if mode != self.mode {
            self.mode = mode;
            self.limiter.set_enabled(mode.enabled);
            // A coalesced off/on can preserve the bool while changing the epoch.
            self.limiter.reset();
            self.reset_window();
        }
    }
    fn process(&mut self, mix: &mut [[f32; 2]], now_ms: u64) {
        if self
            .last_write_ms
            .is_some_and(|last| now_ms < last || now_ms - last > 40)
        {
            self.limiter.reset();
            self.reset_window();
        }
        self.pre_clip |= mix.iter().flatten().any(|sample| sample.abs() >= 1.0);
        let reduction = self.limiter.process_block(mix);
        if self.mode.enabled {
            self.reduction = Some(self.reduction.unwrap_or(0.0).max(reduction));
        }
        clamp_mix(mix);
    }
    fn write(
        &mut self,
        bus: &crate::mix_bus::Bus,
        mix: &mut [[f32; 2]],
        started_ms: u64,
        completed: impl FnOnce() -> u64,
    ) {
        let previous = self.last_write_ms;
        self.process(mix, started_ms);
        let finished = completed();
        let crossed_gap = previous.is_some_and(|last| {
            started_ms.saturating_sub(last) <= 40 && finished.saturating_sub(last) > 40
        });
        if finished < started_ms || finished.saturating_sub(started_ms) > 40 || crossed_gap {
            // A pause during source mixing or DSP must not stamp old mixed PCM
            // as fresh. Clear retained audio before the first resumed bus write.
            self.limiter.reset();
            self.reset_window();
            mix.fill([0.0; 2]);
        }
        self.last_write_ms = Some(finished);
        bus.write(mix, finished);
    }
    fn reset_window(&mut self) {
        self.pre_clip = false;
        self.reduction = self.mode.enabled.then_some(0.0);
    }
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
        self.latest.timing = None;
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
            body: AudioCommand::Sources(RequestHistory::default().update(
                1,
                inputs.clone(),
                true,
                true,
            )),
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
        sources: Option<SourceUpdate>,
        history: RequestHistory,
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
                    sources: Some(SourceUpdate::default()),
                    history: RequestHistory::default(),
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
                            AudioCommand::Sources(state.sources.take().unwrap_or_default())
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
        pub fn sources(&self, inputs: Vec<InputStrip>, drift: bool, limiter: bool) {
            let (lock, wake) = &*self.mailbox;
            let mut mail = lock.lock().unwrap_or_else(|e| e.into_inner());
            if mail.stop.is_none() {
                let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
                let update = mail.history.update(revision, inputs, drift, limiter);
                if !valid_update(&update) {
                    mail.stop = Some(Instant::now());
                } else {
                    mail.sources = Some(update);
                }
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
        intent_epoch: u64,
        session_epoch: u64,
    }
    impl Slot {
        fn begin_open(&mut self, next_session_epoch: &mut u64) {
            *next_session_epoch = next_session_epoch
                .checked_add(1)
                .expect("capture lifetime exhausted");
            self.session_epoch = *next_session_epoch;
            let input = self.desired.as_mut().expect("pending source");
            self.source = input.source_id.clone();
            self.cancel = Arc::new(AtomicBool::new(false));
            self.operation = true;
            self.deadline = Some(Instant::now() + Duration::from_secs(5));
            input.phase = InputPhase::Opening;
        }
        fn accepts_open(&self, source: &str, intent: u64, session: u64) -> bool {
            !self.cancel.load(Ordering::Acquire)
                && self.session_epoch == session
                && self.intent_epoch == intent
                && self
                    .desired
                    .as_ref()
                    .is_some_and(|input| active(input) && input.source_id == source)
        }
        fn activate(&mut self, mut session: CaptureSession, drift: ProcessingMode) {
            self.operation = false;
            self.deadline = None;
            session.input.configure_clock_correction(
                drift.enabled,
                self.session_epoch,
                drift.epoch,
            );
            self.session = Some(session);
            self.meters.unavailable();
            if let Some(input) = &mut self.desired {
                input.phase = InputPhase::Capturing;
                input.fault = None;
                input.restored = false;
            }
        }
    }
    enum Completed {
        Open(
            String,
            String,
            u64,
            u64,
            Box<Result<CaptureSession, AppError>>,
        ),
        Stopped(String, u64, bool),
    }
    fn retire(id: String, epoch: u64, session: CaptureSession, done: mpsc::SyncSender<Completed>) {
        std::thread::spawn(move || {
            let stopped = session.control.stop();
            drop(session);
            let _ = done.send(Completed::Stopped(id, epoch, stopped));
        });
    }
    fn open_pending_sources(
        slots: &mut BTreeMap<String, Slot>,
        done: &mpsc::SyncSender<Completed>,
        next_session_epoch: &mut u64,
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
                slot.begin_open(next_session_epoch);
                occupied.push((id.clone(), slot.source.clone()));
                let source = slot.source.clone();
                let intent_epoch = slot.intent_epoch;
                let session_epoch = slot.session_epoch;
                let id = id.clone();
                let cancel = slot.cancel.clone();
                let done = done.clone();
                std::thread::spawn(move || {
                    let result = audio::start_capture_with_cancel(&source, &|| {
                        cancel.load(Ordering::Acquire)
                    });
                    let _ = done.send(Completed::Open(
                        id,
                        source,
                        intent_epoch,
                        session_epoch,
                        Box::new(result),
                    ));
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
                retire(id.clone(), slot.session_epoch, session, done.clone());
                slot.meters.unavailable();
                continue;
            }
            let mut frames = [[0.0; 2]; 240];
            let fresh = session.input.drain_frames_observed(&mut frames);
            let capture = session.control.snapshot();
            let level = sum_source(&mut mix, &frames, input.gain_db, input.muted);
            let pre_clip = session.control.take_pre_clip();
            if fresh && capture.level_dbfs.is_some() {
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
                intent_epoch: 1,
                session_epoch: 1,
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
            Completed::Stopped(_, _, true)
        ));
    }

    #[cfg(test)]
    #[test]
    fn disabled_mixer_preserves_real_legacy_capture_samples() {
        let (session, mut pump) = audio::tests::synchronous_mixer_fixture();
        let control = session.control.clone();
        let requested = super::tests::input("fixture", "fixture-source");
        let mut slots = BTreeMap::from([(
            "fixture".into(),
            Slot {
                source: requested.source_id.clone(),
                desired: Some(requested),
                session: Some(session),
                operation: false,
                cancel: Arc::new(AtomicBool::new(false)),
                deadline: None,
                meters: SourceMeters::new("fixture".into()),
                intent_epoch: 1,
                session_epoch: 1,
            },
        )]);
        let (done, _completed) = mpsc::sync_channel(1);
        pump();
        let (mut mix, clipped) = mix_sources(&mut slots, &done);
        assert!(!clipped);
        assert_eq!(mix, [[0.125, -0.25]; 240]);
        let bus = mix_bus::Bus::test_bus();
        let mut output = MixOutput::new();
        output.write(&bus, &mut mix, 5, || 5);
        assert_eq!(mix, [[0.125, -0.25]; 240]);
        assert_eq!(output.reduction, None);
        assert!(control.stop());
    }

    #[cfg(test)]
    #[test]
    fn fault_retry_gets_a_new_actual_lifetime_even_when_parent_intent_is_still_opening() {
        let requested = super::tests::input("fixture", "fixture-source");
        let mut parent = RequestHistory::default();
        let first = parent.update(1, vec![requested.clone()], true, false);
        let mut slot = Slot {
            source: requested.source_id.clone(),
            desired: Some(requested.clone()),
            session: None,
            operation: false,
            cancel: Arc::new(AtomicBool::new(false)),
            deadline: None,
            meters: SourceMeters::new(requested.strip_id.clone()),
            intent_epoch: first.sources[0].epoch,
            session_epoch: 0,
        };
        let drift = ProcessingMode {
            enabled: true,
            epoch: 1,
        };
        let mut nonce = 0;
        slot.begin_open(&mut nonce);
        let old_session = slot.session_epoch;
        let (session, _pump) = audio::tests::synchronous_mixer_fixture_with_correction();
        slot.activate(session, drift);
        slot.session
            .as_ref()
            .unwrap()
            .control
            .simulate_application_exit();
        let (done, completed) = mpsc::sync_channel(1);
        let mut slots = BTreeMap::from([("fixture".into(), slot)]);
        mix_sources(&mut slots, &done);
        let Completed::Stopped(_, epoch, true) =
            completed.recv_timeout(Duration::from_secs(1)).unwrap()
        else {
            panic!("fixture cleanup")
        };
        let slot = slots.get_mut("fixture").unwrap();
        assert_eq!(epoch, old_session);
        assert_eq!(slot.desired.as_ref().unwrap().phase, InputPhase::Fault);
        slot.operation = false;
        let retry = parent.update(2, vec![requested], true, false);
        assert_eq!(first.sources[0].epoch, retry.sources[0].epoch);
        slot.desired = Some(retry.sources[0].input.clone());
        slot.begin_open(&mut nonce);
        assert_ne!(slot.session_epoch, old_session);
        assert!(!slot.accepts_open("fixture-source", slot.intent_epoch, old_session));
        assert!(slot.accepts_open("fixture-source", slot.intent_epoch, slot.session_epoch));
        assert!(!slot.accepts_open("other-source", slot.intent_epoch, slot.session_epoch));
        assert!(!slot.accepts_open("fixture-source", slot.intent_epoch + 1, slot.session_epoch));
        let (session, _pump) = audio::tests::synchronous_mixer_fixture_with_correction();
        slot.activate(session, drift);
        let session = slot.session.as_mut().unwrap();
        assert!(
            session
                .input
                .timing_observation()
                .unwrap()
                .epoch
                .starts_with(&format!("{}:1:", slot.session_epoch))
        );
        session.control.stop();
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
            let (mut session, pump) = audio::tests::synchronous_mixer_fixture_with_correction();
            session
                .input
                .configure_clock_correction(true, index as u64 + 1, 1);
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
                    intent_epoch: index as u64 + 1,
                    session_epoch: index as u64 + 1,
                },
            );
        }
        let (done, _completed) = mpsc::sync_channel(16);
        let bus = mix_bus::Bus::test_bus();
        let mut next_session_epoch = MAX_INPUTS as u64;
        let mut output = MixOutput::new();
        output.configure(ProcessingMode {
            enabled: true,
            epoch: 1,
        });
        let allocations = allocations_during(|| {
            for tick in 0..1000 {
                for pump in &mut pumps {
                    pump();
                }
                open_pending_sources(&mut slots, &done, &mut next_session_epoch);
                let (mut mix, _) = mix_sources(&mut slots, &done);
                output.write(&bus, &mut mix, tick * 5, || tick * 5);
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
        let (desired, latest) = std::sync::mpsc::sync_channel::<SourceUpdate>(1);
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
                    AudioCommand::Sources(update) => {
                        if !valid_update(&update) {
                            break;
                        }
                        if desired.send(update).is_err() {
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
        let mut waiting = BTreeMap::<String, SourceIntent>::new();
        let mut revision = 0;
        let mut drift = ProcessingMode::default();
        let mut mix_output = MixOutput::new();
        let mut next_session_epoch = 0;
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
            while let Ok(update) = latest.try_recv() {
                lifecycle_changed = true;
                revision = update.revision;
                let drift_changed = drift != update.drift;
                drift = update.drift;
                mix_output.configure(update.limiter);
                if drift_changed {
                    for slot in slots.values_mut() {
                        slot.meters.unavailable();
                    }
                }
                waiting.retain(|id, _| {
                    update
                        .sources
                        .iter()
                        .any(|source| &source.input.strip_id == id)
                });
                for (id, slot) in &mut slots {
                    let new = update
                        .sources
                        .iter()
                        .find(|source| &source.input.strip_id == id);
                    let needs_stop = new.is_none_or(|source| {
                        source.epoch != slot.intent_epoch
                            || source.input.source_id != slot.source
                            || !active(&source.input)
                    });
                    slot.desired = new.map(|source| source.input.clone());
                    if let Some(source) = new {
                        slot.intent_epoch = source.epoch;
                    }
                    if needs_stop {
                        slot.cancel.store(true, Ordering::Release);
                        slot.meters.unavailable();
                        if let Some(session) = slot.session.take() {
                            slot.operation = true;
                            slot.deadline = Some(Instant::now() + Duration::from_millis(750));
                            retire(id.clone(), slot.session_epoch, session, done.clone());
                        }
                    } else if let Some(session) = &mut slot.session {
                        if drift_changed {
                            session.input.configure_clock_correction(
                                drift.enabled,
                                slot.session_epoch,
                                drift.epoch,
                            );
                        }
                        // Parent control can still say Opening while its
                        // acknowledgement is in flight. A retained native
                        // session must not regress to Opening after a fader.
                        if let Some(input) = &mut slot.desired {
                            input.phase = InputPhase::Capturing;
                            input.fault = None;
                            input.restored = false;
                        }
                    }
                }
                for source in update.sources {
                    let input = &source.input;
                    if !slots.contains_key(&input.strip_id) {
                        if slots.len() >= MAX_INPUTS {
                            waiting.insert(input.strip_id.clone(), source);
                            continue;
                        }
                        slots.insert(
                            input.strip_id.clone(),
                            Slot {
                                meters: SourceMeters::new(input.strip_id.clone()),
                                source: input.source_id.clone(),
                                deadline: None,
                                intent_epoch: source.epoch,
                                session_epoch: 0,
                                desired: Some(source.input),
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
                    Completed::Stopped(id, epoch, clean) => {
                        if !clean {
                            std::process::exit(1);
                        }
                        if let Some(slot) = slots.get_mut(&id)
                            && slot.session_epoch == epoch
                        {
                            slot.operation = false;
                            slot.deadline = None;
                        }
                    }
                    Completed::Open(id, source, intent_epoch, session_epoch, result) => {
                        let result = *result;
                        if let Some(slot) = slots.get_mut(&id) {
                            if slot.session_epoch != session_epoch {
                                if let Ok(session) = result {
                                    retire(id, session_epoch, session, done.clone());
                                }
                                continue;
                            }
                            match result {
                                Ok(session)
                                    if slot.accepts_open(&source, intent_epoch, session_epoch) =>
                                {
                                    slot.activate(session, drift);
                                }
                                Ok(session) => {
                                    slot.deadline =
                                        Some(Instant::now() + Duration::from_millis(750));
                                    retire(id, session_epoch, session, done.clone());
                                }
                                Err(error) => {
                                    if error.code == ErrorCode::CleanupIncomplete {
                                        std::process::exit(1);
                                    }
                                    slot.operation = false;
                                    slot.deadline = None;
                                    if !slot.cancel.load(Ordering::Acquire)
                                        && slot.session_epoch == session_epoch
                                        && slot.intent_epoch == intent_epoch
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
                    let Some((id, source)) = waiting.pop_first() else {
                        break;
                    };
                    slots.insert(
                        id,
                        Slot {
                            meters: SourceMeters::new(source.input.strip_id.clone()),
                            source: source.input.source_id.clone(),
                            deadline: None,
                            intent_epoch: source.epoch,
                            session_epoch: 0,
                            desired: Some(source.input),
                            session: None,
                            operation: false,
                            cancel: Arc::new(AtomicBool::new(false)),
                        },
                    );
                }
                open_pending_sources(&mut slots, &done, &mut next_session_epoch);
            }
            let started_ms = mix_bus::now_ms();
            let (mut mix, _) = mix_sources(&mut slots, &done);
            mix_output.write(mapping.bus(), &mut mix, started_ms, mix_bus::now_ms);
            if Instant::now() >= publish {
                publish = Instant::now() + Duration::from_millis(100);
                let _ = statuses.try_send(AudioSnapshot {
                    limiter_reduction_db: mix_output.reduction,
                    observed_at_ms: Some(audio::observation_clock_ms()),
                    mix_pre_clip: mix_output.pre_clip,
                    revision,
                    inputs: slots.values().filter_map(|s| s.desired.clone()).collect(),
                    telemetry: slots
                        .values_mut()
                        .filter(|slot| slot.desired.is_some())
                        .map(|slot| {
                            let mut meter = slot.meters.latest.clone();
                            meter.timing = slot
                                .session
                                .as_mut()
                                .and_then(|session| session.input.timing_observation());
                            meter
                        })
                        .collect(),
                });
                for slot in slots.values_mut() {
                    slot.meters.reset_window();
                }
                mix_output.reset_window();
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
                retire(String::new(), slot.session_epoch, session, done.clone());
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
    pub(super) fn input(id: &str, source: &str) -> InputStrip {
        InputStrip {
            strip_id: id.into(),
            source_id: source.into(),
            kind: SourceKind::Recording,
            display_name: "Fixture".into(),
            detail: None,
            phase: InputPhase::Opening,
            fault: None,
            gain_db: Some(0.0),
            muted: false,
            colour_slot: 0,
            restored: false,
        }
    }

    #[test]
    fn parent_epochs_preserve_coalesced_lifetimes_and_modes_but_ignore_faders_and_mute() {
        let mut history = RequestHistory::default();
        let mut first = input("first", "A");
        let peer = input("peer", "peer-source");
        let a = history.update(1, vec![first.clone(), peer.clone()], true, true);
        first.gain_db = Some(-12.0);
        first.muted = true;
        first.phase = InputPhase::Capturing;
        let controls = history.update(2, vec![first.clone(), peer.clone()], true, true);
        assert_eq!(a.sources[0].epoch, controls.sources[0].epoch);
        assert_eq!(a.drift, controls.drift);
        assert_eq!(a.limiter, controls.limiter);
        first.phase = InputPhase::Stopped;
        history.update(3, vec![first.clone(), peer.clone()], false, false);
        first.phase = InputPhase::Opening;
        let restarted = history.update(4, vec![first.clone(), peer.clone()], true, true);
        assert_ne!(a.sources[0].epoch, restarted.sources[0].epoch);
        assert_eq!(a.sources[1].epoch, restarted.sources[1].epoch);
        assert!(restarted.drift.epoch > a.drift.epoch);
        assert!(restarted.limiter.epoch > a.limiter.epoch);
        first.source_id = "B".into();
        history.update(5, vec![first.clone(), peer.clone()], true, true);
        first.source_id = "A".into();
        let returned = history.update(6, vec![first, peer], true, true);
        assert!(returned.sources[0].epoch > restarted.sources[0].epoch);
        assert_eq!(returned.sources[1].epoch, a.sources[1].epoch);
    }

    #[test]
    fn private_source_commands_validate_identity_size_and_fit_with_maximal_telemetry() {
        let mut inputs: Vec<_> = (0..MAX_INPUTS)
            .map(|index| {
                let mut input = input(
                    &format!("{index}{}", "\\".repeat(63)),
                    &format!("{index}{}", "\\".repeat(1023)),
                );
                input.display_name = "\"".repeat(1024);
                input.detail = Some("\\".repeat(1024));
                input.phase = InputPhase::Fault;
                input.fault = Some(AppError::new(ErrorCode::CaptureFailed, "\"".repeat(256)));
                input
            })
            .collect();
        let update = RequestHistory::default().update(u64::MAX, inputs.clone(), true, true);
        assert!(valid_update(&update));
        let command = crate::protocol::Frame {
            version: crate::protocol::VERSION,
            generation: 1,
            sequence: 1,
            body: AudioCommand::Sources(update.clone()),
        };
        assert!(crate::protocol::encode(&command).is_ok());
        assert!(
            crate::protocol::encode(&crate::protocol::Frame {
                version: crate::protocol::VERSION,
                generation: 1,
                sequence: 2,
                body: AudioCommand::Stop
            })
            .is_ok()
        );
        let timing = TimingObservation {
            epoch: format!("{}:{}:{}", u64::MAX, u64::MAX, u64::MAX),
            phase: TimingPhase::Running,
            applied_adjustment_ppm: Some(-1000.0),
            queued_frames: Some(4096),
            target_frames: Some(4096),
            capacity_frames: 4096,
            gap_output_frames: u64::MAX,
            dropped_input_frames: u64::MAX,
            resync_count: u64::MAX,
            last_reset_reason: Some(TimingResetReason::Discontinuity),
            last_reset_at_ms: Some(9_007_199_254_740_991.0),
            state_observed_at_ms: Some(9_007_199_254_740_991.0),
            audio_observed_at_ms: Some(9_007_199_254_740_991.0),
        };
        let telemetry = inputs
            .iter()
            .map(|input| InputTelemetry {
                strip_id: input.strip_id.clone(),
                observed_at_ms: Some(9_007_199_254_740_991.0),
                level_dbfs: Some(-96.0),
                pre_clip: true,
                dropped_frames: u32::MAX,
                underrun_frames: u32::MAX,
                queued_frames: 4096,
                timing: Some(timing.clone()),
            })
            .collect();
        let status = crate::protocol::Frame {
            version: crate::protocol::VERSION,
            generation: 1,
            sequence: 1,
            body: AudioSnapshot {
                observed_at_ms: Some(9_007_199_254_740_991.0),
                mix_pre_clip: true,
                limiter_reduction_db: Some(800.0),
                revision: u64::MAX,
                inputs: inputs.clone(),
                telemetry,
            },
        };
        let encoded =
            crate::protocol::encode(&status).expect("largest supported status fits protocol");
        println!("largest source status: {} bytes", encoded.len());
        inputs[1].strip_id = inputs[0].strip_id.clone();
        assert!(!valid_update(
            &RequestHistory::default().update(1, inputs, true, false)
        ));
        for mutation in 0..4 {
            let mut invalid = update.clone();
            match mutation {
                0 => invalid.sources[0].input.source_id = "mix-handle:42".into(),
                1 => invalid.sources[0].input.source_id.push('x'),
                2 => invalid.sources[0].epoch = 0,
                _ => invalid.sources[0].input.gain_db = Some(f32::NAN),
            }
            assert!(!valid_update(&invalid));
        }
    }

    #[test]
    fn limiter_keeps_preclip_bypass_and_mode_window_contracts() {
        let bus = crate::mix_bus::Bus::test_bus();
        let mut output = MixOutput::new();
        let mut raw = [[1.5, -1.25]; 240];
        output.write(&bus, &mut raw, 0, || 0);
        assert_eq!(raw, [[1.0, -1.0]; 240]);
        assert!(output.pre_clip);
        assert_eq!(output.reduction, None);
        let on = ProcessingMode {
            enabled: true,
            epoch: 1,
        };
        output.configure(on);
        assert!(!output.pre_clip);
        assert_eq!(output.reduction, Some(0.0));
        let mut loud = [[1.5, -1.25]; 240];
        output.write(&bus, &mut loud, 5, || 5);
        assert!(output.pre_clip);
        assert!(output.reduction.unwrap() > 0.0);
        assert!(
            loud.iter()
                .flatten()
                .all(|sample| sample.abs() <= crate::limiter::SAMPLE_CEILING)
        );
        output.configure(ProcessingMode {
            enabled: true,
            epoch: 3,
        });
        let mut silent = [[0.0; 2]; 240];
        output.write(&bus, &mut silent, 10, || 10);
        assert_eq!(silent, [[0.0; 2]; 240]);
        assert_eq!(output.reduction, Some(0.0));
        output.configure(ProcessingMode {
            enabled: false,
            epoch: 4,
        });
        assert_eq!(output.reduction, None);
    }

    #[test]
    fn limiter_source_edits_preserve_peer_but_engine_gaps_clear_before_bus_write() {
        let bus = crate::mix_bus::Bus::test_bus();
        let mut output = MixOutput::new();
        let mode = ProcessingMode {
            enabled: true,
            epoch: 1,
        };
        output.configure(mode);
        let mut mixed = [[0.75; 2]; 240];
        output.write(&bus, &mut mixed, 0, || 0);
        // A new Sources command/fader keeps this exact mode and shared delay.
        output.configure(mode);
        mixed.fill([0.25; 2]);
        output.write(&bus, &mut mixed, 40, || 40);
        assert_eq!(&mixed[..176], &[[0.75; 2]; 176]);
        let mut cursor = bus.cursor();
        mixed.fill([0.125; 2]);
        output.write(&bus, &mut mixed, 81, || 81);
        assert_eq!(&mixed[..176], &[[0.0; 2]; 176]);
        assert_eq!(&mixed[176..], &[[0.125; 2]; 64]);
        let mut observed = [[9.0; 2]; 240];
        bus.read(&mut cursor, &mut observed, 81);
        assert_eq!(observed, mixed);
        // A pause after source draining/DSP is caught before fresh publication.
        mixed.fill([0.25; 2]);
        output.write(&bus, &mut mixed, 86, || 127);
        assert_eq!(mixed, [[0.0; 2]; 240]);
        mixed.fill([0.125; 2]);
        output.write(&bus, &mut mixed, 132, || 132);
        assert_eq!(&mixed[..176], &[[0.0; 2]; 176]);
    }
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
