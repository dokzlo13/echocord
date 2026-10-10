//! Parent control surface. This module never creates a gateway client or handles PCM.
use crate::{
    model::*,
    protocol::{self, Failure},
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::Instant,
};

const MAX_SETTINGS_BYTES: u64 = 65536;

#[derive(Clone)]
pub struct ControllerConfig {
    pub executable: PathBuf,
    pub token_file: Option<PathBuf>,
    pub settings_file: Option<PathBuf>,
}
impl fmt::Debug for ControllerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ControllerConfig { .. }")
    }
}

#[derive(Clone)]
pub struct Controller {
    commands: mpsc::Sender<Request>,
    snapshots: watch::Receiver<ControlSnapshot>,
}

enum Request {
    Activate,
    Dispatch(ControlCommand, oneshot::Sender<Result<(), AppError>>),
    // Intentionally nonserialized and without Debug: only the shell's private
    // configuration RPC may carry this short-lived secret into the actor.
    ConfigureToken(String, oneshot::Sender<Result<(), AppError>>),
    Shutdown(oneshot::Sender<Result<(), AppError>>),
}
enum Event {
    PlaybackDevices(u64, Result<Vec<PlaybackDevice>, AppError>),
    PlaybackScanFinished(u64, Option<Result<Vec<PlaybackDevice>, AppError>>),
    Sources(u64, Result<Vec<RecordingSource>, AppError>),
    SourceScanFinished(u64, Option<Result<Vec<RecordingSource>, AppError>>),
    Catalog(u64, Result<Catalog, AppError>),
    Unconfigured(u64),
    Configured(u64, Result<(), AppError>),
}

impl Controller {
    /// Requires a Tokio runtime. Starts capture restoration and REST discovery.
    pub fn spawn(config: ControllerConfig) -> Self {
        Self::spawn_mode(config, true)
    }
    /// Prepare the control channel without discovery, capture or worker creation.
    /// The desktop shell activates it only after single-instance admission.
    pub fn prepare(config: ControllerConfig) -> Self {
        Self::spawn_mode(config, false)
    }
    pub fn activate(&self) {
        let _ = self.commands.try_send(Request::Activate);
    }
    fn spawn_mode(config: ControllerConfig, active: bool) -> Self {
        let (commands, receiver) = mpsc::channel(32);
        let (snapshots, snapshot_rx) = watch::channel(ControlSnapshot::default());
        let (events, event_rx) = mpsc::channel(32);
        tokio::spawn(Actor::new(config, snapshots, events).run(receiver, event_rx, active));
        Self {
            commands,
            snapshots: snapshot_rx,
        }
    }
    pub fn snapshot(&self) -> ControlSnapshot {
        self.snapshots.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<ControlSnapshot> {
        self.snapshots.clone()
    }
    pub async fn dispatch(&self, command: ControlCommand) -> Result<(), AppError> {
        let (send, recv) = oneshot::channel();
        self.commands
            .send(Request::Dispatch(command, send))
            .await
            .map_err(|_| unavailable())?;
        recv.await.map_err(|_| unavailable())?
    }
    /// Private configuration path, separate from the shared control-plane model.
    /// Neither requests nor tokens may be logged or debug formatted by callers.
    pub async fn configure_token(&self, token: String) -> Result<(), AppError> {
        let (send, recv) = oneshot::channel();
        self.commands
            .send(Request::ConfigureToken(token, send))
            .await
            .map_err(|_| unavailable())?;
        recv.await.map_err(|_| unavailable())?
    }
    pub async fn shutdown(&self) -> Result<(), AppError> {
        let (send, recv) = oneshot::channel();
        self.commands
            .send(Request::Shutdown(send))
            .await
            .map_err(|_| unavailable())?;
        recv.await.map_err(|_| unavailable())?
    }
}

fn error(code: ErrorCode, message: &str) -> AppError {
    AppError {
        code,
        message: message.into(),
    }
}
fn unavailable() -> AppError {
    error(ErrorCode::Internal, "The controller is unavailable.")
}
fn busy() -> AppError {
    error(ErrorCode::Busy, "Wait for the current operation to finish.")
}
fn cleanup_error() -> AppError {
    error(
        ErrorCode::CleanupIncomplete,
        "Engine exit could not be confirmed. Quit and restart the application.",
    )
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preferences {
    #[serde(default)]
    version: u8,
    #[serde(default)]
    selection: Selection,
    output: Option<Output>,
    #[serde(default)]
    inputs: Vec<InputStrip>,
    #[serde(default)]
    destinations: Vec<DestinationStrip>,
    #[serde(default)]
    settings: AppSettings,
}

fn separate_settings(config: &ControllerConfig) -> bool {
    match (&config.settings_file, &config.token_file) {
        (Some(settings), Some(token)) => {
            settings != token
                && match (settings.canonicalize(), token.canonicalize()) {
                    (Ok(a), Ok(b)) => a != b,
                    _ => true,
                }
        }
        _ => true,
    }
}
fn load_preferences(config: &ControllerConfig) -> Result<Preferences, AppError> {
    if !separate_settings(config) {
        return Err(Failure::Configuration.error());
    }
    let Some(path) = &config.settings_file else {
        return Ok(Preferences::default());
    };
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::default()),
        Err(_) => return Err(Failure::Configuration.error()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::Configuration.error())?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(Failure::Configuration.error());
    }
    let mut prefs: Preferences =
        serde_json::from_slice(&bytes).map_err(|_| Failure::Configuration.error())?;
    if prefs.version > 4
        || prefs.inputs.len() > MAX_INPUTS
        || prefs.destinations.len() > MAX_DESTINATIONS
        || (prefs.version < 4
            && prefs
                .destinations
                .iter()
                .filter(|d| d.kind == DestinationKind::Discord)
                .count()
                > 1)
        || prefs
            .destinations
            .iter()
            .filter(|d| d.kind == DestinationKind::Playback)
            .count()
            > MAX_PLAYBACK_DESTINATIONS
        || prefs
            .inputs
            .iter()
            .any(|i| !crate::mixing::valid_gain(i.gain_db))
        || prefs
            .destinations
            .iter()
            .any(|d| !crate::mixing::valid_gain(d.volume_db))
    {
        return Err(Failure::Configuration.error());
    }
    if prefs.version < 2 {
        if let Some(source_id) = &prefs.selection.source_id
            && !crate::audio::application::is_application_id(source_id)
        {
            prefs.inputs.push(InputStrip {
                strip_id: "input-1".into(),
                source_id: source_id.clone(),
                kind: SourceKind::Recording,
                display_name: "Saved recording input".into(),
                detail: None,
                phase: InputPhase::Stopped,
                fault: None,
                gain_db: Some(0.0),
                muted: false,
                colour_slot: 0,
                restored: true,
            });
        }
        if let (Some(server_id), Some(channel_id)) =
            (&prefs.selection.server_id, &prefs.selection.channel_id)
        {
            let output = prefs
                .output
                .clone()
                .unwrap_or(ControlSnapshot::default().output);
            prefs.destinations.push(DestinationStrip {
                session_id: 0,
                kind: DestinationKind::Discord,
                device_id: None,
                display_name: None,
                destination_id: "discord-1".into(),
                server_id: server_id.clone(),
                channel_id: channel_id.clone(),
                connection: Connection::Disconnected,
                volume_db: if output.volume_percent == 0 {
                    None
                } else {
                    Some(20.0 * (output.volume_percent as f32 / 100.0).log10())
                },
                muted: output.muted,
            });
        }
    }
    let mut strips = std::collections::HashSet::new();
    let mut sources = std::collections::HashSet::new();
    for input in &mut prefs.inputs {
        if input.source_id.is_empty()
            || input.source_id.len() > 1024
            || input.source_id.starts_with("mix-handle:")
            || input.strip_id.is_empty()
            || input.strip_id.len() > 64
            || input.display_name.len() > 1024
            || input.display_name.chars().any(char::is_control)
            || input.source_id.chars().any(char::is_control)
            || input
                .detail
                .as_ref()
                .is_some_and(|detail| detail.chars().any(char::is_control))
            || input
                .detail
                .as_ref()
                .is_some_and(|detail| detail.len() > 1024)
            || !strips.insert(input.strip_id.clone())
            || !sources.insert(input.source_id.clone())
        {
            return Err(Failure::Configuration.error());
        }
        input.kind = if crate::audio::application::is_application_id(&input.source_id) {
            SourceKind::Application
        } else {
            SourceKind::Recording
        };
    }
    let mut destination_ids = std::collections::HashSet::new();
    let mut playback_ids = std::collections::HashSet::new();
    let mut discord_servers = std::collections::HashSet::new();
    if prefs.destinations.iter().any(|d| {
        !destination_ids.insert(d.destination_id.clone())
            || d.destination_id.len() > 64
            || d.display_name
                .as_ref()
                .is_some_and(|name| name.len() > 1024 || name.chars().any(char::is_control))
            || match d.kind {
                DestinationKind::Discord => {
                    !d.destination_id.starts_with("discord-")
                        || !discord_servers.insert(d.server_id.clone())
                        || d.device_id.is_some()
                        || protocol::discord_id(&d.server_id).is_none()
                        || protocol::discord_id(&d.channel_id).is_none()
                }
                DestinationKind::Playback => {
                    !d.destination_id.starts_with("playback-")
                        || !d.server_id.is_empty()
                        || !d.channel_id.is_empty()
                        || d.device_id.as_ref().is_none_or(|id| {
                            id.is_empty()
                                || id.len() > 1024
                                || id.chars().any(char::is_control)
                                || !playback_ids.insert(id.clone())
                        })
                }
            }
    }) {
        return Err(Failure::Configuration.error());
    }
    for input in &mut prefs.inputs {
        input.phase = if input.kind == SourceKind::Recording && prefs.settings.restore_setup {
            InputPhase::Opening
        } else {
            InputPhase::Stopped
        };
        input.fault = None;
        input.restored = true;
    }
    for destination in &mut prefs.destinations {
        destination.connection = Connection::Disconnected;
        destination.session_id = 0;
    }
    if !prefs.settings.restore_setup {
        prefs.inputs.clear();
        prefs.destinations.clear();
    }
    if prefs
        .selection
        .source_id
        .as_deref()
        .is_some_and(crate::audio::application::is_application_id)
    {
        prefs.selection.source_id = None;
    }
    if prefs.output.as_ref().is_some_and(|o| {
        o.volume_percent > 100
            || o.volume_linear
                .is_some_and(|gain| !gain.is_finite() || !(0.0..=1.0).contains(&gain))
    }) {
        return Err(Failure::Configuration.error());
    }
    Ok(prefs)
}
fn save_preferences(config: &ControllerConfig, snapshot: &ControlSnapshot) -> Result<(), AppError> {
    use std::io::Write;
    if !separate_settings(config) {
        return Err(Failure::Configuration.error());
    }
    let Some(path) = &config.settings_file else {
        return Ok(());
    };
    if let Ok(file) = std::fs::File::open(path) {
        let mut old = Vec::new();
        if file
            .take(MAX_SETTINGS_BYTES + 1)
            .read_to_end(&mut old)
            .is_ok()
            && serde_json::from_slice::<serde_json::Value>(&old)
                .ok()
                .and_then(|v| v.get("version").and_then(|v| v.as_u64()))
                .is_some_and(|version| version > 4)
        {
            return Err(error(
                ErrorCode::Configuration,
                "These preferences were written by a newer Echocord version and were preserved.",
            ));
        }
    }
    let bytes = serde_json::to_vec(&Preferences {
        version: 4,
        inputs: snapshot
            .inputs
            .iter()
            .cloned()
            .map(|mut i| {
                i.phase = InputPhase::Stopped;
                i.fault = None;
                i
            })
            .collect(),
        destinations: snapshot
            .destinations
            .iter()
            .cloned()
            .map(|mut d| {
                d.connection = Connection::Disconnected;
                d.session_id = 0;
                d
            })
            .collect(),
        settings: snapshot.settings.clone(),
        selection: snapshot.selection.clone(),
        output: Some(snapshot.output.clone()),
    })
    .map_err(|_| Failure::Configuration.error())?;
    static NEXT_FILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let suffix = NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), suffix));
    let result = (|| {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result.map_err(|_| error(ErrorCode::Configuration, "Preferences could not be saved."))
}

struct Catalog {
    id: Option<String>,
    avatar_url: Option<String>,
    servers: Vec<Server>,
    name: String,
}

async fn source_scan(
    epoch: u64,
    events: mpsc::Sender<Event>,
    mut scan: tokio::task::JoinHandle<Result<Vec<RecordingSource>, AppError>>,
    deadline: Duration,
) {
    match tokio::time::timeout(deadline, &mut scan).await {
        Ok(result) => {
            let result = result.map_err(|_| unavailable()).and_then(|result| result);
            let _ = events
                .send(Event::SourceScanFinished(epoch, Some(result)))
                .await;
        }
        Err(_) => {
            let _ = events
                .send(Event::Sources(
                    epoch,
                    Err(error(
                        ErrorCode::CaptureFailed,
                        "Audio source discovery timed out. Retry after the current scan finishes.",
                    )),
                ))
                .await;
            // Blocking work cannot be cancelled by dropping its task. Keep actual occupancy,
            // discard late data, and preserve the displayed error until an explicit retry.
            let _ = scan.await;
            let _ = events.send(Event::SourceScanFinished(epoch, None)).await;
        }
    }
}

fn missing_token(config: &ControllerConfig) -> bool {
    config.token_file.as_ref().is_none_or(|path| {
        std::fs::metadata(path).is_err_and(|error| missing_token_error(error.kind()))
    })
}

fn missing_token_error(kind: std::io::ErrorKind) -> bool {
    kind == std::io::ErrorKind::NotFound
}

struct ConfigurationCheck {
    epoch: u64,
    bot: BotProfile,
    discovery: Discovery,
    reply: oneshot::Sender<Result<(), AppError>>,
    task: tokio::task::JoinHandle<()>,
    control: ConfigurationControl,
}

#[derive(Clone, Default)]
struct ConfigurationControl(Arc<Mutex<ConfigurationPhase>>);

#[derive(Default)]
enum ConfigurationPhase {
    #[default]
    Validating,
    #[cfg(any(windows, test))]
    Saving,
    Cancelled,
}

impl ConfigurationControl {
    #[cfg(any(windows, test))]
    fn begin_save(&self) -> bool {
        let mut phase = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*phase, ConfigurationPhase::Validating) {
            *phase = ConfigurationPhase::Saving;
            true
        } else {
            false
        }
    }
    fn cancel_validation(&self) -> bool {
        let mut phase = self.0.lock().unwrap_or_else(|e| e.into_inner());
        #[cfg(any(windows, test))]
        if matches!(*phase, ConfigurationPhase::Saving) {
            return false;
        }
        *phase = ConfigurationPhase::Cancelled;
        true
    }
}

