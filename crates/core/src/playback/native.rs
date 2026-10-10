use super::*;
use crate::{
    mix_bus::{self, BusHandle, Mapping},
    model::{ConnectStage, Connection, Output},
    process::EngineProcess,
    protocol::{self, Frame, Validator},
};
use cpal::{
    DeviceId, HostId, SampleFormat, SizedSample, SupportedStreamConfig, SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    io::{BufReader, Read},
    os::windows::io::{AsHandle, AsRawHandle},
    path::PathBuf,
    sync::{Condvar, Mutex, atomic::AtomicU8, mpsc},
    time::{Duration, Instant},
};

fn host() -> Result<cpal::Host, AppError> {
    cpal::host_from_id(HostId::Wasapi).map_err(|_| unavailable("Windows playback is unavailable."))
}
fn select_config(
    ranges: impl Iterator<Item = SupportedStreamConfigRange>,
) -> Option<SupportedStreamConfig> {
    ranges
        .filter(|range| range.channels() == 2)
        .filter_map(|range| {
            let rank = match range.sample_format() {
                SampleFormat::F32 => 0,
                SampleFormat::I16 => 1,
                SampleFormat::U16 => 2,
                _ => return None,
            };
            range
                .try_with_sample_rate(48_000)
                .map(|config| (rank, config))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, config)| config)
}
pub fn enumerate_devices() -> Result<Vec<PlaybackDevice>, AppError> {
    let mut devices = Vec::new();
    for device in host()?
        .output_devices()
        .map_err(|_| unavailable("Cannot enumerate playback outputs."))?
    {
        let id = device
            .id()
            .map_err(|_| unavailable("Cannot identify playback output."))?
            .to_string();
        let name = device
            .description()
            .map_err(|_| unavailable("Cannot read playback output name."))?
            .name()
            .to_owned();
        let supported = device
            .supported_output_configs()
            .ok()
            .and_then(select_config)
            .is_some();
        devices.push(PlaybackDevice {
            id,
            name,
            available: supported,
            detail: Some(
                if supported {
                    "48 kHz stereo playback"
                } else {
                    "Requires supported 48 kHz stereo PCM."
                }
                .into(),
            ),
        });
    }
    Ok(devices)
}
fn open(
    device_id: &str,
    mapping: Mapping,
    control: Arc<RenderControl>,
) -> Result<cpal::Stream, AppError> {
    let id: DeviceId = device_id
        .parse()
        .map_err(|_| unavailable("The playback endpoint identity is invalid."))?;
    if id.host() != HostId::Wasapi {
        return Err(unavailable("Select a Windows WASAPI playback endpoint."));
    }
    let device = host()?
        .output_devices()
        .map_err(|_| unavailable("Cannot enumerate playback outputs."))?
        .find(|device| device.id().ok().as_ref() == Some(&id))
        .ok_or_else(|| {
            unavailable("The selected playback output is unavailable. Select it again.")
        })?;
    let config = device
        .supported_output_configs()
        .ok()
        .and_then(select_config)
        .ok_or_else(|| {
            unavailable("This playback output requires a supported 48 kHz stereo format.")
        })?;
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, mapping, control),
        SampleFormat::I16 => build::<i16>(&device, config, mapping, control),
        SampleFormat::U16 => build::<u16>(&device, config, mapping, control),
        _ => return Err(unavailable("The playback PCM format is unsupported.")),
    }
    .map_err(|_| unavailable("Cannot open the selected playback output."))?;
    stream
        .play()
        .map_err(|_| unavailable("Cannot start the selected playback output."))?;
    Ok(stream)
}
fn build<T>(
    device: &cpal::Device,
    config: SupportedStreamConfig,
    mapping: Mapping,
    control: Arc<RenderControl>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + cpal::FromSample<f32> + Copy,
{
    let mut renderer = Renderer::attach(mapping.bus(), control.clone());
    device.build_output_stream(
        config.config(),
        move |data: &mut [T], _| {
            renderer.render(mapping.bus(), data, mix_bus::now_ms(), T::from_sample)
        },
        move |_| {
            control.failed.store(true, SeqCst);
        },
        None,
    )
}

#[derive(Serialize, Deserialize)]
enum Command {
    Init { device_id: String, output: Output },
    Output(Output),
    Stop,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PlaybackSnapshot {
    pub connection: Connection,
    pub observation: RenderObservation,
}
struct Mailbox {
    output: Option<Output>,
    stopped: Option<Instant>,
}
pub struct PlaybackHost {
    mailbox: Arc<(Mutex<Mailbox>, Condvar)>,
    snapshot: Arc<Mutex<Option<PlaybackSnapshot>>>,
    pub exit: Arc<AtomicU8>,
    pub process_id: Arc<AtomicU32>,
}
impl PlaybackHost {
    pub fn launch(
        executable: PathBuf,
        bus: Arc<BusHandle>,
        device_id: String,
        output: Output,
        generation: u32,
    ) -> Result<Self, AppError> {
        let inherited = bus
            .inheritable()
            .map_err(|_| unavailable("Cannot share the audio mix with this output."))?;
        let mailbox = Arc::new((
            Mutex::new(Mailbox {
                output: None,
                stopped: None,
            }),
            Condvar::new(),
        ));
        let snapshot = Arc::new(Mutex::new(None));
        let exit = Arc::new(AtomicU8::new(0));
        let process_id = Arc::new(AtomicU32::new(0));
        let opened = Arc::new(AtomicBool::new(false));
        let owner = Self {
            mailbox: mailbox.clone(),
            snapshot: snapshot.clone(),
            exit: exit.clone(),
            process_id: process_id.clone(),
        };
        std::thread::spawn(move || {
            let args = [
                "--playback-worker".into(),
                (inherited.as_raw_handle() as usize).to_string().into(),
            ];
            let Ok(mut process) =
                EngineProcess::spawn_with_handles(&executable, &args, &[inherited.as_handle()])
            else {
                exit.store(1, SeqCst);
                return;
            };
            process_id.store(process.id(), SeqCst);
            drop(inherited);
            let (Some(mut stdin), Some(stdout), Some(mut stderr)) = (
                process.take_stdin(),
                process.take_stdout(),
                process.take_stderr(),
            ) else {
                let _ = process.terminate();
                exit.store(2, SeqCst);
                return;
            };
            let writer_mail = mailbox.clone();
            std::thread::spawn(move || {
                let (lock, wake) = &*writer_mail;
                let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                if state.stopped.is_some() {
                    return;
                }
                let output = state.output.take().unwrap_or(output);
                drop(state);
                let mut sequence = 1;
                if protocol::write(
                    &mut stdin,
                    &Frame {
                        version: protocol::VERSION,
                        generation,
                        sequence,
                        body: Command::Init { device_id, output },
                    },
                )
                .is_err()
                {
                    stop_mailbox(&writer_mail);
                    return;
                }
                loop {
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while state.output.is_none() && state.stopped.is_none() {
                        state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    let stopping = state.stopped.is_some();
                    let body = if stopping {
                        Command::Stop
                    } else {
                        Command::Output(state.output.take().expect("pending output"))
                    };
                    drop(state);
                    sequence += 1;
                    if protocol::write(
                        &mut stdin,
                        &Frame {
                            version: protocol::VERSION,
                            generation,
                            sequence,
                            body,
                        },
                    )
                    .is_err()
                    {
                        stop_mailbox(&writer_mail);
                        break;
                    }
                    if stopping {
                        break;
                    }
                }
            });
            let reader_mail = mailbox.clone();
            let reader_opened = opened.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut validator =
                    Validator::new(generation).expect("nonzero destination generation");
                while let Ok(Some(frame)) = protocol::read::<PlaybackSnapshot>(&mut reader) {
                    if validator.accept(&frame).is_err() {
                        break;
                    }
                    if !matches!(frame.body.connection, Connection::Connecting { .. }) {
                        reader_opened.store(true, SeqCst);
                    }
                    *snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame.body);
                }
                stop_mailbox(&reader_mail);
            });
            std::thread::spawn(move || {
                let mut buffer = [0; 4096];
                while stderr.read(&mut buffer).is_ok_and(|n| n > 0) {}
            });
            let started = Instant::now();
            loop {
                match process.try_wait() {
                    Ok(Some(_)) => {
                        stop_mailbox(&mailbox);
                        exit.store(1, SeqCst);
                        break;
                    }
                    Err(_) => stop_mailbox(&mailbox),
                    _ => {}
                }
                if started.elapsed() > Duration::from_secs(8) && !opened.load(SeqCst) {
                    stop_mailbox(&mailbox);
                }
                if let Some(stopped) = mailbox.0.lock().unwrap_or_else(|e| e.into_inner()).stopped {
                    if stopped.elapsed() > Duration::from_millis(750) {
                        let _ = process.terminate();
                    }
                    if stopped.elapsed() > Duration::from_millis(3750) {
                        // Keep the owned process handle until a later observed exit.
                        // The UI blocks reuse while cleanup is unconfirmed.
                        exit.store(2, SeqCst);
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        Ok(owner)
    }
    pub fn output(&self, output: Output) {
        let (lock, wake) = &*self.mailbox;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped.is_none() {
            state.output = Some(output);
            wake.notify_one();
        }
    }
    pub fn stop(&self) {
        stop_mailbox(&self.mailbox);
    }
    pub fn snapshot(&self) -> Option<PlaybackSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }
}
impl Drop for PlaybackHost {
    fn drop(&mut self) {
        self.stop();
    }
}
fn stop_mailbox(mailbox: &Arc<(Mutex<Mailbox>, Condvar)>) {
    let (lock, wake) = &**mailbox;
    lock.lock()
        .unwrap_or_else(|e| e.into_inner())
        .stopped
        .get_or_insert_with(Instant::now);
    wake.notify_one();
}

pub fn run_worker(raw: usize) -> Result<(), AppError> {
    let mapping =
        Mapping::inherited(raw).map_err(|_| unavailable("The shared mix is unavailable."))?;
    let mut reader = BufReader::new(std::io::stdin());

    let frame = protocol::read::<Command>(&mut reader)
        .map_err(|_| unavailable("Invalid playback protocol."))?
        .ok_or_else(|| unavailable("Playback startup was cancelled."))?;
    let generation = frame.generation;
    let mut validator =
        Validator::new(generation).map_err(|_| unavailable("Invalid playback generation."))?;
    validator
        .accept(&frame)
        .map_err(|_| unavailable("Invalid playback protocol."))?;
    let Command::Init { device_id, output } = frame.body else {
        return Err(unavailable("Invalid playback initialization."));
    };
    if device_id.is_empty() || device_id.len() > 1024 {
        return Err(unavailable("Invalid playback endpoint."));
    }
    let control = Arc::new(RenderControl::default());
    control.set_output(
        output
            .volume_linear
            .unwrap_or(output.volume_percent as f32 / 100.0),
        output.muted,
    );
    let commands = control.clone();
    std::thread::spawn(move || {
        while let Ok(Some(frame)) = protocol::read::<Command>(&mut reader) {
            if validator.accept(&frame).is_err() {
                break;
            }
            match frame.body {
                Command::Output(output) => commands.set_output(
                    output
                        .volume_linear
                        .unwrap_or(output.volume_percent as f32 / 100.0),
                    output.muted,
                ),
                Command::Stop | Command::Init { .. } => break,
            }
        }
        commands.stop();
    });
    let (updates, snapshots) = mpsc::sync_channel::<PlaybackSnapshot>(1);
    let writer_control = control.clone();
    std::thread::spawn(move || {
        let mut output = std::io::stdout();
        let mut sequence = 0;
        while let Ok(body) = snapshots.recv() {
            sequence += 1;
            if protocol::write(
                &mut output,
                &Frame {
                    version: protocol::VERSION,
                    generation,
                    sequence,
                    body,
                },
            )
            .is_err()
            {
                writer_control.stop();
                break;
            }
        }
    });
    let _ = updates.try_send(PlaybackSnapshot {
        connection: Connection::Connecting {
            stage: ConnectStage::Capture,
        },
        observation: RenderObservation::default(),
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !mapping.bus().valid() && Instant::now() < deadline && !control.stopped.load(SeqCst) {
        std::thread::sleep(Duration::from_millis(2));
    }
    if control.stopped.load(SeqCst) {
        return Ok(());
    }
    if !mapping.bus().valid() {
        return Err(unavailable("The shared mix is unavailable."));
    }
    let stream = match open(&device_id, mapping, control.clone()) {
        Ok(stream) => stream,
        Err(error) => {
            let _ = updates.send(PlaybackSnapshot {
                connection: Connection::Error {
                    code: error.code.clone(),
                    message: error.message.clone(),
                },
                observation: RenderObservation::default(),
            });
            std::thread::sleep(Duration::from_millis(20));
            return Err(error);
        }
    };
    while !control.stopped.load(SeqCst) {
        if control.callback_frames.load(SeqCst) > 0
            && mix_bus::now_ms().saturating_sub(control.meter_ms.load(SeqCst)) > 2000
        {
            control.failed.store(true, SeqCst);
        }
        let connection = if control.failed.load(SeqCst) {
            Connection::Error {
                code: ErrorCode::ConnectionFailed,
                message:
                    "The selected playback output stopped. Select it again or press Play to retry."
                        .into(),
            }
        } else if control.callback_frames.load(SeqCst) == 0 {
            Connection::Connecting {
                stage: ConnectStage::Capture,
            }
        } else {
            Connection::Connected
        };
        let failed = control.failed.load(SeqCst);
        let _ = updates.try_send(PlaybackSnapshot {
            connection,
            observation: control.snapshot(mix_bus::now_ms()),
        });
        if failed {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    control.stop();
    drop(stream);
    Ok(())
}

/// Read-only native probe inventory; callers keep endpoint/session identity private.
pub fn inspect_devices() -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    use windows::{
        Win32::{
            Media::Audio::*,
            System::Com::{
                CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
                CoUninitialize,
            },
        },
        core::HSTRING,
    };
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _com = Com;
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
    let endpoint_id =
        |device: IMMDevice| -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let raw = unsafe { device.GetId() }?;
            let id = unsafe { raw.to_string() };
            unsafe {
                CoTaskMemFree(Some(raw.0.cast()));
            }
            Ok(id?)
        };
    let defaults: Vec<_> = [
        ("console", eConsole),
        ("multimedia", eMultimedia),
        ("communications", eCommunications),
    ]
    .into_iter()
    .filter_map(|(role, kind)| {
        unsafe { enumerator.GetDefaultAudioEndpoint(eRender, kind) }
            .ok()
            .and_then(|device| endpoint_id(device).ok())
            .map(|id| (role, id))
    })
    .collect();
    let mut result = Vec::new();
    for device in enumerate_devices()? {
        let id: DeviceId = device.id.parse()?;
        let endpoint = unsafe { enumerator.GetDevice(&HSTRING::from(id.id())) }?;
        let manager: IAudioSessionManager2 = unsafe { endpoint.Activate(CLSCTX_ALL, None) }?;
        let sessions = unsafe { manager.GetSessionEnumerator() }?;
        let mut active = Vec::new();
        for index in 0..unsafe { sessions.GetCount() }? {
            let session = unsafe { sessions.GetSession(index) }?;
            if unsafe { session.GetState() }? == AudioSessionStateActive {
                use windows::core::Interface;
                let identity: IAudioSessionControl2 = session.cast()?;
                active.push(unsafe { identity.GetProcessId() }?);
            }
        }
        result.push(serde_json::json!({"device": device, "defaultRoles": defaults.iter().filter(|(_, endpoint)| endpoint == id.id()).map(|(role, _)| *role).collect::<Vec<_>>(), "activeSessionPids": active}));
    }
    Ok(serde_json::Value::Array(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playback_format_selection_requires_explicit_48k_stereo() {
        use cpal::SupportedBufferSize;
        let range = |channels, min, max, format| {
            SupportedStreamConfigRange::new(
                channels,
                min,
                max,
                SupportedBufferSize::Unknown,
                format,
            )
        };
        assert!(
            select_config(
                [
                    range(2, 44100, 44100, SampleFormat::F32),
                    range(1, 48000, 48000, SampleFormat::F32)
                ]
                .into_iter()
            )
            .is_none()
        );
        let selected = select_config(
            [
                range(2, 44100, 96000, SampleFormat::I16),
                range(2, 48000, 48000, SampleFormat::F32),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(selected.sample_rate(), 48000);
        assert_eq!(selected.channels(), 2);
        assert_eq!(selected.sample_format(), SampleFormat::F32);
    }
}
