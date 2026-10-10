//! Owned, credential-free synthetic native application-capture regression.
use super::ProbeResult;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use echocord_core::audio::{self, application};
use serde::Serialize;
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::{
    Win32::{
        Media::Audio::*,
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
    },
    core::Interface,
};

pub fn dispatch() -> Option<ProbeResult<()>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--application-suite") if args.len() == 1 => Some(suite()),
        Some("--mixing-suite") if args.len() == 1 => Some(mixing_suite()),
        Some("--audio-worker") if args.len() == 2 => {
            let result = args[1]
                .parse::<usize>()
                .map_err(|_| "invalid audio handle")
                .and_then(|raw| {
                    echocord_core::mixing::run_worker(raw).map_err(|_| "audio worker failed")
                });
            std::process::exit(if result.is_ok() { 0 } else { 1 });
        }
        Some("--browser-suite") if args.len() == 5 => Some(browser_suite(&args[1..])),
        Some("--synthetic-render") if args.len() == 4 => Some(render(&args[1..])),
        _ => None,
    }
}

fn browser_suite(args: &[String]) -> ProbeResult<()> {
    use echocord_core::{
        model::{RecordingSource, SourceKind},
        process::EngineProcess,
    };
    use std::{ffi::OsString, path::Path};
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _com = Com;
    // Caller supplies a dedicated ignored profile and synthetic URLs; never an owner profile.
    let browser = EngineProcess::spawn(
        Path::new(&args[0]),
        &[
            OsString::from("-no-remote"),
            OsString::from("-new-instance"),
            OsString::from("-profile"),
            OsString::from(&args[1]),
            OsString::from(&args[2]),
            OsString::from(&args[3]),
        ],
    )?;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut selected = None;
    while Instant::now() < deadline {
        let output = Command::new(std::env::current_exe()?)
            .arg("--list")
            .output()?;
        if !output.status.success() {
            return Err("browser discovery probe failed".into());
        }
        let sources: Vec<RecordingSource> = serde_json::from_slice(&output.stdout)?;
        for source in sources.iter().filter(|source| {
            source.kind == SourceKind::Application
                && source.name.eq_ignore_ascii_case("firefox.exe")
        }) {
            let pid: u32 = source
                .id
                .split(':')
                .nth(1)
                .ok_or("invalid discovered source identity")?
                .parse()?;
            let raw = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            if raw.is_null() {
                continue;
            }
            // SAFETY: one uniquely returned owned query/synchronize handle.
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            if browser.contains_process(&handle)?
                && application::identity_for_pid(pid)? == source.id
            {
                selected = Some((source.id.clone(), handle));
                break;
            }
        }
        if selected.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let (id, held_root) = selected.ok_or("owned browser window root was not discovered")?;
    let _unselected = OwnedRenderer::spawn(48_000, 2233, false)?;
    let mut session = audio::start_capture(&id)?;
    let _ = measure(&mut session, 1000)?;
    let mut live = measure(&mut session, 1500)?;
    for _ in 0..8 {
        if live.root_left > 0.002 && live.child_left > 0.002 {
            break;
        }
        live = measure(&mut session, 1000)?;
    }
    let mut owned_session_witness = Vec::new();
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
    let endpoints = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }?;
    for device_index in 0..unsafe { endpoints.GetCount() }? {
        let endpoint = unsafe { endpoints.Item(device_index) }?;
        let manager: IAudioSessionManager2 = unsafe { endpoint.Activate(CLSCTX_ALL, None) }?;
        let sessions = unsafe { manager.GetSessionEnumerator() }?;
        for index in 0..unsafe { sessions.GetCount() }? {
            let control = unsafe { sessions.GetSession(index) }?;
            let control2: IAudioSessionControl2 = control.cast()?;
            let pid = unsafe { control2.GetProcessId() }?;
            let raw = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            if raw.is_null() {
                continue;
            }
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            if browser.contains_process(&handle)? {
                let volume: ISimpleAudioVolume = control.cast()?;
                let meter: IAudioMeterInformation = control.cast()?;
                owned_session_witness.push(serde_json::json!({"mute":unsafe{volume.GetMute()}?.as_bool(),"volume":unsafe{volume.GetMasterVolume()}?,"peak":unsafe{meter.GetPeakValue()}?}));
            }
        }
    }
    require(
        live.root_left > 0.002 && live.child_left > 0.002,
        "owned browser capture did not contain both fixture markers",
    )?;
    require(
        live.unselected_left < live.root_left * 0.03,
        "unrelated synthetic process leaked into browser root",
    )?;
    require(session.control.stop(), "browser capture cleanup failed")?;
    // Closing the job kills the fixture root and descendants even if its launcher already exited.
    drop(browser);
    require(
        unsafe { WaitForSingleObject(held_root.as_raw_handle(), 3000) }
            == windows_sys::Win32::Foundation::WAIT_OBJECT_0,
        "owned browser window root shutdown was not confirmed",
    )?;
    println!(
        "{}",
        serde_json::json!({"browserRootDiscovered":true,"multipleTabsIncluded":true,"unrelatedProcessIsolated":true,"rootShutdownConfirmed":true,
        "scope":"selected window's owning browser process and children, across tabs/windows; no per-tab isolation", "live":live,"ownedSessionWitness":owned_session_witness})
    );
    Ok(())
}