#[cfg(windows)]
struct PlaybackRuntime {
    host: crate::playback::PlaybackHost,
    device_id: String,
    session_id: u32,
    stopping: bool,
    restart: bool,
    removing: bool,
    started: Instant,
    stop_since: Option<Instant>,
}

#[cfg(windows)]
type GatewayHost = crate::discord::worker::Worker<
    crate::discord::protocol::GatewayCommand,
    crate::discord::protocol::GatewayStatus,
>;
#[cfg(windows)]
type VoiceHost = crate::discord::worker::Worker<
    crate::discord::protocol::VoiceCommand,
    crate::discord::protocol::VoiceStatus,
>;

struct Actor {
    #[cfg(test)]
    simulated_discord: bool,
    discord: crate::discord::control::Manager,
    next_discord_publish: Instant,
    discord_restart: std::collections::BTreeSet<String>,
    discord_removing: std::collections::BTreeSet<String>,
    #[cfg(windows)]
    gateway_host: Option<(u32, GatewayHost)>,
    #[cfg(windows)]
    voice_hosts: std::collections::BTreeMap<String, (u32, VoiceHost)>,
    #[cfg(windows)]
    playback: std::collections::BTreeMap<String, PlaybackRuntime>,
    playback_epoch: u64,
    playback_scanning: bool,
    next_destination: u64,
    #[cfg(windows)]
    audio: Option<crate::mixing::AudioHost>,
    audio_failed: bool,
    next_strip: u64,
    config: ControllerConfig,
    snapshot: ControlSnapshot,
    snapshots: watch::Sender<ControlSnapshot>,
    events: mpsc::Sender<Event>,
    catalog_epoch: u64,
    source_epoch: u64,
    source_scanning: bool,
    configuration: Option<ConfigurationCheck>,
    preferences_error: Option<AppError>,
    shutting_down: bool,
    shutdown_waiters: Vec<oneshot::Sender<Result<(), AppError>>>,
}
impl Actor {
    fn new(
        config: ControllerConfig,
        snapshots: watch::Sender<ControlSnapshot>,
        events: mpsc::Sender<Event>,
    ) -> Self {
        Self {
            #[cfg(test)]
            simulated_discord: false,
            discord: crate::discord::control::Manager::default(),
            next_discord_publish: Instant::now(),
            discord_restart: std::collections::BTreeSet::new(),
            discord_removing: std::collections::BTreeSet::new(),
            #[cfg(windows)]
            gateway_host: None,
            #[cfg(windows)]
            voice_hosts: std::collections::BTreeMap::new(),
            #[cfg(windows)]
            playback: std::collections::BTreeMap::new(),
            playback_epoch: 0,
            playback_scanning: false,
            next_destination: 1,
            #[cfg(windows)]
            audio: None,
            audio_failed: false,
            next_strip: 1,
            config,
            snapshot: ControlSnapshot::default(),
            snapshots,
            events,
            catalog_epoch: 0,
            source_epoch: 0,
            source_scanning: false,
            configuration: None,
            preferences_error: None,
            shutting_down: false,
            shutdown_waiters: Vec::new(),
        }
    }
    fn publish(&mut self) {
        self.snapshot.selection.source_id = self
            .snapshot
            .inputs
            .first()
            .map(|input| input.source_id.clone());
        self.discord.publish(&mut self.snapshot);
        self.snapshot.telemetry.destinations.retain(|meter| {
            self.snapshot
                .destinations
                .iter()
                .any(|d| d.destination_id == meter.destination_id)
        });
        if let Some(d) = self
            .snapshot
            .destinations
            .iter()
            .find(|d| d.kind == DestinationKind::Discord)
        {
            self.snapshot.connection = d.connection.clone();
            self.snapshot.session_id = d.session_id;
            self.snapshot.selection.server_id = Some(d.server_id.clone());
            self.snapshot.selection.channel_id = Some(d.channel_id.clone());
            self.snapshot.capture = self
                .discord
                .rooms
                .get(&d.destination_id)
                .map_or_else(|| ControlSnapshot::default().capture, |r| r.capture.clone());
        } else {
            self.snapshot.selection.server_id = None;
            self.snapshot.selection.channel_id = None;
            self.snapshot.connection = Connection::Disconnected;
            self.snapshot.capture = ControlSnapshot::default().capture;
        }
        self.snapshot.telemetry.gateway_ping_ms = self.discord.ping;
        self.snapshot.telemetry.output_level_dbfs = self.snapshot.capture.output_level_dbfs;
        self.snapshot.telemetry.queued_frames = self.snapshot.capture.queued_frames;
        self.snapshot.telemetry.dropped_frames = self.snapshot.capture.dropped_frames;
        self.snapshot.telemetry.underrun_frames = self.snapshot.capture.underrun_frames;
        self.snapshot.revision = self.snapshot.revision.wrapping_add(1);
        self.snapshots.send_replace(self.snapshot.clone());
    }
    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Request>,
        events: mpsc::Receiver<Event>,
        active: bool,
    ) {
        if !active {
            loop {
                match commands.recv().await {
                    Some(Request::Activate) => break,
                    Some(Request::Shutdown(reply)) => {
                        let _ = reply.send(Ok(()));
                        return;
                    }
                    Some(Request::Dispatch(_, reply)) | Some(Request::ConfigureToken(_, reply)) => {
                        let _ = reply.send(Err(busy()));
                    }
                    None => return,
                }
            }
        }
        match load_preferences(&self.config) {
            Ok(prefs) => {
                self.snapshot.inputs = prefs.inputs;
                self.snapshot.destinations = prefs.destinations;
                self.snapshot.settings = prefs.settings;
                self.next_destination = self
                    .snapshot
                    .destinations
                    .iter()
                    .filter_map(|d| {
                        d.destination_id
                            .split_once('-')
                            .map(|(_, id)| id)
                            .and_then(|id| id.parse::<u64>().ok())
                    })
                    .max()
                    .unwrap_or(0)
                    + 1;
                self.next_strip = self
                    .snapshot
                    .inputs
                    .iter()
                    .filter_map(|i| {
                        i.strip_id
                            .strip_prefix("input-")
                            .and_then(|n| n.parse::<u64>().ok())
                    })
                    .max()
                    .unwrap_or(0)
                    + 1;
                self.snapshot.selection = prefs.selection;
                if let Some(destination) = self
                    .snapshot
                    .destinations
                    .iter()
                    .find(|d| d.kind == DestinationKind::Discord)
                {
                    self.snapshot.selection.server_id = Some(destination.server_id.clone());
                    self.snapshot.selection.channel_id = Some(destination.channel_id.clone());
                }
                if let Some(output) = prefs.output {
                    self.snapshot.output = output;
                }
                if let Some(destination) = self
                    .snapshot
                    .destinations
                    .iter()
                    .find(|d| d.kind == DestinationKind::Discord)
                {
                    self.snapshot.output.volume_linear =
                        Some(crate::mixing::linear(destination.volume_db));
                    self.snapshot.output.muted = destination.muted;
                }
            }
            Err(_) => {
                self.preferences_error = Some(error(
                    ErrorCode::Configuration,
                    "Saved preferences could not be loaded. Select a route again.",
                ))
            }
        }
        if let Err(error) = self.sync_audio() {
            self.audio_failed = true;
            for input in &mut self.snapshot.inputs {
                input.phase = InputPhase::Fault;
                input.fault = Some(error.clone());
            }
        }
        self.refresh();
        self.run_requests(commands, events).await;
    }
    async fn run_requests(
        mut self,
        mut commands: mpsc::Receiver<Request>,
        mut events: mpsc::Receiver<Event>,
    ) {
        let mut tick = tokio::time::interval(Duration::from_millis(25));
        loop {
            tokio::select! {
                request = commands.recv(), if !self.shutting_down => match request {
                    Some(Request::Activate) => {},
                    Some(Request::Dispatch(command, reply)) => { let result = self.command(command); let _ = reply.send(result); }
                    Some(Request::ConfigureToken(token, reply)) => self.configure_token(token, reply),
                    Some(Request::Shutdown(reply)) => { self.shutdown_waiters.push(reply); self.begin_shutdown(); }
                    None => self.begin_shutdown(),
                },
                Some(event) = events.recv() => self.event(event),
                _ = tick.tick() => {
                    self.poll_audio();
                    self.poll_playback();
                    self.poll_discord();
                }
            }
            if self.shutting_down
                && self.audio_stopped()
                && self.playback_stopped()
                && !self.discord.busy()
                && self.configuration.is_none()
            {
                let result =
                    if self.audio_cleanup_incomplete() || self.playback_cleanup_incomplete() {
                        Err(cleanup_error())
                    } else {
                        Ok(())
                    };
                for waiter in self.shutdown_waiters.drain(..) {
                    let _ = waiter.send(result.clone());
                }
                // The supervisor still owns a kill-on-close job on incomplete cleanup.
                break;
            }
        }
    }
    fn begin_shutdown(&mut self) {
        self.shutting_down = true;
        self.discord_restart.clear();
        self.discord.stop_all();
        self.poll_discord();
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            audio.stop();
        }
        #[cfg(windows)]
        for output in self.playback.values_mut() {
            output.stopping = true;
            output.stop_since.get_or_insert_with(Instant::now);
            output.restart = false;
            output.host.stop();
        }
        if self
            .configuration
            .as_ref()
            .is_some_and(|check| check.control.cancel_validation())
        {
            let configuration = self.configuration.take().expect("cancelled validation");
            configuration.task.abort();
            let _ = configuration.reply.send(Err(unavailable()));
        }
        self.publish();
        // Saving is committed under the same mutex as cancellation. A queued or
        // running blocking save must report its actual result before shutdown
        // can succeed; the shell separately owns its three-second Quit fallback.
    }
    fn refresh(&mut self) {
        self.refresh_sources();
        self.refresh_playback_devices();
        self.catalog_epoch = self.catalog_epoch.wrapping_add(1);
        let epoch = self.catalog_epoch;
        self.snapshot.discovery = Discovery::Loading;
        self.snapshot.bot.status = if self.config.token_file.is_some() {
            BotStatus::Checking
        } else {
            BotStatus::Unconfigured
        };
        self.publish();
        let config = self.config.clone();
        let events = self.events.clone();
        tokio::spawn(async move {
            let result = tokio::time::timeout(Duration::from_secs(30), async {
                let check_config = config.clone();
                if tokio::task::spawn_blocking(move || missing_token(&check_config))
                    .await
                    .map_err(|_| unavailable())?
                {
                    return Ok(None);
                }
                discover(&config).await.map(Some)
            })
            .await
            .unwrap_or_else(|_| Err(Failure::Connection.error()));
            let event = match result {
                Ok(None) => Event::Unconfigured(epoch),
                Ok(Some(catalog)) => Event::Catalog(epoch, Ok(catalog)),
                Err(error) => Event::Catalog(epoch, Err(error)),
            };
            let _ = events.send(event).await;
        });
    }
    fn refresh_sources(&mut self) {
        if self.source_scanning {
            return;
        }
        self.source_scanning = true;
        self.source_epoch = self.source_epoch.wrapping_add(1);
        let epoch = self.source_epoch;
        self.snapshot.source_discovery = Discovery::Loading;
        self.publish();
        let events = self.events.clone();
        tokio::spawn(source_scan(
            epoch,
            events,
            tokio::task::spawn_blocking(crate::audio::enumerate_sources),
            Duration::from_secs(5),
        ));
    }
    fn command(&mut self, command: ControlCommand) -> Result<(), AppError> {
        match command {
            ControlCommand::RefreshPlaybackDevices => {
                self.refresh_playback_devices();
                return Ok(());
            }
            ControlCommand::RefreshSources => {
                self.refresh_sources();
                return Ok(());
            }
            ControlCommand::RefreshCatalog | ControlCommand::RecheckConfiguration => {
                if self.configuration.is_some() {
                    return Err(busy());
                }
                self.refresh();
                return Ok(());
            }
            ControlCommand::SelectSource { source_id } => {
                let command = self.snapshot.inputs.first().map_or_else(
                    || ControlCommand::AddSource {
                        source_id: source_id.clone(),
                    },
                    |input| ControlCommand::ReplaceSource {
                        strip_id: input.strip_id.clone(),
                        source_id: source_id.clone(),
                    },
                );
                return self.mix_command(command);
            }
            ControlCommand::SelectDestination {
                server_id,
                channel_id,
            } => {
                let channel_id = channel_id.ok_or_else(|| {
                    error(
                        ErrorCode::InvalidSelection,
                        "Select a voice channel for this destination.",
                    )
                })?;
                let command = self
                    .snapshot
                    .destinations
                    .iter()
                    .find(|d| d.kind == DestinationKind::Discord)
                    .map_or_else(
                        || ControlCommand::AddDestination {
                            server_id: server_id.clone(),
                            channel_id: channel_id.clone(),
                        },
                        |d| ControlCommand::ChangeDestination {
                            destination_id: d.destination_id.clone(),
                            server_id: server_id.clone(),
                            channel_id: channel_id.clone(),
                        },
                    );
                return self.mix_command(command);
            }
            ControlCommand::SetVolume { percent } => {
                if percent > 100 {
                    return Err(error(
                        ErrorCode::InvalidVolume,
                        "Volume must be between 0 and 100.",
                    ));
                }
                self.snapshot.output.volume_percent = percent;
                self.snapshot.output.volume_linear = None;
                if let Some(d) = self
                    .snapshot
                    .destinations
                    .iter_mut()
                    .find(|d| d.kind == DestinationKind::Discord)
                {
                    d.volume_db = if percent == 0 {
                        None
                    } else {
                        Some(20.0 * (percent as f32 / 100.0).log10())
                    };
                }
                if let Some(index) = self
                    .snapshot
                    .destinations
                    .iter()
                    .position(|d| d.kind == DestinationKind::Discord)
                {
                    self.update_destination_output(index);
                }
            }
            ControlCommand::SetMuted { muted } => {
                self.snapshot.output.muted = muted;
                if let Some(d) = self
                    .snapshot
                    .destinations
                    .iter_mut()
                    .find(|d| d.kind == DestinationKind::Discord)
                {
                    d.muted = muted;
                }
                if let Some(index) = self
                    .snapshot
                    .destinations
                    .iter()
                    .position(|d| d.kind == DestinationKind::Discord)
                {
                    self.update_destination_output(index);
                }
            }
            ControlCommand::Connect => {
                let id = self
                    .snapshot
                    .destinations
                    .iter()
                    .find(|d| d.kind == DestinationKind::Discord)
                    .map(|d| d.destination_id.clone())
                    .ok_or_else(|| error(ErrorCode::InvalidSelection, "Select a Discord room."))?;
                return self.start_discord(&id);
            }
            ControlCommand::Disconnect => {
                self.discord_restart.clear();
                self.discord.stop_all();
                self.poll_discord();
                return Ok(());
            }
            command => return self.mix_command(command),
        }
        self.publish();
        self.persist()?;
        if self.preferences_error.take().is_some() && self.snapshot.bot.status == BotStatus::Ready {
            self.snapshot.discovery = Discovery::Ready;
            self.publish();
        }
        Ok(())
    }
    fn sync_audio(&mut self) -> Result<(), AppError> {
        if self.audio_failed {
            return Err(error(
                ErrorCode::CaptureFailed,
                "The audio worker stopped. Restart Echocord.",
            ));
        }
        #[cfg(windows)]
        {
            if self.audio.is_none() {
                self.audio = Some(crate::mixing::AudioHost::launch(
                    self.config.executable.clone(),
                )?);
            }
            if let Some(audio) = &self.audio {
                audio.sources(self.snapshot.inputs.clone());
            }
        }
        Ok(())
    }
    fn audio_cleanup_incomplete(&self) -> bool {
        #[cfg(windows)]
        {
            self.audio
                .as_ref()
                .is_some_and(|a| a.exit.load(std::sync::atomic::Ordering::Acquire) == 2)
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    fn audio_stopped(&self) -> bool {
        #[cfg(windows)]
        {
            self.audio
                .as_ref()
                .is_none_or(|a| a.exit.load(std::sync::atomic::Ordering::Acquire) != 0)
        }
        #[cfg(not(windows))]
        {
            true
        }
    }
    fn poll_audio(&mut self) {
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            let exit = audio.exit.load(std::sync::atomic::Ordering::Acquire);
            if exit != 0 && !self.audio_failed {
                self.audio_failed = true;
                for input in &mut self.snapshot.inputs {
                    input.phase = InputPhase::Fault;
                    input.fault = Some(error(
                        if exit == 2 {
                            ErrorCode::CleanupIncomplete
                        } else {
                            ErrorCode::CaptureFailed
                        },
                        "The audio worker stopped. Restart Echocord.",
                    ));
                }
                self.snapshot.telemetry.inputs.clear();
                // Network worker remains owned; the dead producer yields paced silence.
                self.publish();
            } else if let Some(observation) = audio.snapshot() {
                for observed in observation.inputs {
                    if let Some(input) = self.snapshot.inputs.iter_mut().find(|i| {
                        i.strip_id == observed.strip_id && i.source_id == observed.source_id
                    }) && matches!(input.phase, InputPhase::Opening | InputPhase::Capturing)
                    {
                        input.phase = observed.phase;
                        input.fault = observed.fault;
                        input.restored = observed.restored;
                    }
                }
                self.snapshot.telemetry.mix_pre_clip = observation.mix_pre_clip;
                self.snapshot.telemetry.inputs = observation.telemetry;
                self.publish();
            }
        }
    }
    fn source_for_add(
        &self,
        source_id: &str,
        replacing: Option<&str>,
    ) -> Result<RecordingSource, AppError> {
        let source = self
            .snapshot
            .sources
            .iter()
            .find(|s| s.id == source_id && s.available)
            .cloned()
            .ok_or_else(|| {
                error(
                    ErrorCode::InvalidSelection,
                    "Select an available audio source.",
                )
            })?;
        if source.name.chars().any(char::is_control)
            || source.id.chars().any(char::is_control)
            || source
                .detail
                .as_ref()
                .is_some_and(|detail| detail.chars().any(char::is_control))
            || source.id.len() > 1024
            || source.id.starts_with("mix-handle:")
            || source.name.len() > 1024
            || source
                .detail
                .as_ref()
                .is_some_and(|detail| detail.len() > 1024)
        {
            return Err(error(
                ErrorCode::InvalidSelection,
                "This source descriptor exceeds the supported size.",
            ));
        }
        for input in self
            .snapshot
            .inputs
            .iter()
            .filter(|i| Some(i.strip_id.as_str()) != replacing)
        {
            if input.source_id == source_id {
                return Err(error(
                    ErrorCode::InvalidSelection,
                    "This source is already in the mix.",
                ));
            }
            #[cfg(windows)]
            if source.kind == SourceKind::Application
                && input.kind == SourceKind::Application
                && crate::audio::application::roots_overlap(&input.source_id, source_id)?
            {
                return Err(error(
                    ErrorCode::InvalidSelection,
                    "This application overlaps a source already in the mix.",
                ));
            }
        }
        Ok(source)
    }
    fn validate_destination(&self, server: &str, channel: &str) -> Result<(), AppError> {
        if self
            .snapshot
            .servers
            .iter()
            .any(|s| s.id == server && s.channels.iter().any(|c| c.id == channel))
        {
            Ok(())
        } else {
            Err(error(
                ErrorCode::InvalidSelection,
                "Select an available voice room.",
            ))
        }
    }
    fn mix_command(&mut self, command: ControlCommand) -> Result<(), AppError> {
        if matches!(
            command,
            ControlCommand::AddPlaybackDestination { .. }
                | ControlCommand::ChangePlaybackDestination { .. }
                | ControlCommand::SetAllDestinationsMuted { .. }
                | ControlCommand::DisconnectAllDestinations
        ) {
            return self.playback_command(command);
        }
        if self.audio_failed
            && matches!(
                command,
                ControlCommand::AddSource { .. }
                    | ControlCommand::ReplaceSource { .. }
                    | ControlCommand::StartCapture { .. }
                    | ControlCommand::SetSourceGain { .. }
                    | ControlCommand::SetSourceMuted { .. }
            )
        {
            return Err(error(
                ErrorCode::CaptureFailed,
                "The audio worker stopped. Restart Echocord.",
            ));
        }
        match command {
            ControlCommand::AddSource { source_id } => {
                if self.snapshot.inputs.len() >= MAX_INPUTS {
                    return Err(error(
                        ErrorCode::InvalidSelection,
                        "The mix supports up to eight sources.",
                    ));
                }
                let source = self.source_for_add(&source_id, None)?;
                let strip_id = format!("input-{}", self.next_strip);
                self.next_strip += 1;
                let colour_slot = (self.next_strip.saturating_sub(2)).min(5) as u8;
                self.snapshot.inputs.push(InputStrip {
                    strip_id,
                    source_id,
                    kind: source.kind,
                    display_name: source.name,
                    detail: source.detail,
                    phase: InputPhase::Opening,
                    fault: None,
                    gain_db: Some(0.0),
                    muted: false,
                    colour_slot,
                    restored: false,
                });
                self.sync_audio()?;
            }
            ControlCommand::RemoveSource { strip_id } => {
                if !self.snapshot.inputs.iter().any(|i| i.strip_id == strip_id) {
                    return Err(error(
                        ErrorCode::InvalidSelection,
                        "This source no longer exists.",
                    ));
                }
                self.snapshot.inputs.retain(|i| i.strip_id != strip_id);
                self.snapshot
                    .telemetry
                    .inputs
                    .retain(|i| i.strip_id != strip_id);
                if !self.audio_failed {
                    self.sync_audio()?;
                }
            }
            ControlCommand::ReplaceSource {
                strip_id,
                source_id,
            } => {
                let source = self.source_for_add(&source_id, Some(&strip_id))?;
                let input = self
                    .snapshot
                    .inputs
                    .iter_mut()
                    .find(|i| i.strip_id == strip_id)
                    .ok_or_else(|| {
                        error(ErrorCode::InvalidSelection, "This source no longer exists.")
                    })?;
                if input.source_id == source_id
                    && matches!(input.phase, InputPhase::Opening | InputPhase::Capturing)
                {
                    return Ok(());
                }
                input.source_id = source_id;
                input.kind = source.kind;
                input.display_name = source.name;
                input.detail = source.detail;
                input.phase = InputPhase::Opening;
                input.fault = None;
                input.restored = false;
                self.sync_audio()?;
            }
            ControlCommand::StartCapture { strip_id } => {
                let input = self
                    .snapshot
                    .inputs
                    .iter_mut()
                    .find(|i| i.strip_id == strip_id)
                    .ok_or_else(|| {
                        error(ErrorCode::InvalidSelection, "This source no longer exists.")
                    })?;
                if matches!(input.phase, InputPhase::Opening | InputPhase::Capturing) {
                    return Ok(());
                }
                #[cfg(windows)]
                if input.kind == SourceKind::Application {
                    crate::audio::validate_source(&input.source_id).map_err(|_| {
                        error(
                            ErrorCode::InvalidSelection,
                            "This app instance is no longer running. Select the app again.",
                        )
                    })?;
                }
                input.phase = InputPhase::Opening;
                input.fault = None;
                self.sync_audio()?;
            }
            ControlCommand::StopCapture { strip_id } => {
                let input = self
                    .snapshot
                    .inputs
                    .iter_mut()
                    .find(|i| i.strip_id == strip_id)
                    .ok_or_else(|| {
                        error(ErrorCode::InvalidSelection, "This source no longer exists.")
                    })?;
                input.phase = InputPhase::Stopped;
                input.fault = None;
                if !self.audio_failed {
                    self.sync_audio()?;
                }
            }
            ControlCommand::SetSourceMuted { strip_id, muted } => {
                let input = self
                    .snapshot
                    .inputs
                    .iter_mut()
                    .find(|i| i.strip_id == strip_id)
                    .ok_or_else(|| {
                        error(ErrorCode::InvalidSelection, "This source no longer exists.")
                    })?;
                input.muted = muted;
                self.sync_audio()?;
            }
            ControlCommand::SetSourceGain { strip_id, gain_db } => {
                if !crate::mixing::valid_gain(gain_db) {
                    return Err(error(
                        ErrorCode::InvalidVolume,
                        "Gain must be Off or between −60 and 0 dB.",
                    ));
                }
                let input = self
                    .snapshot
                    .inputs
                    .iter_mut()
                    .find(|i| i.strip_id == strip_id)
                    .ok_or_else(|| {
                        error(ErrorCode::InvalidSelection, "This source no longer exists.")
                    })?;
                input.gain_db = gain_db;
                self.sync_audio()?;
            }
            ControlCommand::AddDestination {
                server_id,
                channel_id,
            } => {
                if self.snapshot.destinations.len() >= MAX_DESTINATIONS {
                    return Err(error(
                        ErrorCode::InvalidSelection,
                        "Up to four destinations are supported.",
                    ));
                }
                self.validate_destination(&server_id, &channel_id)?;
                self.validate_server_slot(&server_id, None)?;
                let destination_id = format!("discord-{}", self.next_destination);
                self.next_destination += 1;
                self.snapshot.destinations.push(DestinationStrip {
                    session_id: 0,
                    kind: DestinationKind::Discord,
                    device_id: None,
                    display_name: None,
                    destination_id,
                    server_id,
                    channel_id,
                    connection: Connection::Disconnected,
                    volume_db: Some(0.0),
                    muted: false,
                });
            }
            ControlCommand::RemoveDestination { destination_id } => {
                let index = self.destination_index(&destination_id)?;
                if self.snapshot.destinations[index].kind == DestinationKind::Playback {
                    if self.defer_playback_removal(&destination_id) {
                        self.publish();
                        return self.persist();
                    }
                    self.stop_playback(&destination_id, false);
                } else {
                    self.discord_restart.remove(&destination_id);
                    if self.discord.contains(&destination_id) {
                        self.discord_removing.insert(destination_id.clone());
                        self.discord.stop(&destination_id, None);
                        self.poll_discord();
                        self.publish();
                        return self.persist();
                    }
                }
                self.snapshot.destinations.remove(index);
            }
            ControlCommand::ChangeDestination {
                destination_id,
                server_id,
                channel_id,
            } => {
                let index = self.destination_index(&destination_id)?;
                let destination = &self.snapshot.destinations[index];
                if destination.kind != DestinationKind::Discord {
                    return Err(error(
                        ErrorCode::InvalidSelection,
                        "Select a playback device for this output.",
                    ));
                }
                self.validate_destination(&server_id, &channel_id)?;
                if destination.server_id == server_id && destination.channel_id == channel_id {
                    return Ok(());
                }
                self.validate_server_slot(&server_id, Some(&destination_id))?;
                if self
                    .discord
                    .rooms
                    .get(&destination_id)
                    .is_some_and(|r| !r.wanted)
                {
                    return Err(busy());
                }
                if self.discord.contains(&destination_id) {
                    self.discord_restart.insert(destination_id.clone());
                    self.discord.stop(&destination_id, None);
                }
                let d = &mut self.snapshot.destinations[index];
                if !self.discord.contains(&destination_id) {
                    d.connection = Connection::Disconnected;
                }
                d.server_id = server_id;
                d.channel_id = channel_id;
                self.poll_discord();
            }
            ControlCommand::ConnectDestination { destination_id } => {
                let index = self.destination_index(&destination_id)?;
                return if self.snapshot.destinations[index].kind == DestinationKind::Playback {
                    self.start_playback(&destination_id)
                } else {
                    self.start_discord(&destination_id)
                };
            }
            ControlCommand::DisconnectDestination { destination_id } => {
                let index = self.destination_index(&destination_id)?;
                if self.snapshot.destinations[index].kind == DestinationKind::Playback {
                    self.stop_playback(&destination_id, false);
                } else {
                    self.discord_restart.remove(&destination_id);
                    self.discord.stop(&destination_id, None);
                    self.poll_discord();
                }
            }
            ControlCommand::SetDestinationVolume {
                destination_id,
                volume_db,
            } => {
                let index = self.destination_index(&destination_id)?;
                if !crate::mixing::valid_gain(volume_db) {
                    return Err(error(
                        ErrorCode::InvalidVolume,
                        "Volume must be Off or between −60 and 0 dB.",
                    ));
                }
                self.snapshot.destinations[index].volume_db = volume_db;
                self.update_destination_output(index);
            }
            ControlCommand::SetDestinationMuted {
                destination_id,
                muted,
            } => {
                let index = self.destination_index(&destination_id)?;
                self.snapshot.destinations[index].muted = muted;
                self.update_destination_output(index);
            }
            ControlCommand::SetSettings { settings } => {
                self.snapshot.settings = settings;
            }
            _ => return Err(error(ErrorCode::Internal, "Unsupported command.")),
        }
        self.publish();
        self.persist()
    }
    fn persist(&mut self) -> Result<(), AppError> {
        let mut snapshot = self.snapshot.clone();
        snapshot
            .destinations
            .retain(|d| !self.discord_removing.contains(&d.destination_id));
        #[cfg(windows)]
        snapshot.destinations.retain(|d| {
            self.playback
                .get(&d.destination_id)
                .is_none_or(|r| !r.removing)
        });
        save_preferences(&self.config, &snapshot)?;
        if self.preferences_error.take().is_some() && self.snapshot.bot.status == BotStatus::Ready {
            self.snapshot.discovery = Discovery::Ready;
            self.publish();
        }
        Ok(())
    }
    fn destination_index(&self, id: &str) -> Result<usize, AppError> {
        self.snapshot
            .destinations
            .iter()
            .position(|d| d.destination_id == id)
            .ok_or_else(|| {
                error(
                    ErrorCode::InvalidSelection,
                    "This destination no longer exists.",
                )
            })
    }
    fn refresh_playback_devices(&mut self) {
        if self.playback_scanning {
            return;
        }
        self.playback_scanning = true;
        self.playback_epoch = self.playback_epoch.wrapping_add(1);
        let epoch = self.playback_epoch;
        let events = self.events.clone();
        self.snapshot.playback_discovery = Discovery::Loading;
        self.publish();
        tokio::spawn(async move {
            let mut scan = tokio::task::spawn_blocking(crate::playback::enumerate_devices);
            match tokio::time::timeout(Duration::from_secs(5), &mut scan).await {
                Ok(result) => {
                    let result = result.map_err(|_| unavailable()).and_then(|value| value);
                    let _ = events
                        .send(Event::PlaybackScanFinished(epoch, Some(result)))
                        .await;
                }
                Err(_) => {
                    let _ = events
                        .send(Event::PlaybackDevices(
                            epoch,
                            Err(error(
                                ErrorCode::ConnectionFailed,
                                "Playback discovery timed out. Retry after this scan finishes.",
                            )),
                        ))
                        .await;
                    let _ = scan.await;
                    let _ = events.send(Event::PlaybackScanFinished(epoch, None)).await;
                }
            }
        });
    }
    fn playback_device(
        &self,
        device_id: &str,
        replacing: Option<&str>,
    ) -> Result<PlaybackDevice, AppError> {
        let device = self
            .snapshot
            .playback_devices
            .iter()
            .find(|d| d.id == device_id && d.available)
            .cloned()
            .ok_or_else(|| {
                error(
                    ErrorCode::InvalidSelection,
                    "Select an available 48 kHz stereo playback output.",
                )
            })?;
        if device.id.len() > 1024
            || device.id.chars().any(char::is_control)
            || device.name.len() > 1024
            || device.name.chars().any(char::is_control)
        {
            return Err(error(
                ErrorCode::InvalidSelection,
                "This playback output identity exceeds the supported size.",
            ));
        }
        if self.snapshot.destinations.iter().any(|d| {
            Some(d.destination_id.as_str()) != replacing
                && d.device_id.as_deref() == Some(device_id)
        }) {
            return Err(error(
                ErrorCode::InvalidSelection,
                "This playback output is already selected.",
            ));
        }
        #[cfg(windows)]
        if self
            .playback
            .iter()
            .any(|(id, runtime)| Some(id.as_str()) != replacing && runtime.device_id == device_id)
        {
            return Err(error(
                ErrorCode::Busy,
                "Wait for the previous playback output to finish stopping.",
            ));
        }
        Ok(device)
    }
    fn update_destination_output(&mut self, index: usize) {
        let destination = &self.snapshot.destinations[index];
        let output = Output {
            volume_linear: Some(crate::mixing::linear(destination.volume_db)),
            volume_percent: (crate::mixing::linear(destination.volume_db) * 100.0).round() as u8,
            muted: destination.muted,
        };
        if destination.kind == DestinationKind::Discord {
            #[cfg(windows)]
            if let Some((_, host)) = self.voice_hosts.get(&destination.destination_id) {
                let _ = host.send(crate::discord::protocol::VoiceCommand::Output {
                    output: output.clone(),
                });
            }
            self.snapshot.output = output;
        } else {
            #[cfg(windows)]
            if let Some(runtime) = self.playback.get(&destination.destination_id) {
                runtime.host.output(output);
            }
        }
    }
    fn clear_destination_meter(&mut self, index: usize) {
        let destination = &self.snapshot.destinations[index];
        let id = destination.destination_id.clone();
        self.snapshot
            .telemetry
            .destinations
            .retain(|m| m.destination_id != id);
        self.snapshot
            .telemetry
            .destinations
            .push(DestinationTelemetry {
                destination_id: id,
                session_id: destination.session_id,
                level_dbfs: None,
                queued_frames: 0,
                dropped_frames: 0,
                underrun_frames: 0,
                gateway_ping_ms: None,
            });
    }
    fn start_playback(&mut self, destination_id: &str) -> Result<(), AppError> {
        let index = self.destination_index(destination_id)?;
        let destination = &self.snapshot.destinations[index];
        if destination.kind != DestinationKind::Playback {
            return Err(error(
                ErrorCode::InvalidSelection,
                "This destination is not a playback output.",
            ));
        }
        if destination.connection == Connection::Connected {
            return Ok(());
        }
        if destination.connection.locks_route() {
            return Err(busy());
        }
        let device_id = destination.device_id.clone().ok_or_else(unavailable)?;
        self.playback_device(&device_id, Some(destination_id))?;
        #[cfg(not(windows))]
        {
            Err(error(
                ErrorCode::ConnectionFailed,
                "Playback outputs require Windows.",
            ))
        }
        #[cfg(windows)]
        {
            if self.playback.contains_key(destination_id) {
                return Err(busy());
            }
            self.sync_audio()?;
            let destination = &self.snapshot.destinations[index];
            let session_id = destination
                .session_id
                .checked_add(1)
                .ok_or_else(unavailable)?;
            let output = Output {
                volume_linear: Some(crate::mixing::linear(destination.volume_db)),
                volume_percent: 100,
                muted: destination.muted,
            };
            let bus = self.audio.as_ref().ok_or_else(unavailable)?.bus.clone();
            let host = crate::playback::PlaybackHost::launch(
                self.config.executable.clone(),
                bus,
                device_id.clone(),
                output,
                session_id,
            )?;
            self.playback.insert(
                destination_id.into(),
                PlaybackRuntime {
                    host,
                    device_id,
                    session_id,
                    stopping: false,
                    restart: false,
                    removing: false,
                    started: Instant::now(),
                    stop_since: None,
                },
            );
            self.snapshot.destinations[index].session_id = session_id;
            self.snapshot.destinations[index].connection = Connection::Connecting {
                stage: ConnectStage::Capture,
            };
            self.clear_destination_meter(index);
            self.publish();
            Ok(())
        }
    }
    fn defer_playback_removal(&mut self, destination_id: &str) -> bool {
        #[cfg(windows)]
        if let Some(runtime) = self.playback.get_mut(destination_id) {
            runtime.removing = true;
            self.stop_playback(destination_id, false);
            return true;
        }
        #[cfg(not(windows))]
        let _ = destination_id;
        false
    }
    fn stop_playback(&mut self, destination_id: &str, restart: bool) {
        #[cfg(windows)]
        let stopping = if let Some(runtime) = self.playback.get_mut(destination_id) {
            runtime.stopping = true;
            runtime.stop_since.get_or_insert_with(Instant::now);
            runtime.restart = restart;
            runtime.host.stop();
            true
        } else {
            false
        };
        #[cfg(not(windows))]
        let stopping = {
            let _ = restart;
            false
        };
        if let Ok(index) = self.destination_index(destination_id) {
            if !matches!(
                self.snapshot.destinations[index].connection,
                Connection::CleanupIncomplete { .. }
            ) {
                self.snapshot.destinations[index].connection = if stopping {
                    Connection::Disconnecting
                } else {
                    Connection::Disconnected
                };
            }
            self.clear_destination_meter(index);
        }
    }
    fn playback_stopped(&self) -> bool {
        #[cfg(windows)]
        {
            self.playback
                .values()
                .all(|p| p.host.exit.load(std::sync::atomic::Ordering::Acquire) != 0)
        }
        #[cfg(not(windows))]
        {
            true
        }
    }
    fn playback_cleanup_incomplete(&self) -> bool {
        #[cfg(windows)]
        {
            self.playback
                .values()
                .any(|p| p.host.exit.load(std::sync::atomic::Ordering::Acquire) == 2)
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    fn poll_playback(&mut self) {
        #[cfg(windows)]
        {
            let mut finished = Vec::new();
            let mut changed = false;
            for (id, runtime) in &mut self.playback {
                let index = self
                    .snapshot
                    .destinations
                    .iter()
                    .position(|d| &d.destination_id == id);
                if !runtime.stopping
                    && runtime.started.elapsed() > Duration::from_secs(10)
                    && index.is_some_and(|index| {
                        matches!(
                            self.snapshot.destinations[index].connection,
                            Connection::Connecting { .. }
                        )
                    })
                {
                    runtime.stopping = true;
                    runtime.stop_since = Some(Instant::now());
                    runtime.host.stop();
                    if let Some(index) = index {
                        self.snapshot.destinations[index].connection = Connection::Error {
                            code: ErrorCode::ConnectionFailed,
                            message:
                                "Playback startup timed out. Press Play to retry after cleanup."
                                    .into(),
                        };
                        changed = true;
                    }
                }
                if runtime
                    .stop_since
                    .is_some_and(|since| since.elapsed() > Duration::from_secs(4))
                    && runtime.host.exit.load(std::sync::atomic::Ordering::Acquire) == 0
                {
                    // Covers even delayed OS process creation before supervision starts.
                    let _ = runtime.host.exit.compare_exchange(
                        0,
                        2,
                        std::sync::atomic::Ordering::AcqRel,
                        std::sync::atomic::Ordering::Acquire,
                    );
                }
                if let Some(observed) = runtime.host.snapshot()
                    && !runtime.stopping
                    && let Some(index) = index
                    && self.snapshot.destinations[index].session_id == runtime.session_id
                {
                    let failed = matches!(observed.connection, Connection::Error { .. });
                    self.snapshot.destinations[index].connection = observed.connection;
                    let value = observed.observation;
                    self.snapshot
                        .telemetry
                        .destinations
                        .retain(|m| &m.destination_id != id);
                    self.snapshot
                        .telemetry
                        .destinations
                        .push(DestinationTelemetry {
                            destination_id: id.clone(),
                            session_id: runtime.session_id,
                            level_dbfs: value
                                .level_dbfs
                                .filter(|v| v.is_finite())
                                .map(|v| v.clamp(-96.0, 0.0)),
                            queued_frames: value.queued_frames,
                            dropped_frames: value.dropped_frames,
                            underrun_frames: value.underrun_frames,
                            gateway_ping_ms: None,
                        });
                    if failed {
                        runtime.stopping = true;
                        runtime.stop_since.get_or_insert_with(Instant::now);
                        runtime.restart = false;
                        runtime.host.stop();
                    }
                    changed = true;
                }
                let exit = runtime.host.exit.load(std::sync::atomic::Ordering::Acquire);
                if exit == 2 {
                    if let Some(index) = index
                        && !matches!(
                            self.snapshot.destinations[index].connection,
                            Connection::CleanupIncomplete { .. }
                        )
                    {
                        self.snapshot.destinations[index].connection = Connection::CleanupIncomplete { message: "Playback output cleanup could not be confirmed. Restart Echocord.".into() };
                        changed = true;
                    }
                } else if exit == 1 {
                    let restart = runtime.restart
                        && !runtime.removing
                        && !self.shutting_down
                        && index.is_some();
                    if let Some(index) = index
                        && runtime.removing
                    {
                        self.snapshot.destinations.remove(index);
                        self.snapshot
                            .telemetry
                            .destinations
                            .retain(|m| &m.destination_id != id);
                    } else if let Some(index) = index {
                        if restart
                            || !matches!(
                                self.snapshot.destinations[index].connection,
                                Connection::Error { .. }
                            )
                        {
                            self.snapshot.destinations[index].connection = if runtime.stopping {
                                Connection::Disconnected
                            } else {
                                Connection::Error {
                                    code: ErrorCode::ConnectionFailed,
                                    message:
                                        "The playback output worker stopped. Press Play to retry."
                                            .into(),
                                }
                            };
                        }
                        self.snapshot
                            .telemetry
                            .destinations
                            .retain(|m| &m.destination_id != id);
                        self.snapshot
                            .telemetry
                            .destinations
                            .push(DestinationTelemetry {
                                destination_id: id.clone(),
                                session_id: runtime.session_id,
                                level_dbfs: None,
                                queued_frames: 0,
                                dropped_frames: 0,
                                underrun_frames: 0,
                                gateway_ping_ms: None,
                            });
                    }
                    finished.push((id.clone(), restart));
                    changed = true;
                }
            }
            let retired = !finished.is_empty();
            for (id, restart) in finished {
                self.playback.remove(&id);
                if restart
                    && let Err(error) = self.start_playback(&id)
                    && let Ok(index) = self.destination_index(&id)
                {
                    self.snapshot.destinations[index].connection = Connection::Error {
                        code: error.code,
                        message: error.message,
                    };
                }
            }
            if changed {
                self.publish();
                if retired {
                    let _ = self.persist();
                }
            }
        }
    }
    fn playback_command(&mut self, command: ControlCommand) -> Result<(), AppError> {
        match command {
            ControlCommand::AddPlaybackDestination { device_id } => {
                if self.snapshot.destinations.len() >= MAX_DESTINATIONS
                    || self
                        .snapshot
                        .destinations
                        .iter()
                        .filter(|d| d.kind == DestinationKind::Playback)
                        .count()
                        >= MAX_PLAYBACK_DESTINATIONS
                {
                    return Err(error(
                        ErrorCode::InvalidSelection,
                        "Up to four destinations, including three playback outputs, are supported.",
                    ));
                }
                let device = self.playback_device(&device_id, None)?;
                let destination_id = format!("playback-{}", self.next_destination);
                self.next_destination += 1;
                self.snapshot.destinations.push(DestinationStrip {
                    session_id: 0,
                    kind: DestinationKind::Playback,
                    device_id: Some(device_id),
                    display_name: Some(device.name),
                    destination_id,
                    server_id: String::new(),
                    channel_id: String::new(),
                    connection: Connection::Disconnected,
                    volume_db: Some(0.0),
                    muted: false,
                });
                self.clear_destination_meter(self.snapshot.destinations.len() - 1);
            }
            ControlCommand::ChangePlaybackDestination {
                destination_id,
                device_id,
            } => {
                let index = self.destination_index(&destination_id)?;
                if self.snapshot.destinations[index].kind != DestinationKind::Playback {
                    return Err(error(
                        ErrorCode::InvalidSelection,
                        "Select a Discord room for this destination.",
                    ));
                }
                let device = self.playback_device(&device_id, Some(&destination_id))?;
                if self.snapshot.destinations[index].device_id.as_deref() == Some(&device_id) {
                    return Ok(());
                }
                let live = matches!(
                    self.snapshot.destinations[index].connection,
                    Connection::Connected
                        | Connection::Connecting { .. }
                        | Connection::Recovering { .. }
                );
                if matches!(
                    self.snapshot.destinations[index].connection,
                    Connection::Disconnecting | Connection::CleanupIncomplete { .. }
                ) {
                    return Err(busy());
                }
                if live {
                    self.stop_playback(&destination_id, true);
                }
                self.snapshot.destinations[index].device_id = Some(device_id);
                self.snapshot.destinations[index].display_name = Some(device.name);
                if !live {
                    self.snapshot.destinations[index].connection = Connection::Disconnected;
                }
            }
            ControlCommand::SetAllDestinationsMuted { muted } => {
                for index in 0..self.snapshot.destinations.len() {
                    self.snapshot.destinations[index].muted = muted;
                    self.update_destination_output(index);
                }
            }
            ControlCommand::DisconnectAllDestinations => {
                self.discord_restart.clear();
                self.discord.stop_all();
                self.poll_discord();
                let local: Vec<_> = self
                    .snapshot
                    .destinations
                    .iter()
                    .filter(|d| d.kind == DestinationKind::Playback)
                    .map(|d| d.destination_id.clone())
                    .collect();
                for id in local {
                    self.stop_playback(&id, false);
                }
            }
            _ => return Err(error(ErrorCode::Internal, "Unsupported playback command.")),
        }
        self.publish();
        self.persist()
    }
    fn validate_server_slot(&self, server: &str, except: Option<&str>) -> Result<(), AppError> {
        if self.snapshot.destinations.iter().any(|d| {
            d.kind == DestinationKind::Discord
                && d.server_id == server
                && Some(d.destination_id.as_str()) != except
        }) || self.discord.rooms.values().any(|r| {
            r.key.server_id == server
                && (Some(r.key.destination_id.as_str()) != except || !r.wanted)
        }) {
            return Err(error(
                ErrorCode::Busy,
                "This bot already has a room selected or finishing cleanup in this server.",
            ));
        }
        Ok(())
    }
    fn start_discord(&mut self, id: &str) -> Result<(), AppError> {
        let index = self.destination_index(id)?;
        let d = &self.snapshot.destinations[index];
        if d.kind != DestinationKind::Discord {
            return Err(error(ErrorCode::InvalidSelection, "Select a Discord room."));
        }
        if self.discord.rooms.get(id).is_some_and(|r| r.wanted) {
            return Ok(());
        }
        self.validate_destination(&d.server_id, &d.channel_id)?;
        self.validate_server_slot(&d.server_id, Some(id))?;
        if self.configuration.is_some()
            || self.snapshot.bot.status != BotStatus::Ready
            || !matches!(self.snapshot.discovery, Discovery::Ready)
        {
            return Err(Failure::Configuration.error());
        }
        if self.config.token_file.is_none() {
            return Err(Failure::Configuration.error());
        }
        self.sync_audio()?;
        self.discord.start(&self.snapshot.destinations[index])?;
        self.poll_discord();
        self.publish();
        Ok(())
    }
    fn poll_discord(&mut self) {
        if !self.discord.busy() && self.discord_restart.is_empty() {
            return;
        }
        #[cfg(windows)]
        {
            use std::sync::atomic::Ordering;
            if let Some((generation, host)) = &self.gateway_host {
                let generation = *generation;
                for status in host.take_statuses() {
                    self.discord.gateway_status(generation, status);
                }
                host.poll_deadline();
                let exited = host.exit.load(Ordering::Acquire);
                if exited != 0 {
                    self.discord.gateway_exit(generation, exited == 1);
                    if exited == 1 {
                        self.gateway_host = None;
                    }
                }
            }
            let mut exited = Vec::new();
            for (id, (generation, host)) in &self.voice_hosts {
                for status in host.take_statuses() {
                    self.discord.voice_status(id, *generation, status);
                }
                host.poll_deadline();
                let exit = host.exit.load(Ordering::Acquire);
                if exit != 0 {
                    self.discord.voice_exit(id, *generation, exit == 1);
                    if exit == 1 {
                        exited.push(id.clone());
                    }
                }
            }
            for id in exited {
                self.voice_hosts.remove(&id);
            }
        }
        let done = self.discord.advance(!self.discord_restart.is_empty());
        for (id, connection) in done {
            if let Some(d) = self
                .snapshot
                .destinations
                .iter_mut()
                .find(|d| d.destination_id == id)
            {
                d.connection = connection;
            }
            if self.discord_removing.remove(&id) {
                self.snapshot
                    .destinations
                    .retain(|d| d.destination_id != id);
            }
        }
        let restart = self.discord_restart.iter().cloned().collect::<Vec<_>>();
        for id in restart {
            if self.discord.contains(&id) {
                continue;
            }
            if self.shutting_down
                || !self
                    .snapshot
                    .destinations
                    .iter()
                    .any(|d| d.destination_id == id && d.connection == Connection::Disconnected)
            {
                self.discord_restart.remove(&id);
                continue;
            }
            if self.discord.gateway.phase == GatewayPhase::Connected {
                self.discord_restart.remove(&id);
                if let Err(error) = self.start_discord(&id)
                    && let Some(d) = self
                        .snapshot
                        .destinations
                        .iter_mut()
                        .find(|d| d.destination_id == id)
                {
                    d.connection = Connection::Error {
                        code: error.code,
                        message: error.message,
                    };
                }
            }
        }
        let effects = std::mem::take(&mut self.discord.effects);
        #[cfg(test)]
        if self.simulated_discord {
            let _ = effects;
            self.publish();
            return;
        }

        #[cfg(not(windows))]
        {
            let _ = effects;
        }
        #[cfg(windows)]
        for effect in effects {
            use crate::discord::{control::Effect, protocol::*};
            use std::os::windows::io::AsRawHandle;
            match effect {
                Effect::LaunchGateway { generation } => {
                    if self.discord.rooms.values().all(|r| !r.wanted) {
                        self.discord.gateway_exit(generation, true);
                        continue;
                    }

                    if self.gateway_host.is_some() {
                        self.discord.gateway_failed(Failure::Protocol);
                        continue;
                    }
                    let Some(token_file) = self.config.token_file.clone() else {
                        self.discord.gateway_failed(Failure::Configuration);
                        continue;
                    };
                    self.gateway_host = Some((
                        generation,
                        GatewayHost::launch(
                            self.config.executable.clone(),
                            "--gateway-worker",
                            generation,
                            Vec::new(),
                            GatewayCommand::Init { token_file },
                            GatewayCommand::Stop,
                        ),
                    ));
                }
                Effect::StopGateway => {
                    if let Some((_, host)) = &self.gateway_host {
                        host.stop();
                    }
                }
                Effect::Gateway(command) => {
                    if let GatewayCommand::Join { key } = &command
                        && !self
                            .discord
                            .rooms
                            .get(&key.destination_id)
                            .is_some_and(|r| r.key == *key && r.wanted)
                    {
                        continue;
                    }

                    if self
                        .gateway_host
                        .as_ref()
                        .is_none_or(|(_, host)| !host.send(command))
                    {
                        self.discord.gateway_failed(Failure::Connection);
                    }
                }
                Effect::StopVoice {
                    destination_id,
                    generation,
                } => {
                    if let Some((current, host)) = self
                        .voice_hosts
                        .get(&destination_id)
                        .filter(|(current, _)| *current == generation)
                    {
                        let _ = current;
                        host.stop();
                    }
                }
                Effect::LaunchVoice {
                    key,
                    generation,
                    revision,
                    secret,
                } => {
                    if !self
                        .discord
                        .rooms
                        .get(&key.destination_id)
                        .is_some_and(|r| r.key == key && r.wanted && r.voice == Some(generation))
                    {
                        self.discord
                            .voice_exit(&key.destination_id, generation, true);
                        continue;
                    }
                    if self.voice_hosts.contains_key(&key.destination_id) {
                        self.discord
                            .stop(&key.destination_id, Some(Failure::Protocol.error()));
                        continue;
                    }
                    let Some(destination) = self
                        .snapshot
                        .destinations
                        .iter()
                        .find(|d| d.destination_id == key.destination_id)
                    else {
                        self.discord.stop(&key.destination_id, None);
                        continue;
                    };
                    let inherited = self
                        .audio
                        .as_ref()
                        .and_then(|audio| audio.bus.inheritable().ok());
                    let Some(inherited) = inherited else {
                        self.discord
                            .stop(&key.destination_id, Some(Failure::Capture.error()));
                        self.discord
                            .voice_exit(&key.destination_id, generation, true);
                        continue;
                    };
                    let source_id = format!("mix-handle:{}", inherited.as_raw_handle() as usize);
                    let command = VoiceCommand::Init {
                        key: key.clone(),
                        gateway_epoch: self.discord.gateway.generation,
                        credential_revision: revision,
                        source_id,
                        secret,
                        output: Output {
                            volume_percent: 100,
                            volume_linear: Some(crate::mixing::linear(destination.volume_db)),
                            muted: destination.muted,
                        },
                    };
                    self.voice_hosts.insert(
                        key.destination_id,
                        (
                            generation,
                            VoiceHost::launch(
                                self.config.executable.clone(),
                                "--voice-worker",
                                generation,
                                vec![inherited],
                                command,
                                VoiceCommand::Stop,
                            ),
                        ),
                    );
                }
            }
        }
        if !self.discord.busy() || Instant::now() >= self.next_discord_publish {
            self.next_discord_publish = Instant::now() + Duration::from_millis(100);
            self.publish();
        }
    }

    fn configure_token(&mut self, token: String, reply: oneshot::Sender<Result<(), AppError>>) {
        if self.configuration_busy() {
            let _ = reply.send(Err(busy()));
            return;
        }
        let Some(path) = self.config.token_file.clone() else {
            let _ = reply.send(Err(Failure::Configuration.error()));
            return;
        };
        if !separate_settings(&self.config) {
            let _ = reply.send(Err(Failure::Configuration.error()));
            return;
        }
        let token = token.trim().to_owned();
        if token.is_empty()
            || token.len() > 4096
            || token.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            let _ = reply.send(Err(Failure::Configuration.error()));
            return;
        }
        self.start_configuration(reply, move |control| {
            configure_bot_token(path, token, control)
        });
    }

    fn start_configuration<Fut>(
        &mut self,
        reply: oneshot::Sender<Result<(), AppError>>,
        operation: impl FnOnce(ConfigurationControl) -> Fut + Send + 'static,
    ) where
        Fut: std::future::Future<Output = Result<(), AppError>> + Send + 'static,
    {
        if self.configuration_busy() {
            let _ = reply.send(Err(busy()));
            return;
        }
        self.catalog_epoch = self.catalog_epoch.wrapping_add(1);
        let epoch = self.catalog_epoch;
        let bot = self.snapshot.bot.clone();
        let discovery = self.snapshot.discovery.clone();
        self.snapshot.bot = BotProfile {
            id: None,
            avatar_url: None,
            status: BotStatus::Checking,
            name: None,
            message: None,
        };
        self.snapshot.discovery = Discovery::Loading;
        self.publish();
        let events = self.events.clone();
        let control = ConfigurationControl::default();
        let task_control = control.clone();
        let task = tokio::spawn(async move {
            let result = operation(task_control).await;
            let _ = events.send(Event::Configured(epoch, result)).await;
        });
        self.configuration = Some(ConfigurationCheck {
            epoch,
            bot,
            discovery,
            reply,
            task,
            control,
        });
    }
    fn configuration_busy(&self) -> bool {
        if self.discord.busy() {
            return true;
        }
        self.configuration.is_some() || self.shutting_down
    }
    fn event(&mut self, event: Event) {
        match event {
            Event::PlaybackScanFinished(epoch, result) => {
                if epoch != self.playback_epoch {
                    return;
                }
                self.playback_scanning = false;
                if let Some(result) = result {
                    self.event(Event::PlaybackDevices(epoch, result));
                }
            }
            Event::PlaybackDevices(epoch, result) => {
                if epoch != self.playback_epoch || self.shutting_down {
                    return;
                }
                match result {
                    Ok(devices) => {
                        self.snapshot.playback_devices = devices;
                        self.snapshot.playback_discovery = Discovery::Ready;
                    }
                    Err(error) => {
                        self.snapshot.playback_discovery = Discovery::Error {
                            message: error.message,
                        };
                    }
                }
                self.publish();
            }
            Event::SourceScanFinished(epoch, result) => {
                if epoch != self.source_epoch {
                    return;
                }
                self.source_scanning = false;
                if let Some(result) = result {
                    self.event(Event::Sources(epoch, result));
                }
            }
            Event::Sources(epoch, result) => {
                if epoch != self.source_epoch || self.shutting_down {
                    return;
                }
                match result {
                    Ok(mut sources) => {
                        // Keep the confirmed descriptor when its live root exits.
                        // Refresh never changes the selected/active route.
                        if let Some(selected) = self.snapshot.selection.source_id.as_ref()
                            && !sources.iter().any(|source| &source.id == selected)
                            && let Some(mut previous) = self
                                .snapshot
                                .sources
                                .iter()
                                .find(|source| &source.id == selected)
                                .cloned()
                        {
                            #[cfg(windows)]
                            let still_running = previous.kind == SourceKind::Application
                                && crate::audio::validate_source(&previous.id).is_ok();
                            #[cfg(not(windows))]
                            let still_running = false;
                            previous.available = still_running;
                            if !still_running {
                                previous.detail = Some(
                                    "No longer available. Refresh and select a running source."
                                        .into(),
                                );
                            }
                            sources.push(previous);
                        }
                        for input in &mut self.snapshot.inputs {
                            if let Some(source) =
                                sources.iter().find(|source| source.id == input.source_id)
                            {
                                input.display_name = source.name.clone();
                                input.detail = source.detail.clone();
                            }
                        }
                        self.snapshot.sources = sources;
                        self.snapshot.source_discovery = Discovery::Ready;
                    }
                    Err(error) => {
                        self.snapshot.source_discovery = Discovery::Error {
                            message: error.message,
                        };
                    }
                }
                self.publish();
            }
            Event::Unconfigured(epoch) => {
                if epoch != self.catalog_epoch || self.shutting_down {
                    return;
                }
                self.snapshot.bot = BotProfile {
                    id: None,
                    avatar_url: None,
                    status: BotStatus::Unconfigured,
                    name: None,
                    message: None,
                };
                self.snapshot.discovery = match &self.preferences_error {
                    Some(error) => Discovery::Error {
                        message: error.message.clone(),
                    },
                    None => Discovery::Idle,
                };
                self.publish();
            }
            Event::Configured(epoch, result) => {
                if self
                    .configuration
                    .as_ref()
                    .is_none_or(|check| check.epoch != epoch)
                {
                    return;
                }
                let configuration = self
                    .configuration
                    .take()
                    .expect("matched configuration epoch");
                if self.shutting_down {
                    let _ = configuration.reply.send(result);
                    return;
                }
                if result.is_ok() {
                    self.refresh();
                } else {
                    self.snapshot.bot = configuration.bot;
                    self.snapshot.discovery = configuration.discovery;
                    if matches!(self.snapshot.discovery, Discovery::Loading) {
                        // The previous discovery was invalidated while testing;
                        // restart it against the unchanged, previously saved token.
                        self.refresh();
                    } else {
                        self.publish();
                    }
                }
                let _ = configuration.reply.send(result);
            }
            Event::Catalog(epoch, result) => {
                if epoch != self.catalog_epoch || self.shutting_down {
                    return;
                }
                match result {
                    Ok(catalog) => {
                        self.snapshot.servers = catalog.servers;
                        self.snapshot.bot = BotProfile {
                            id: catalog.id,
                            avatar_url: catalog.avatar_url,
                            status: BotStatus::Ready,
                            name: Some(catalog.name),
                            message: None,
                        };
                        self.snapshot.discovery = match &self.preferences_error {
                            Some(e) => Discovery::Error {
                                message: e.message.clone(),
                            },
                            None => Discovery::Ready,
                        };
                    }
                    Err(e) => {
                        self.snapshot.bot = BotProfile {
                            id: None,
                            avatar_url: None,
                            status: if self.config.token_file.is_none() {
                                BotStatus::Unconfigured
                            } else if e.code == ErrorCode::Authentication {
                                BotStatus::Invalid
                            } else {
                                BotStatus::Error
                            },
                            name: None,
                            message: Some(e.message.clone()),
                        };
                        self.snapshot.discovery = Discovery::Error { message: e.message };
                    }
                }
                self.publish();
            }
        }
    }
}

#[cfg(not(windows))]
async fn discover(_: &ControllerConfig) -> Result<Catalog, AppError> {
    Err(error(
        ErrorCode::Configuration,
        "Recording and Discord are supported on Windows.",
    ))
}

#[cfg(not(windows))]
async fn configure_bot_token(
    _: PathBuf,
    _: String,
    _: ConfigurationControl,
) -> Result<(), AppError> {
    Err(Failure::Configuration.error())
}

#[cfg(any(windows, test))]
async fn validate_then_save(
    control: ConfigurationControl,
    validation: impl std::future::Future<Output = Result<(), AppError>>,
    save: impl FnOnce() -> Result<(), AppError> + Send + 'static,
) -> Result<(), AppError> {
    tokio::time::timeout(Duration::from_secs(15), validation)
        .await
        .unwrap_or_else(|_| Err(Failure::Connection.error()))?;
    if !control.begin_save() {
        return Err(unavailable());
    }
    // File encryption/write work cannot block the actor or its controls. Once
    // saving starts, await its actual result; a timeout must not restore old UI
    // state while an uncancelled blocking write is still changing the token.
    tokio::task::spawn_blocking(save)
        .await
        .map_err(|_| unavailable())?
}

#[cfg(windows)]
async fn configure_bot_token(
    path: PathBuf,
    token: String,
    control: ConfigurationControl,
) -> Result<(), AppError> {
    let http = serenity::http::Http::new(&token);
    validate_then_save(
        control,
        async move {
            let user = http.get_current_user().await.map_err(rest_error)?;
            if !user.bot {
                return Err(Failure::Authentication.error());
            }
            Ok(())
        },
        move || {
            crate::storage::save_token(&path, &token).map_err(|_| Failure::Configuration.error())
        },
    )
    .await
}

#[cfg(windows)]
fn http(config: &ControllerConfig) -> Result<serenity::http::Http, AppError> {
    let token = protocol::read_token(
        config
            .token_file
            .as_deref()
            .ok_or_else(|| Failure::Configuration.error())?,
    )?;
    Ok(serenity::http::Http::new(&token))
}
#[cfg(windows)]
pub(crate) fn rest_error(e: serenity::Error) -> AppError {
    match &e {
        serenity::Error::Http(e) if e.status_code().is_some_and(|s| s.as_u16() == 401) => {
            Failure::Authentication.error()
        }
        serenity::Error::Http(e) if e.status_code().is_some_and(|s| s.as_u16() == 403) => {
            Failure::Permission.error()
        }
        _ => Failure::Connection.error(),
    }
}
#[cfg(windows)]
async fn discover(config: &ControllerConfig) -> Result<Catalog, AppError> {
    use serenity::{http::GuildPagination, model::channel::ChannelType};
    let config = config.clone();
    let http = tokio::task::spawn_blocking(move || http(&config))
        .await
        .map_err(|_| unavailable())??;
    let user = http.get_current_user().await.map_err(rest_error)?;
    if !user.bot {
        return Err(Failure::Authentication.error());
    }
    let mut servers = Vec::new();
    let mut after = None;
    // Paginate REST guild discovery without a gateway. Bound pathological catalogs.
    for _ in 0..50 {
        let guilds = http
            .get_guilds(after.map(GuildPagination::After), Some(200))
            .await
            .map_err(rest_error)?;
        let complete = guilds.len() < 200;
        for guild in &guilds {
            let catalog = http.get_channels(guild.id).await.map_err(rest_error)?;
            let categories: std::collections::HashMap<_, _> = catalog
                .iter()
                .filter(|c| c.kind == ChannelType::Category)
                .map(|c| (c.id, c.name.clone()))
                .collect();
            let mut channels: Vec<_> = catalog
                .into_iter()
                .filter(|c| c.kind == ChannelType::Voice)
                .map(|c| VoiceChannel {
                    id: c.id.to_string(),
                    name: c.name,
                    category_name: c.parent_id.and_then(|id| categories.get(&id).cloned()),
                    position: c.position,
                })
                .collect();
            channels.sort_by(|a, b| a.position.cmp(&b.position).then_with(|| a.id.cmp(&b.id)));
            servers.push(Server {
                id: guild.id.to_string(),
                name: guild.name.clone(),
                icon_url: guild.icon_url(),
                channels,
            });
        }
        after = guilds.last().map(|g| g.id);
        if complete {
            return Ok(Catalog {
                id: Some(user.id.to_string()),
                avatar_url: user.avatar_url(),
                servers,
                name: user.name.clone(),
            });
        }
    }
    Err(error(
        ErrorCode::Configuration,
        "The bot catalog exceeds the supported size.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> ControllerConfig {
        let synthetic_missing = std::env::temp_dir().join(format!(
            "echocord-runtime-test-missing-token-{}",
            std::process::id()
        ));
        assert!(!synthetic_missing.exists());
        ControllerConfig {
            executable: "unavailable-engine".into(),
            token_file: Some(synthetic_missing),
            settings_file: None,
        }
    }
    fn actor() -> Actor {
        let (snapshots, _) = watch::channel(ControlSnapshot::default());
        let (events, _) = mpsc::channel(32);
        let mut actor = Actor::new(config(), snapshots, events);
        actor.snapshot.sources.push(RecordingSource {
            id: "recording".into(),
            name: "Recording".into(),
            kind: SourceKind::Recording,
            available: true,
            detail: None,
        });
        actor.snapshot.servers.push(Server {
            id: "1".into(),
            name: "Server".into(),
            icon_url: None,
            channels: vec![VoiceChannel {
                id: "2".into(),
                name: "Voice".into(),
                category_name: None,
                position: 0,
            }],
        });
        actor.snapshot.selection = Selection {
            source_id: Some("recording".into()),
            server_id: Some("1".into()),
            channel_id: Some("2".into()),
        };
        actor.snapshot.bot.status = BotStatus::Ready;
        actor.snapshot.discovery = Discovery::Ready;
        actor.snapshot.session_id = 4;
        actor
    }

    #[test]
    fn source_discovery_is_independent_and_preserves_selected_descriptor() {
        let mut a = actor();
        a.source_epoch = 7;
        a.snapshot.inputs.push(InputStrip {
            strip_id: "input-1".into(),
            source_id: "recording".into(),
            kind: SourceKind::Recording,
            display_name: "Recording".into(),
            detail: None,
            phase: InputPhase::Stopped,
            fault: None,
            gain_db: Some(0.0),
            muted: false,
            colour_slot: 0,
            restored: false,
        });
        a.snapshot.selection.server_id = None;
        a.snapshot.selection.channel_id = None;
        let selected = a.snapshot.selection.clone();
        let bot = a.snapshot.bot.clone();
        a.event(Event::Sources(6, Ok(Vec::new())));
        assert!(a.snapshot.sources[0].available);
        a.event(Event::Sources(7, Ok(Vec::new())));
        assert_eq!(a.snapshot.sources.len(), 1);
        assert!(!a.snapshot.sources[0].available);
        assert_eq!(a.snapshot.selection, selected);
        assert_eq!(a.snapshot.bot, bot);
        assert_eq!(a.snapshot.source_discovery, Discovery::Ready);
        let sources = a.snapshot.sources.clone();
        a.event(Event::Sources(7, Err(Failure::Capture.error())));
        assert_eq!(a.snapshot.sources, sources);
        assert!(matches!(
            a.snapshot.source_discovery,
            Discovery::Error { .. }
        ));
    }

    #[tokio::test]
    async fn timed_out_source_scan_keeps_occupancy_and_discards_late_results() {
        let mut a = actor();
        a.source_epoch = 7;
        a.source_scanning = true;
        a.snapshot.source_discovery = Discovery::Loading;
        let sources = a.snapshot.sources.clone();
        let (release, released) = std::sync::mpsc::channel();
        let scan = tokio::task::spawn_blocking(move || {
            released.recv().unwrap();
            Ok(Vec::new())
        });
        let (events, mut receive) = mpsc::channel(2);
        let watcher = tokio::spawn(source_scan(7, events, scan, Duration::from_millis(10)));
        a.event(receive.recv().await.unwrap());
        assert!(matches!(
            a.snapshot.source_discovery,
            Discovery::Error { .. }
        ));
        a.command(ControlCommand::RefreshSources).unwrap();
        assert_eq!(a.source_epoch, 7);
        assert!(a.source_scanning);
        release.send(()).unwrap();
        a.event(receive.recv().await.unwrap());
        watcher.await.unwrap();
        assert!(!a.source_scanning);
        assert_eq!(a.snapshot.sources, sources);
        assert!(matches!(
            a.snapshot.source_discovery,
            Discovery::Error { .. }
        ));
        a.command(ControlCommand::RefreshSources).unwrap();
        assert_eq!(a.source_epoch, 8);
        assert!(a.source_scanning);
    }
    fn running_actor(mut actor: Actor) -> (Controller, mpsc::Sender<Event>) {
        let (events, event_rx) = mpsc::channel(32);
        actor.events = events.clone();
        (running_actor_on(actor, event_rx), events)
    }
    fn running_actor_on(mut actor: Actor, event_rx: mpsc::Receiver<Event>) -> Controller {
        let (commands, receiver) = mpsc::channel(32);
        let controller = Controller {
            commands,
            snapshots: actor.snapshots.subscribe(),
        };
        actor.publish();
        tokio::spawn(actor.run_requests(receiver, event_rx));
        controller
    }
    #[tokio::test]
    async fn private_token_check_is_serialized_responsive_and_restores_previous_configuration() {
        let mut a = actor();
        let (events, event_rx) = mpsc::channel(32);
        a.events = events.clone();
        a.snapshot.bot.name = Some("Previously saved bot".into());
        let previous = a.snapshot.bot.clone();
        let stale_epoch = a.catalog_epoch;
        let stale_catalog = Catalog {
            id: None,
            avatar_url: None,
            servers: a.snapshot.servers.clone(),
            name: "Stale bot".into(),
        };
        let (reply, result) = oneshot::channel();
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        a.start_configuration(reply, move |_| async move {
            let _ = started.send(());
            let _ = release_rx.await;
            Err(Failure::Authentication.error())
        });
        let controller = running_actor_on(a, event_rx);
        started_rx.await.unwrap();
        assert_eq!(controller.snapshot().bot.status, BotStatus::Checking);
        assert_eq!(controller.snapshot().discovery, Discovery::Loading);
        events
            .send(Event::Catalog(stale_epoch, Ok(stale_catalog)))
            .await
            .unwrap();
        for command in [
            ControlCommand::SetMuted { muted: true },
            ControlCommand::SetVolume { percent: 42 },
        ] {
            tokio::time::timeout(Duration::from_millis(250), controller.dispatch(command))
                .await
                .unwrap()
                .unwrap();
        }
        assert_eq!(controller.snapshot().bot.status, BotStatus::Checking);
        assert!(controller.snapshot().output.muted);
        assert_eq!(controller.snapshot().output.volume_percent, 42);
        assert_eq!(
            controller
                .configure_token("synthetic-private-token".into())
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        assert_eq!(
            controller
                .dispatch(ControlCommand::RefreshCatalog)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        assert!(controller.dispatch(ControlCommand::Connect).await.is_err());
        assert!(
            !serde_json::to_string(&controller.snapshot())
                .unwrap()
                .contains("synthetic-private-token")
        );
        release.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), result)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Authentication
        );
        assert_eq!(controller.snapshot().bot, previous);
        assert_eq!(controller.snapshot().discovery, Discovery::Ready);
        controller.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn malformed_private_token_requests_do_not_replace_saved_configuration() {
        let a = actor();
        let previous = a.snapshot.bot.clone();
        let (controller, _) = running_actor(a);
        for token in [String::new(), "invalid token".into(), "x".repeat(4097)] {
            assert_eq!(
                controller.configure_token(token).await.unwrap_err().code,
                ErrorCode::Configuration
            );
            assert_eq!(controller.snapshot().bot, previous);
        }
        controller.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn configuration_refreshes_only_after_successful_persistence() {
        let mut a = actor();
        let old_epoch = a.catalog_epoch;
        let (reply, result) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        a.start_configuration(reply, move |_| async move {
            let _ = release_rx.await;
            Ok(())
        });
        let validation_epoch = a.catalog_epoch;
        assert_ne!(validation_epoch, old_epoch);
        a.event(Event::Configured(old_epoch, Ok(())));
        assert!(a.configuration.is_some());
        assert_eq!(a.snapshot.bot.status, BotStatus::Checking);
        a.event(Event::Configured(validation_epoch, Ok(())));
        result.await.unwrap().unwrap();
        assert!(a.configuration.is_none());
        assert_ne!(a.catalog_epoch, validation_epoch);
        assert_eq!(a.snapshot.bot.status, BotStatus::Checking);
        assert_eq!(a.snapshot.discovery, Discovery::Loading);
        let _ = release.send(());
    }
    #[tokio::test]
    async fn failed_validation_never_calls_storage_and_success_waits_for_actual_save() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let saved = Arc::new(AtomicBool::new(false));
        let flag = saved.clone();
        let result = validate_then_save(
            ConfigurationControl::default(),
            async { Err(Failure::Authentication.error()) },
            move || {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        assert_eq!(result.unwrap_err().code, ErrorCode::Authentication);
        assert!(!saved.load(Ordering::SeqCst));
        let validated = Arc::new(AtomicBool::new(false));
        let validation_flag = validated.clone();
        let flag = saved.clone();
        validate_then_save(
            ConfigurationControl::default(),
            async move {
                validation_flag.store(true, Ordering::SeqCst);
                Ok(())
            },
            move || {
                assert!(validated.load(Ordering::SeqCst));
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(saved.load(Ordering::SeqCst));
        let error = validate_then_save(ConfigurationControl::default(), async { Ok(()) }, || {
            Err(Failure::Configuration.error())
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Configuration);
    }
    #[tokio::test(start_paused = true)]
    async fn stalled_bot_validation_times_out_without_touching_storage() {
        let error = validate_then_save(
            ConfigurationControl::default(),
            std::future::pending(),
            || panic!("timed-out validation must not save"),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::ConnectionFailed);
    }
    #[tokio::test]
    async fn missing_default_token_starts_unconfigured_without_an_error_or_a_token_file() {
        let config = config();
        let path = config.token_file.clone().unwrap();
        let controller = Controller::spawn(config);
        let mut snapshots = controller.subscribe();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let snapshot = controller.snapshot();
                if snapshot.revision > 0
                    && snapshot.bot.status == BotStatus::Unconfigured
                    && snapshot.discovery == Discovery::Idle
                {
                    break;
                }
                snapshots.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(controller.snapshot().bot.message.is_none());
        assert!(!path.exists());
        controller.shutdown().await.unwrap();
    }
    #[test]
    fn only_not_found_is_classified_as_missing_configuration() {
        assert!(missing_token_error(std::io::ErrorKind::NotFound));
        assert!(!missing_token_error(std::io::ErrorKind::PermissionDenied));
        assert!(!missing_token_error(std::io::ErrorKind::InvalidData));
        let directory =
            std::env::temp_dir().join(format!("echocord-config-presence-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut config = config();
        let token_path = directory.join("token");
        config.token_file = Some(token_path.clone());
        assert!(missing_token(&config));
        std::fs::write(&token_path, b"malformed envelope").unwrap();
        assert!(!missing_token(&config));
        config.token_file = Some(directory.clone());
        assert!(!missing_token(&config));
        std::fs::remove_dir_all(directory).unwrap();
        let mut a = actor();
        a.event(Event::Catalog(
            a.catalog_epoch,
            Err(Failure::Configuration.error()),
        ));
        assert_eq!(a.snapshot.bot.status, BotStatus::Error);
    }
    #[tokio::test]
    async fn first_output_change_creates_only_the_preferences_parent_directory() {
        let directory =
            std::env::temp_dir().join(format!("echocord-first-settings-{}", std::process::id()));
        assert!(!directory.exists());
        let mut config = config();
        let settings = directory.join("new-profile/preferences.json");
        config.settings_file = Some(settings.clone());
        let token_file = config.token_file.clone().unwrap();
        let controller = Controller::spawn(config.clone());
        controller
            .dispatch(ControlCommand::SetMuted { muted: true })
            .await
            .unwrap();
        assert!(settings.is_file());
        assert!(!token_file.exists());
        assert!(load_preferences(&config).unwrap().output.unwrap().muted);
        controller.shutdown().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[tokio::test]
    async fn private_token_configuration_rejects_the_preferences_file_before_checking_or_saving() {
        let mut a = actor();
        a.config.settings_file = a.config.token_file.clone();
        let previous = a.snapshot.clone();
        let (reply, result) = oneshot::channel();
        a.configure_token("synthetic-private-token".into(), reply);
        assert_eq!(
            result.await.unwrap().unwrap_err().code,
            ErrorCode::Configuration
        );
        assert_eq!(a.snapshot, previous);
        assert!(a.configuration.is_none());
        #[cfg(unix)]
        {
            let directory =
                std::env::temp_dir().join(format!("echocord-config-alias-{}", std::process::id()));
            std::fs::create_dir_all(&directory).unwrap();
            let settings = directory.join("preferences.json");
            let token = directory.join("token-alias");
            std::fs::write(&settings, b"unchanged preferences").unwrap();
            std::os::unix::fs::symlink(&settings, &token).unwrap();
            a.config.settings_file = Some(settings.clone());
            a.config.token_file = Some(token);
            let (reply, result) = oneshot::channel();
            a.configure_token("synthetic-private-token".into(), reply);
            assert_eq!(
                result.await.unwrap().unwrap_err().code,
                ErrorCode::Configuration
            );
            assert_eq!(a.snapshot, previous);
            assert!(a.configuration.is_none());
            assert_eq!(std::fs::read(&settings).unwrap(), b"unchanged preferences");
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
    #[tokio::test]
    async fn delayed_token_save_keeps_output_responsive_and_shutdown_waits_for_storage() {
        let mut a = actor();
        let (events, event_rx) = mpsc::channel(32);
        a.events = events;
        let (reply, mut result) = oneshot::channel();
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        a.start_configuration(reply, move |control| {
            validate_then_save(control, async { Ok(()) }, move || {
                let _ = started.send(());
                release_rx.recv().unwrap();
                Ok(())
            })
        });
        let controller = running_actor_on(a, event_rx);
        started_rx.await.unwrap();
        tokio::time::timeout(
            Duration::from_millis(250),
            controller.dispatch(ControlCommand::SetMuted { muted: true }),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            result.try_recv().unwrap_err(),
            oneshot::error::TryRecvError::Empty
        );
        assert_eq!(controller.snapshot().bot.status, BotStatus::Checking);
        assert!(controller.snapshot().output.muted);
        let (reply, mut shutdown) = oneshot::channel();
        let mut snapshots = controller.subscribe();
        let revision = controller.snapshot().revision;
        controller
            .commands
            .send(Request::Shutdown(reply))
            .await
            .unwrap_or_else(|_| panic!("controller must accept shutdown"));
        tokio::time::timeout(Duration::from_secs(1), async {
            while controller.snapshot().revision == revision {
                snapshots.changed().await.unwrap();
            }
        })
        .await
        .expect("shutdown must be processed while save is blocked");
        assert_eq!(
            result.try_recv().unwrap_err(),
            oneshot::error::TryRecvError::Empty
        );
        assert_eq!(
            shutdown.try_recv().unwrap_err(),
            oneshot::error::TryRecvError::Empty
        );
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), result)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), shutdown)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn shutdown_aborts_validation_and_prevents_a_later_save() {
        let mut a = actor();
        let (events, event_rx) = mpsc::channel(32);
        a.events = events;
        let (reply, result) = oneshot::channel();
        let (started, started_rx) = oneshot::channel();
        let (mut release, release_rx) = oneshot::channel();
        a.start_configuration(reply, move |control| {
            validate_then_save(
                control,
                async move {
                    let _ = started.send(());
                    let _ = release_rx.await;
                    Ok(())
                },
                || panic!("cancelled validation must never reach storage"),
            )
        });
        let controller = running_actor_on(a, event_rx);
        started_rx.await.unwrap();
        tokio::time::timeout(Duration::from_millis(250), controller.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.await.unwrap().unwrap_err().code, ErrorCode::Internal);
        tokio::time::timeout(Duration::from_millis(250), release.closed())
            .await
            .unwrap();
        assert!(release.send(()).is_err());
        let control = ConfigurationControl::default();
        assert!(control.cancel_validation());
        let error = validate_then_save(control, async { Ok(()) }, || {
            panic!("late validation success must not commit a cancelled save")
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
    }

    #[test]
    fn preference_load_failure_survives_discovery_until_preferences_are_saved() {
        let mut a = actor();
        a.preferences_error = Some(Failure::Configuration.error());
        let catalog = Catalog {
            id: None,
            avatar_url: None,
            servers: a.snapshot.servers.clone(),
            name: "Bot".into(),
        };
        a.event(Event::Catalog(a.catalog_epoch, Ok(catalog)));
        assert!(matches!(a.snapshot.discovery, Discovery::Error { .. }));
        a.command(ControlCommand::SelectSource {
            source_id: "recording".into(),
        })
        .unwrap();
        assert!(matches!(a.snapshot.discovery, Discovery::Ready));
    }
    #[test]
    fn saves_only_generic_preferences_and_rejects_secret_destination() {
        let directory =
            std::env::temp_dir().join(format!("echocord-preferences-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut config = config();
        config.settings_file = Some(directory.join("settings.json"));
        let mut a = actor();
        a.snapshot.output.muted = true;
        save_preferences(&config, &a.snapshot).unwrap();
        a.snapshot.output.volume_percent = 60;
        save_preferences(&config, &a.snapshot).unwrap();
        let bytes = std::fs::read_to_string(config.settings_file.as_ref().unwrap()).unwrap();
        assert!(!bytes.contains("private-token"));
        assert!(!bytes.contains("bot"));
        let restored = load_preferences(&config).unwrap().output.unwrap();
        assert!(restored.muted);
        assert_eq!(restored.volume_percent, 60);
        assert_eq!(
            load_preferences(&config)
                .unwrap()
                .selection
                .source_id
                .as_deref(),
            Some("recording")
        );
        a.snapshot.selection.source_id = Some("app:42:0000000000000001".into());
        save_preferences(&config, &a.snapshot).unwrap();
        let app_restored = load_preferences(&config).unwrap();
        assert!(app_restored.selection.source_id.is_none());
        assert_eq!(
            app_restored.selection.server_id,
            a.snapshot.selection.server_id
        );
        assert_eq!(app_restored.output.unwrap(), a.snapshot.output);
        config.token_file = config.settings_file.clone();
        assert!(save_preferences(&config, &a.snapshot).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(not(windows))]
    #[test]
    fn duplicate_sources_and_ninth_source_are_rejected_without_changing_mix() {
        let mut a = actor();
        for index in 0..9 {
            a.snapshot.sources.push(RecordingSource {
                id: format!("source-{index}"),
                name: "Source".into(),
                kind: SourceKind::Recording,
                available: true,
                detail: None,
            });
        }
        for index in 0..8 {
            a.command(ControlCommand::AddSource {
                source_id: format!("source-{index}"),
            })
            .unwrap();
        }
        assert!(
            a.command(ControlCommand::AddSource {
                source_id: "source-0".into()
            })
            .is_err()
        );
        assert!(
            a.command(ControlCommand::AddSource {
                source_id: "source-8".into()
            })
            .is_err()
        );
        assert_eq!(a.snapshot.inputs.len(), MAX_INPUTS);
        assert_eq!(
            a.snapshot
                .inputs
                .iter()
                .map(|i| i.colour_slot)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 5, 5]
        );
    }
    #[test]
    fn destination_gain_preserves_fractional_db_and_one_destination_limit() {
        let mut a = actor();
        a.command(ControlCommand::AddDestination {
            server_id: "1".into(),
            channel_id: "2".into(),
        })
        .unwrap();
        assert!(
            a.command(ControlCommand::AddDestination {
                server_id: "1".into(),
                channel_id: "2".into()
            })
            .is_err()
        );
        a.command(ControlCommand::SetDestinationVolume {
            destination_id: "discord-1".into(),
            volume_db: Some(-59.5),
        })
        .unwrap();
        assert_eq!(a.snapshot.destinations[0].volume_db, Some(-59.5));
        assert!(a.snapshot.output.volume_linear.unwrap() > 0.001);
        assert_eq!(a.snapshot.output.volume_percent, 0); // Legacy percent is not the native gain.
        assert!(
            a.command(ControlCommand::SetDestinationVolume {
                destination_id: "discord-1".into(),
                volume_db: Some(f32::NAN)
            })
            .is_err()
        );
    }
    #[test]
    fn versioned_preferences_migrate_restore_safely_and_preserve_future_files() {
        let directory =
            std::env::temp_dir().join(format!("echocord-mixing-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.json");
        let mut config = config();
        config.settings_file = Some(path.clone());
        std::fs::write(&path, r#"{"selection":{"sourceId":"recording","serverId":"1","channelId":"2"},"output":{"volumePercent":37,"muted":true}}"#).unwrap();
        let legacy = load_preferences(&config).unwrap();
        assert_eq!(legacy.inputs.len(), 1);
        assert_eq!(legacy.inputs[0].phase, InputPhase::Opening);
        assert_eq!(legacy.destinations[0].connection, Connection::Disconnected);
        assert!(legacy.destinations[0].muted);
        let mut snapshot = ControlSnapshot {
            inputs: legacy.inputs,
            destinations: legacy.destinations,
            ..ControlSnapshot::default()
        };
        snapshot.inputs[0].source_id = "app:42:0000000000000001".into();
        snapshot.inputs[0].kind = SourceKind::Recording; // Do not trust serialized kind.
        save_preferences(&config, &snapshot).unwrap();
        let restored = load_preferences(&config).unwrap();
        assert_eq!(restored.inputs[0].kind, SourceKind::Application);
        assert_eq!(restored.inputs[0].phase, InputPhase::Stopped);
        snapshot.settings.restore_setup = false;
        save_preferences(&config, &snapshot).unwrap();
        assert!(load_preferences(&config).unwrap().inputs.is_empty());
        let future = r#"{"version":99,"futureSetting":"preserve"}"#;
        std::fs::write(&path, future).unwrap();
        assert!(load_preferences(&config).is_err());
        assert!(save_preferences(&config, &snapshot).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), future);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[tokio::test]
    async fn prepared_controller_is_inert_until_single_instance_admission() {
        let controller = Controller::prepare(config());
        tokio::task::yield_now().await;
        assert_eq!(controller.snapshot(), ControlSnapshot::default());
        controller.shutdown().await.unwrap();
    }
    #[test]
    fn failed_audio_worker_rejects_new_intent_without_phantom_cards_but_allows_cleanup() {
        let mut a = actor();
        a.audio_failed = true;
        a.snapshot.inputs.push(InputStrip {
            strip_id: "input-1".into(),
            source_id: "recording".into(),
            kind: SourceKind::Recording,
            display_name: "Recording".into(),
            detail: None,
            phase: InputPhase::Fault,
            fault: Some(Failure::Capture.error()),
            gain_db: Some(0.0),
            muted: false,
            colour_slot: 0,
            restored: false,
        });
        let before = a.snapshot.clone();
        for command in [
            ControlCommand::AddSource {
                source_id: "recording".into(),
            },
            ControlCommand::ReplaceSource {
                strip_id: "input-1".into(),
                source_id: "recording".into(),
            },
            ControlCommand::StartCapture {
                strip_id: "input-1".into(),
            },
        ] {
            assert!(a.command(command).is_err());
            assert_eq!(a.snapshot, before);
        }
        a.command(ControlCommand::StopCapture {
            strip_id: "input-1".into(),
        })
        .unwrap();
        assert_eq!(a.snapshot.inputs[0].phase, InputPhase::Stopped);
        a.command(ControlCommand::RemoveSource {
            strip_id: "input-1".into(),
        })
        .unwrap();
        assert!(a.snapshot.inputs.is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn selecting_the_current_stopped_or_faulted_source_still_retries_capture() {
        for phase in [InputPhase::Stopped, InputPhase::Fault] {
            let mut a = actor();
            a.snapshot.inputs.push(InputStrip {
                strip_id: "input-1".into(),
                source_id: "recording".into(),
                kind: SourceKind::Recording,
                display_name: "Recording".into(),
                detail: None,
                phase,
                fault: Some(Failure::Capture.error()),
                gain_db: Some(-13.4),
                muted: true,
                colour_slot: 2,
                restored: false,
            });
            a.command(ControlCommand::ReplaceSource {
                strip_id: "input-1".into(),
                source_id: "recording".into(),
            })
            .unwrap();
            assert_eq!(a.snapshot.inputs[0].phase, InputPhase::Opening);
            assert!(a.snapshot.inputs[0].fault.is_none());
            assert_eq!(a.snapshot.inputs[0].gain_db, Some(-13.4));
            assert!(a.snapshot.inputs[0].muted);
        }
    }
    fn add_playback_catalog(a: &mut Actor) {
        for index in 0..4 {
            a.snapshot.playback_devices.push(PlaybackDevice {
                id: format!("wasapi:owned-output-{index}"),
                name: format!("Output {index}"),
                available: true,
                detail: None,
            });
        }
    }
    #[test]
    fn playback_selection_limits_and_discovery_identity_are_enforced() {
        let mut a = actor();
        add_playback_catalog(&mut a);
        for index in 0..3 {
            a.command(ControlCommand::AddPlaybackDestination {
                device_id: format!("wasapi:owned-output-{index}"),
            })
            .unwrap();
        }
        assert!(
            a.command(ControlCommand::AddPlaybackDestination {
                device_id: "wasapi:owned-output-0".into()
            })
            .is_err()
        );
        assert!(
            a.command(ControlCommand::AddPlaybackDestination {
                device_id: "wasapi:owned-output-3".into()
            })
            .is_err()
        );
        a.command(ControlCommand::AddDestination {
            server_id: "1".into(),
            channel_id: "2".into(),
        })
        .unwrap();
        assert_eq!(a.snapshot.destinations.len(), MAX_DESTINATIONS);
        assert!(
            a.command(ControlCommand::AddDestination {
                server_id: "1".into(),
                channel_id: "2".into()
            })
            .is_err()
        );
        let first = a.snapshot.destinations[0].clone();
        a.command(ControlCommand::RemoveDestination {
            destination_id: first.destination_id,
        })
        .unwrap();
        a.command(ControlCommand::AddPlaybackDestination {
            device_id: "wasapi:owned-output-3".into(),
        })
        .unwrap();
        assert_eq!(
            a.snapshot.destinations.last().unwrap().destination_id,
            "playback-5"
        );
        assert!(
            a.snapshot
                .destinations
                .iter()
                .all(|d| d.connection == Connection::Disconnected)
        );
    }
    #[test]
    fn version_three_playback_restores_idle_with_stable_endpoint_and_settings() {
        let directory = std::env::temp_dir().join(format!(
            "echocord-output-preferences-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let mut config = config();
        config.settings_file = Some(directory.join("settings.json"));
        let mut a = actor();
        add_playback_catalog(&mut a);
        a.command(ControlCommand::AddPlaybackDestination {
            device_id: "wasapi:owned-output-0".into(),
        })
        .unwrap();
        a.snapshot.destinations[0].connection = Connection::Connected;
        a.snapshot.destinations[0].session_id = 12;
        a.snapshot.destinations[0].volume_db = Some(-59.5);
        a.snapshot.destinations[0].muted = true;
        save_preferences(&config, &a.snapshot).unwrap();
        let restored = load_preferences(&config).unwrap();
        assert_eq!(restored.version, 4);
        assert_eq!(restored.destinations[0].kind, DestinationKind::Playback);
        assert_eq!(
            restored.destinations[0].connection,
            Connection::Disconnected
        );
        assert_eq!(restored.destinations[0].session_id, 0);
        assert_eq!(
            restored.destinations[0].device_id.as_deref(),
            Some("wasapi:owned-output-0")
        );
        assert_eq!(restored.destinations[0].volume_db, Some(-59.5));
        assert!(restored.destinations[0].muted);
        let mut value = serde_json::to_value(restored).unwrap();
        value["version"] = 2.into();
        value["destinations"] = serde_json::json!([{ "destinationId":"discord-1", "serverId":"1", "channelId":"2", "connection":{"phase":"connected"}, "volumeDb":-12.5, "muted":true }]);
        std::fs::write(
            config.settings_file.as_ref().unwrap(),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let legacy = load_preferences(&config).unwrap();
        assert_eq!(legacy.destinations[0].kind, DestinationKind::Discord);
        assert_eq!(legacy.destinations[0].session_id, 0);
        assert_eq!(legacy.destinations[0].connection, Connection::Disconnected);
        std::fs::remove_dir_all(directory).unwrap();
    }
    fn simulated_room(
        a: &mut Actor,
        id: &str,
        guild: &str,
    ) -> (crate::discord::protocol::RoomKey, u32) {
        use crate::discord::protocol::*;
        a.simulated_discord = true;
        if !a.snapshot.servers.iter().any(|s| s.id == guild) {
            let mut server = a.snapshot.servers[0].clone();
            server.id = guild.into();
            a.snapshot.servers.push(server);
        }
        let d = DestinationStrip {
            destination_id: id.into(),
            kind: DestinationKind::Discord,
            session_id: 0,
            device_id: None,
            display_name: None,
            server_id: guild.into(),
            channel_id: "2".into(),
            connection: Connection::Disconnected,
            volume_db: Some(0.0),
            muted: false,
        };
        a.snapshot.destinations.push(d.clone());
        a.discord.start(&d).unwrap();
        a.discord.gateway_status(1, GatewayStatus::Ready);
        let key = a.discord.rooms[id].key.clone();
        a.discord.gateway_status(
            1,
            GatewayStatus::Credentials {
                key: key.clone(),
                revision: 1,
                secret: VoiceSecret {
                    endpoint: "private-endpoint-canary".into(),
                    session_id: "private-session-canary".into(),
                    token: "private-token-canary".into(),
                    user_id: "3".into(),
                },
            },
        );
        a.discord.advance(true);
        let generation = a.discord.rooms[id].voice.unwrap();
        a.discord
            .voice_status(id, generation, VoiceStatus::Connected);
        a.discord.effects.clear();
        a.publish();
        (key, generation)
    }
    #[test]
    fn controller_publication_never_exports_room_credentials_and_preserves_peers() {
        let mut a = actor();
        simulated_room(&mut a, "discord-1", "1");
        simulated_room(&mut a, "discord-2", "4");
        a.snapshot.destinations.push(DestinationStrip {
            destination_id: "playback-3".into(),
            kind: DestinationKind::Playback,
            device_id: Some("explicit-device".into()),
            display_name: Some("Playback".into()),
            server_id: String::new(),
            channel_id: String::new(),
            session_id: 27,
            connection: Connection::Connected,
            volume_db: Some(-22.5),
            muted: false,
        });
        let playback = a.snapshot.destinations[2].clone();
        let sources = a.snapshot.inputs.clone();
        let peer = a.snapshot.destinations[1].clone();
        a.command(ControlCommand::SetDestinationVolume {
            destination_id: "discord-1".into(),
            volume_db: Some(-37.5),
        })
        .unwrap();
        a.command(ControlCommand::SetDestinationMuted {
            destination_id: "discord-1".into(),
            muted: true,
        })
        .unwrap();
        let key = a.discord.rooms["discord-1"].key.clone();
        a.discord.gateway_status(
            1,
            crate::discord::protocol::GatewayStatus::RoomFailed {
                key,
                code: Failure::Permission,
            },
        );
        a.publish();
        assert_eq!(a.snapshot.inputs, sources);
        assert_eq!(a.snapshot.destinations[1], peer);
        assert_eq!(a.snapshot.destinations[2], playback);
        let published = a.snapshots.borrow().clone();
        let json = serde_json::to_string(&published).unwrap();
        let debug = format!("{published:?}");
        for canary in [
            "private-token-canary",
            "private-session-canary",
            "private-endpoint-canary",
        ] {
            assert!(!json.contains(canary));
            assert!(!debug.contains(canary));
        }
    }
    #[test]
    fn remove_keeps_the_card_and_persists_intent_until_both_receipts() {
        let mut a = actor();
        let (key, generation) = simulated_room(&mut a, "discord-1", "1");
        simulated_room(&mut a, "discord-2", "4");
        let directory =
            std::env::temp_dir().join(format!("echocord-room-removal-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        a.config.settings_file = Some(directory.join("prefs.json"));
        a.command(ControlCommand::RemoveDestination {
            destination_id: "discord-1".into(),
        })
        .unwrap();
        assert!(
            a.snapshot
                .destinations
                .iter()
                .any(|d| d.destination_id == "discord-1")
        );
        assert!(
            !load_preferences(&a.config)
                .unwrap()
                .destinations
                .iter()
                .any(|d| d.destination_id == "discord-1")
        );
        assert!(
            a.command(ControlCommand::DisconnectDestination {
                destination_id: "discord-1".into()
            })
            .is_ok()
        );
        a.discord
            .gateway_status(1, crate::discord::protocol::GatewayStatus::Left { key });
        a.poll_discord();
        assert!(
            a.snapshot
                .destinations
                .iter()
                .any(|d| d.destination_id == "discord-1")
        );
        a.discord.voice_exit("discord-1", generation, true);
        a.poll_discord();
        assert!(
            !a.snapshot
                .destinations
                .iter()
                .any(|d| d.destination_id == "discord-1")
        );
        assert_eq!(a.snapshot.destinations[0].connection, Connection::Connected);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn current_room_is_noop_and_old_target_reservation_retains_actual_channel() {
        for same_server in [false, true] {
            let mut a = actor();
            let (_, generation) = simulated_room(&mut a, "discord-1", "1");
            a.command(ControlCommand::ChangeDestination {
                destination_id: "discord-1".into(),
                server_id: "1".into(),
                channel_id: "2".into(),
            })
            .unwrap();
            assert_eq!(a.discord.rooms["discord-1"].voice, Some(generation));
            assert!(a.discord.effects.is_empty());
            let mut server = a.snapshot.servers[0].clone();
            server.id = if same_server { "1" } else { "4" }.into();
            server.channels[0].id = "5".into();
            if same_server {
                a.snapshot.servers[0].channels.extend(server.channels);
            } else {
                a.snapshot.servers.push(server);
            }
            a.command(ControlCommand::ChangeDestination {
                destination_id: "discord-1".into(),
                server_id: if same_server { "1" } else { "4" }.into(),
                channel_id: "5".into(),
            })
            .unwrap();
            assert_eq!(a.snapshot.server_reservations.len(), 2);
            assert!(
                a.snapshot
                    .server_reservations
                    .iter()
                    .any(|r| r.server_id == "1"
                        && r.channel_id == "2"
                        && r.phase == ReservationPhase::Leaving)
            );
            assert!(
                a.snapshot
                    .server_reservations
                    .iter()
                    .any(|r| r.channel_id == "5" && r.phase == ReservationPhase::Selected)
            );
        }
    }
    #[test]
    fn v4_multiple_rooms_restore_idle_and_older_formats_keep_their_limits() {
        let mut a = actor();
        for (id, guild) in [("discord-1", "1"), ("discord-2", "4"), ("discord-3", "7")] {
            simulated_room(&mut a, id, guild);
        }
        let directory =
            std::env::temp_dir().join(format!("echocord-multi-preferences-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        a.config.settings_file = Some(directory.join("prefs.json"));
        save_preferences(&a.config, &a.snapshot).unwrap();
        let restored = load_preferences(&a.config).unwrap();
        assert_eq!(restored.version, 4);
        assert_eq!(restored.destinations.len(), 3);
        assert!(
            restored
                .destinations
                .iter()
                .all(|d| d.session_id == 0 && d.connection == Connection::Disconnected)
        );
        let path = a.config.settings_file.as_ref().unwrap();
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        json["version"] = 3.into();
        std::fs::write(path, serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(load_preferences(&a.config).is_err());
        json["destinations"] = serde_json::Value::Array(vec![json["destinations"][0].clone()]);
        std::fs::write(path, serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(load_preferences(&a.config).unwrap().destinations.len(), 1);
        json["version"] = 5.into();
        let future = serde_json::to_vec(&json).unwrap();
        std::fs::write(path, &future).unwrap();
        assert!(save_preferences(&a.config, &a.snapshot).is_err());
        assert_eq!(std::fs::read(path).unwrap(), future);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn credentials_cannot_be_replaced_during_any_room_or_gateway_owner() {
        let mut a = actor();
        let (_, generation) = simulated_room(&mut a, "discord-1", "1");
        simulated_room(&mut a, "discord-2", "4");
        assert!(a.configuration_busy());
        a.discord.stop("discord-1", None);
        a.discord.voice_exit("discord-1", generation, true);
        a.poll_discord();
        assert!(a.configuration_busy());
        a.snapshot.destinations.clear();
        assert!(a.configuration_busy());
    }
    #[test]
    fn legacy_destination_selection_updates_the_authoritative_first_card() {
        let mut a = actor();
        let mut server = a.snapshot.servers[0].clone();
        server.id = "4".into();
        server.channels[0].id = "5".into();
        a.snapshot.servers.push(server);
        a.command(ControlCommand::SelectDestination {
            server_id: "1".into(),
            channel_id: Some("2".into()),
        })
        .unwrap();
        assert_eq!(a.snapshot.destinations.len(), 1);
        a.command(ControlCommand::SelectDestination {
            server_id: "4".into(),
            channel_id: Some("5".into()),
        })
        .unwrap();
        assert_eq!(a.snapshot.destinations[0].server_id, "4");
        assert_eq!(a.snapshot.destinations[0].channel_id, "5");
        assert_eq!(a.snapshot.selection.server_id.as_deref(), Some("4"));
        assert!(
            a.command(ControlCommand::SelectDestination {
                server_id: "1".into(),
                channel_id: None
            })
            .is_err()
        );
    }
    #[cfg(windows)]
    #[test]
    fn a_missing_native_gateway_host_contains_every_discord_room() {
        let mut a = actor();
        simulated_room(&mut a, "discord-1", "1");
        simulated_room(&mut a, "discord-2", "4");
        a.simulated_discord = false;
        a.command(ControlCommand::DisconnectDestination {
            destination_id: "discord-1".into(),
        })
        .unwrap();
        assert!(a.discord.rooms.values().all(|r| !r.wanted));
        assert_eq!(a.discord.gateway.phase, GatewayPhase::Error);
    }
    #[test]
    fn legacy_source_alias_tracks_the_first_input_and_clears_when_removed() {
        let mut a = actor();
        a.snapshot.inputs.push(InputStrip {
            strip_id: "input-1".into(),
            source_id: "first".into(),
            kind: SourceKind::Recording,
            display_name: "Recording".into(),
            detail: None,
            phase: InputPhase::Stopped,
            fault: None,
            gain_db: Some(0.0),
            muted: false,
            colour_slot: 0,
            restored: false,
        });
        a.publish();
        assert_eq!(a.snapshot.selection.source_id.as_deref(), Some("first"));
        a.snapshot.inputs[0].source_id = "replacement".into();
        a.publish();
        assert_eq!(
            a.snapshot.selection.source_id.as_deref(),
            Some("replacement")
        );
        a.snapshot.inputs.clear();
        a.publish();
        assert!(a.snapshot.selection.source_id.is_none());
    }
}