struct OwnedRenderer {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}
impl OwnedRenderer {
    fn spawn(rate: u32, frequency: u32, child: bool) -> ProbeResult<Self> {
        let mut process = Command::new(std::env::current_exe()?)
            .args([
                "--synthetic-render",
                &rate.to_string(),
                &frequency.to_string(),
                if child { "child" } else { "single" },
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = process.stdin.take().ok_or("fixture stdin unavailable")?;
        let stdout = BufReader::new(process.stdout.take().ok_or("fixture stdout unavailable")?);
        let mut owner = Self {
            process,
            stdin,
            stdout,
        };
        owner.ack()?;
        Ok(owner)
    }
    fn ack(&mut self) -> ProbeResult<()> {
        let mut line = String::new();
        self.stdout.read_line(&mut line)?;
        if line.trim() != "ready" {
            return Err("synthetic renderer did not acknowledge".into());
        }
        Ok(())
    }
    fn command(&mut self, command: &str) -> ProbeResult<()> {
        writeln!(self.stdin, "{command}")?;
        self.stdin.flush()?;
        self.ack()
    }
    fn switch_output(&mut self) -> ProbeResult<bool> {
        writeln!(self.stdin, "switch-output")?;
        self.stdin.flush()?;
        let mut line = String::new();
        self.stdout.read_line(&mut line)?;
        match line.trim() {
            "ready" => Ok(true),
            "unavailable" => Ok(false),
            _ => Err("synthetic output switch did not acknowledge".into()),
        }
    }
    fn terminate(&mut self) -> ProbeResult<()> {
        self.process.kill()?;
        self.process.wait()?;
        Ok(())
    }
}
impl Drop for OwnedRenderer {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn own_session() -> ProbeResult<ISimpleAudioVolume> {
    // SAFETY: renderer initializes MTA before this helper and keeps returned interface on this thread.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
    let endpoint = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }?;
    let manager: IAudioSessionManager2 = unsafe { endpoint.Activate(CLSCTX_ALL, None) }?;
    let sessions = unsafe { manager.GetSessionEnumerator() }?;
    for index in 0..unsafe { sessions.GetCount() }? {
        let session = unsafe { sessions.GetSession(index) }?;
        let identity: IAudioSessionControl2 = session.cast()?;
        if unsafe { identity.GetProcessId() }? == std::process::id() {
            return Ok(session.cast()?);
        }
    }
    Err("owned synthetic audio session was not found".into())
}

fn fixture_stream(
    device: &cpal::Device,
    rate: u32,
    frequency: u32,
    silent: Arc<AtomicBool>,
) -> ProbeResult<cpal::Stream> {
    let config = device
        .supported_output_configs()?
        .find_map(|config| {
            (config.channels() == 2 && config.sample_format() == cpal::SampleFormat::F32)
                .then(|| config.try_with_sample_rate(rate))
                .flatten()
        })
        .ok_or("synthetic 44.1/48 kHz stereo output unavailable")?;
    let render_silent = silent;
    let mut index = 0u64;
    let stream = device.build_output_stream(
        config.config(),
        move |data: &mut [f32], _| {
            for frame in data.as_chunks_mut::<2>().0 {
                let amplitude = if render_silent.load(Ordering::Relaxed) {
                    0.0
                } else {
                    0.015
                };
                let phase = index as f64 / rate as f64 * std::f64::consts::TAU;
                frame[0] = (phase * frequency as f64).sin() as f32 * amplitude;
                frame[1] = (phase * (frequency + 360) as f64).sin() as f32 * amplitude;
                index += 1;
            }
        },
        |_| {},
        None,
    )?;
    Ok(stream)
}

fn render(args: &[String]) -> ProbeResult<()> {
    let rate: u32 = args[0].parse()?;
    let frequency: u32 = args[1].parse()?;
    if ![44_100, 48_000].contains(&rate) || ![997, 1499, 2233].contains(&frequency) {
        return Err("invalid synthetic fixture parameters".into());
    }
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _com = Com;
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("synthetic output unavailable")?;
    let silent = Arc::new(AtomicBool::new(false));
    let mut stream = fixture_stream(&device, rate, frequency, silent.clone())?;
    stream.play()?;
    let mut child = if args[2] == "child" {
        Some(OwnedRenderer::spawn(44_100, 1499, false)?)
    } else {
        None
    };
    // This volume control is only this fixture's session, never the endpoint or another process.
    let volume = own_session()?;
    println!("ready");
    io::stdout().flush()?;
    for line in io::stdin().lock().lines() {
        match line?.as_str() {
            "silence" => {
                silent.store(true, Ordering::Release);
                if let Some(child) = &mut child {
                    child.command("silence")?;
                }
            }
            "resume" => {
                silent.store(false, Ordering::Release);
                if let Some(child) = &mut child {
                    child.command("resume")?;
                }
            }
            "switch-output" => {
                let original = device.id()?;
                let alternate = host
                    .output_devices()?
                    .filter(|candidate| candidate.id().ok().as_ref() != Some(&original))
                    .find_map(|candidate| {
                        fixture_stream(&candidate, rate, frequency, silent.clone()).ok()
                    });
                let Some(alternate) = alternate else {
                    println!("unavailable");
                    io::stdout().flush()?;
                    continue;
                };
                alternate.play()?;
                drop(std::mem::replace(&mut stream, alternate));
            }
            "session-mute" => unsafe { volume.SetMute(true, std::ptr::null()) }?,
            "session-unmute" => unsafe { volume.SetMute(false, std::ptr::null()) }?,
            "session-quarter" => unsafe { volume.SetMasterVolume(0.25, std::ptr::null()) }?,
            "session-full" => unsafe { volume.SetMasterVolume(1.0, std::ptr::null()) }?,
            _ => return Err("invalid synthetic fixture command".into()),
        }
        println!("ready");
        io::stdout().flush()?;
    }
    drop(volume);
    drop(child);
    drop(stream);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Signal {
    root_left: f64,
    root_right: f64,
    child_left: f64,
    unselected_left: f64,
    peak: f32,
    max_queue_frames: u32,
    dropped_frames: u32,
}

fn measure(session: &mut audio::CaptureSession, milliseconds: usize) -> ProbeResult<Signal> {
    let mut samples = Vec::with_capacity(milliseconds * 48 * 2);
    let mut max_queue_frames = 0;
    let mut bytes = [0u8; 1920];
    while samples.len() < milliseconds * 48 * 2 {
        let count = session.input.read(&mut bytes)?;
        if count == 0 {
            return Err("synthetic capture ended early".into());
        }
        for value in bytes[..count].as_chunks::<4>().0 {
            samples.push(f32::from_le_bytes(*value));
        }
        max_queue_frames = max_queue_frames.max(session.control.snapshot().queued_frames);
    }
    let amplitude = |frequency: f64, channel: usize| {
        // Max of bounded 100ms segments avoids interpreting reader's silence/queue drops as phase cancellation.
        samples
            .as_chunks::<9600>()
            .0
            .iter()
            .map(|chunk| {
                let (mut real, mut imag) = (0.0, 0.0);
                for (index, frame) in chunk.as_chunks::<2>().0.iter().enumerate() {
                    let phase = std::f64::consts::TAU * frequency * index as f64 / 48_000.0;
                    real += f64::from(frame[channel]) * phase.cos();
                    imag += f64::from(frame[channel]) * phase.sin();
                }
                (real * real + imag * imag).sqrt() / 2400.0
            })
            .fold(0.0_f64, f64::max)
    };
    Ok(Signal {
        root_left: amplitude(997.0, 0),
        root_right: amplitude(1357.0, 1),
        child_left: amplitude(1499.0, 0),
        unselected_left: amplitude(2233.0, 0),
        peak: samples.iter().copied().map(f32::abs).fold(0.0, f32::max),
        max_queue_frames,
        dropped_frames: session.control.snapshot().dropped_frames,
    })
}
fn require(condition: bool, message: &str) -> ProbeResult<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn suite() -> ProbeResult<()> {
    let mut selected = OwnedRenderer::spawn(48_000, 997, true)?;
    let unselected = OwnedRenderer::spawn(48_000, 2233, false)?;
    let unrelated_id = application::identity_for_pid(unselected.process.id())?;
    let mut positive_control = audio::start_capture(&unrelated_id)?;
    let unrelated_signal = measure(&mut positive_control, 400)?;
    require(
        unrelated_signal.unselected_left > 0.002,
        "unselected positive control was not emitting",
    )?;
    require(
        positive_control.control.stop(),
        "positive control cleanup failed",
    )?;
    drop(positive_control);
    let id = application::identity_for_pid(selected.process.id())?;
    let opened = Instant::now();
    let mut session = audio::start_capture(&id)?;
    let open_ms = opened.elapsed().as_secs_f64() * 1000.0;
    let live = measure(&mut session, 1000)?;
    require(
        live.root_left > 0.002 && live.root_right > 0.002 && live.child_left > 0.002,
        "selected root/stereo/child signal missing",
    )?;
    require(
        live.unselected_left < live.root_left * 0.03,
        "unselected process leaked into selected capture",
    )?;
    selected.command("silence")?;
    let _ = measure(&mut session, 200)?;
    let silent = measure(&mut session, 400)?;
    require(
        silent.peak < 0.00001,
        "application silence did not produce silence",
    )?;
    selected.command("resume")?;
    let _ = measure(&mut session, 200)?;
    let resumed = measure(&mut session, 500)?;
    require(
        resumed.root_left > 0.002 && resumed.child_left > 0.002,
        "selected audio did not resume",
    )?;
    selected.command("session-mute")?;
    let _ = measure(&mut session, 200)?;
    let session_muted = measure(&mut session, 500)?;
    require(
        session_muted.root_left < live.root_left * 0.03 && session_muted.child_left > 0.002,
        "own session mute boundary differs from expected root-only scope",
    )?;
    selected.command("session-unmute")?;
    selected.command("session-quarter")?;
    let _ = measure(&mut session, 200)?;
    let quarter = measure(&mut session, 500)?;
    require(
        quarter.root_left < live.root_left * 0.4 && quarter.root_left > live.root_left * 0.1,
        "own session volume did not reach captured signal",
    )?;
    selected.command("session-full")?;
    let endpoint_change = if selected.switch_output()? {
        let _ = measure(&mut session, 200)?;
        let shifted = measure(&mut session, 400)?;
        require(
            shifted.root_left > 0.002 && shifted.child_left > 0.002,
            "application capture did not follow the owned renderer across playback endpoints",
        )?;
        Some(shifted)
    } else {
        None
    };
    std::thread::sleep(Duration::from_millis(250));
    let pressure = measure(&mut session, 500)?;
    require(
        pressure.max_queue_frames <= 1920 && pressure.dropped_frames > 0,
        "capture queue was not bounded under consumer pause",
    )?;
    session.control.set_output(100, true);
    let muted = measure(&mut session, 200)?;
    require(muted.peak == 0.0, "Echocord mute leaked samples")?;
    session.control.set_output(100, false);
    let stop = Instant::now();
    require(session.control.stop(), "capture cleanup deadline exceeded")?;
    let stop_ms = stop.elapsed().as_secs_f64() * 1000.0;
    require(
        session.input.read(&mut [0u8; 8])? == 0,
        "stop did not produce EOF",
    )?;
    drop(session);
    let mut exited_session = audio::start_capture(&id)?;
    selected.terminate()?;
    std::thread::sleep(Duration::from_millis(100));
    require(
        exited_session.input.read(&mut [0u8; 8]).is_err(),
        "source exit did not fail capture",
    )?;
    require(
        application::validate_source(&id).is_err() && audio::start_capture(&id).is_err(),
        "stale process identity was accepted",
    )?;
    require(
        exited_session.control.stop(),
        "exited source cleanup failed",
    )?;
    let replacement = OwnedRenderer::spawn(48_000, 997, false)?;
    let fresh = application::identity_for_pid(replacement.process.id())?;
    require(fresh != id, "source restart reused stale identity")?;
    let cancel = Instant::now();
    require(
        audio::start_capture_with_cancel(&fresh, &|| true).is_err(),
        "opening cancellation was ignored",
    )?;
    let cancel_ms = cancel.elapsed().as_secs_f64() * 1000.0;
    let checks = std::cell::Cell::new(0);
    let during_open = Instant::now();
    require(
        audio::start_capture_with_cancel(&fresh, &|| {
            checks.set(checks.get() + 1);
            checks.get() > 1
        })
        .is_err(),
        "cancel after opening began was ignored",
    )?;
    require(
        checks.get() > 1,
        "during-open cancellation did not reach the handshake",
    )?;
    let during_open_cancel_ms = during_open.elapsed().as_secs_f64() * 1000.0;
    let reopened = audio::start_capture(&fresh)?;
    require(reopened.control.stop(), "fresh instance cleanup failed")?;
    let report = serde_json::json!({"sampleRate":48000,"channels":2,"openMs":open_ms,"stopMs":stop_ms,"cancelMs":cancel_ms,"duringOpenCancelMs":during_open_cancel_ms,
        "unselectedPositiveControl":unrelated_signal,"ownedPlaybackEndpointChange":endpoint_change,
        "live":live,"silence":silent,"resumed":resumed,"sessionMuted":session_muted,"sessionQuarterVolume":quarter,"consumerPressure":pressure,
        "selectedTreeIsolation":true,"stereo":true,"child44100HzIncluded":true,"sourceExitTerminal":true,"staleIdentityRejected":true,
        "restartRequiresNewIdentity":true,"openingCancellation":true,"endpointMute":"unmeasured: shared desktop endpoint deliberately unchanged",
        "localAudibility":"not physically measured; owned session mute/volume are observed at the capture boundary",
        "independentLocalSuppression":false,"queueAgeBoundary":"native sample exposure to reader only; no listener latency"});
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn mixing_suite() -> ProbeResult<()> {
    use echocord_core::{mix_bus, mixing::AudioHost, model::*};
    use std::os::windows::io::IntoRawHandle;
    let mut first = OwnedRenderer::spawn(48_000, 997, false)?;
    let mut second = OwnedRenderer::spawn(44_100, 1499, false)?;
    let _unselected = OwnedRenderer::spawn(48_000, 2233, false)?;
    let source = |id: &str, process: &OwnedRenderer| -> ProbeResult<InputStrip> {
        Ok(InputStrip {
            strip_id: id.into(),
            source_id: application::identity_for_pid(process.process.id())?,
            kind: SourceKind::Application,
            display_name: "Owned synthetic source".into(),
            detail: None,
            phase: InputPhase::Opening,
            fault: None,
            gain_db: Some(0.0),
            muted: false,
            colour_slot: 0,
            restored: false,
        })
    };
    let mut inputs = vec![source("first", &first)?, source("second", &second)?];
    let host = AudioHost::launch(std::env::current_exe()?)?;
    host.sources(inputs.clone());
    let wait = |predicate: &dyn Fn(&echocord_core::mixing::AudioSnapshot) -> bool| -> ProbeResult<echocord_core::mixing::AudioSnapshot> {
        let until = Instant::now() + Duration::from_secs(8);
        while Instant::now() < until {
            if host.exit.load(Ordering::Acquire) != 0 { return Err("audio worker exited unexpectedly".into()); }
            if let Some(snapshot) = host.snapshot() && predicate(&snapshot) { return Ok(snapshot); }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err("audio worker observation timed out".into())
    };
    let preview = wait(&|s| {
        s.inputs.len() == 2
            && s.inputs.iter().all(|i| i.phase == InputPhase::Capturing)
            && s.telemetry
                .iter()
                .all(|t| t.level_dbfs.is_some_and(|level| level > -70.0))
    })?;
    require(
        preview.telemetry.iter().all(|t| t.queued_frames <= 1920),
        "preview queue exceeded its bound",
    )?;
    std::thread::sleep(Duration::from_millis(800));
    let idle = wait(&|s| s.telemetry.len() == 2)?;
    require(
        idle.telemetry.iter().all(|t| t.dropped_frames < 4800),
        "capture without destination accumulated overflow",
    )?;
    let clock_view =
        mix_bus::Mapping::inherited(host.bus.inheritable()?.into_raw_handle() as usize)?;
    let cursor = clock_view.bus().cursor();
    let started = Instant::now();
    std::thread::sleep(Duration::from_secs(2));
    let frames_per_second =
        (clock_view.bus().cursor() - cursor) as f64 / started.elapsed().as_secs_f64();
    require(
        (47_000.0..=49_000.0).contains(&frames_per_second),
        "shared producer clock is not approximately 48000 frames/sec",
    )?;
    println!("native-shared-mix producer frames/sec: {frames_per_second:.1}");
    drop(clock_view);
    let handle = host.bus.inheritable()?;
    let mut destination =
        mix_bus::capture(&format!("mix-handle:{}", handle.into_raw_handle() as usize))?;
    let _ = measure(&mut destination, 300)?;
    let live = measure(&mut destination, 1000)?;
    require(
        live.root_left > 0.002 && live.root_right > 0.002 && live.child_left > 0.002,
        "two native sources did not reach the stereo mix",
    )?;
    require(
        live.unselected_left < live.root_left * 0.05,
        "unselected application leaked into the mix",
    )?;
    inputs[0].muted = true;
    host.sources(inputs.clone());
    let muted_meter = wait(&|s| {
        s.inputs.first().is_some_and(|i| i.muted)
            && s.telemetry
                .iter()
                .any(|t| t.strip_id == "first" && t.level_dbfs.is_some_and(|v| v > -70.0))
    })?;
    require(
        !muted_meter.telemetry.is_empty(),
        "muted source meter missing",
    )?;
    let _ = measure(&mut destination, 200)?;
    let muted = measure(&mut destination, 800)?;
    require(
        muted.root_left < live.root_left * 0.08 && muted.child_left > live.child_left * 0.6,
        "source mute affected the wrong mix signal",
    )?;
    inputs[0].muted = false;
    inputs[0].gain_db = Some(-12.0412);
    host.sources(inputs.clone());
    wait(&|s| {
        s.inputs
            .iter()
            .any(|i| i.strip_id == "first" && i.gain_db == Some(-12.0412))
    })?;
    let _ = measure(&mut destination, 200)?;
    let gained = measure(&mut destination, 800)?;
    require(
        gained.root_left > live.root_left * 0.1 && gained.root_left < live.root_left * 0.4,
        "fractional source gain did not reach native PCM",
    )?;
    destination.control.set_linear_output(0.125, false);
    let output = measure(&mut destination, 800)?;
    require(
        output.child_left < gained.child_left * 0.2 && output.child_left > gained.child_left * 0.05,
        "destination gain did not reach native PCM",
    )?;
    destination.control.set_linear_output(1.0, true);
    let silence = measure(&mut destination, 300)?;
    require(
        silence.peak == 0.0,
        "destination mute did not produce silence",
    )?;
    destination.control.set_linear_output(1.0, false);
    second.terminate()?;
    wait(&|s| {
        s.inputs
            .iter()
            .any(|i| i.strip_id == "second" && i.phase == InputPhase::Fault)
    })?;
    let _ = measure(&mut destination, 250)?;
    let fault = measure(&mut destination, 800)?;
    require(
        fault.root_left > 0.0005 && fault.child_left < live.child_left * 0.08,
        "one source fault stopped or contaminated the shared mix",
    )?;
    first.command("silence")?;
    let _ = measure(&mut destination, 250)?;
    require(
        measure(&mut destination, 300)?.peak == 0.0,
        "source silence did not remain connected silence",
    )?;
    first.command("resume")?;
    let _ = measure(&mut destination, 200)?;
    require(
        measure(&mut destination, 500)?.root_left > 0.0005,
        "source signal did not resume",
    )?;
    inputs.remove(1);
    inputs[0].phase = InputPhase::Stopped;
    host.sources(inputs.clone());
    wait(&|s| s.inputs.len() == 1 && s.inputs[0].phase == InputPhase::Stopped)?;
    inputs[0].phase = InputPhase::Opening;
    host.sources(inputs.clone());
    wait(&|s| s.inputs.len() == 1 && s.inputs[0].phase == InputPhase::Capturing)?;
    // Rapid remove/add of the same physical source must serialize retirement.
    host.sources(Vec::new());
    inputs[0].strip_id = "replacement".into();
    host.sources(inputs.clone());
    wait(&|s| {
        s.inputs.len() == 1
            && s.inputs[0].strip_id == "replacement"
            && s.inputs[0].phase == InputPhase::Capturing
    })?;
    let _ = measure(&mut destination, 250)?;
    require(
        measure(&mut destination, 500)?.root_left > 0.0005,
        "stable mix reader did not survive source replacement",
    )?;
    require(
        destination.control.stop(),
        "destination PCM reader cleanup failed",
    )?;
    host.stop();
    let until = Instant::now() + Duration::from_secs(5);
    while host.exit.load(Ordering::Acquire) == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    require(
        host.exit.load(Ordering::Acquire) == 1,
        "audio worker exit was not observed",
    )?;
    println!(
        "native-shared-mix: passed; two sources, no credentials, idle drain, stereo isolation, fractional gains, source/destination mute, source fault, silence/resume, stop/start/replace, observed cleanup"
    );
    Ok(())
}
