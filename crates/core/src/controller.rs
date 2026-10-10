//! Parent control surface. This module never creates a gateway client or handles PCM.
use crate::{
    model::*,
    protocol::{self, Command, Failure, Frame, Init, Status, Validator},
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::{BufReader, Read},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::Instant,
};

const LAUNCH_COOLDOWN: Duration = Duration::from_secs(5);
const STOP_GRACE: Duration = Duration::from_millis(750);
const EXIT_GRACE: Duration = Duration::from_secs(3);
const CONNECT_DEADLINE: Duration = Duration::from_secs(45);
const RECOVERY_DEADLINE: Duration = Duration::from_secs(40);
const DIAGNOSTIC_GRACE: Duration = Duration::from_secs(1);
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
    Sources(u64, Result<Vec<RecordingSource>, AppError>),
    SourceScanFinished(u64, Option<Result<Vec<RecordingSource>, AppError>>),
    Catalog(u64, Result<Catalog, AppError>),
    Unconfigured(u64),
    Configured(u64, Result<(), AppError>),
    #[cfg(any(windows, test))]
    Stage(u32, ConnectStage),
    Preflight(u32, Result<StartLimit, AppError>),
    LaunchFailed(u32),
    Status(u32, Status),
    ProtocolFailed(u32),
    Exited(u32, bool),
    CleanupIncomplete(u32),
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
    if prefs.version > 2
        || prefs.inputs.len() > MAX_INPUTS
        || prefs.destinations.len() > 1
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
    if prefs.destinations.iter().any(|d| {
        d.destination_id != "discord-1"
            || protocol::discord_id(&d.server_id).is_none()
            || protocol::discord_id(&d.channel_id).is_none()
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
                .is_some_and(|version| version > 2)
        {
            return Err(error(
                ErrorCode::Configuration,
                "These preferences were written by a newer Echocord version and were preserved.",
            ));
        }
    }
    let bytes = serde_json::to_vec(&Preferences {
        version: 2,
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
struct StartLimit {
    remaining: u64,
    reset_after: Duration,
}

struct RetiredSession {
    generation: u32,
    catalog_epoch: u64,
    until: Instant,
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

fn generic_failure(code: &ErrorCode) -> bool {
    matches!(code, ErrorCode::Internal | ErrorCode::ConnectionFailed)
}
fn specific_failure(code: Failure) -> bool {
    matches!(
        code,
        Failure::Configuration
            | Failure::Authentication
            | Failure::Permission
            | Failure::Capture
            | Failure::ApplicationCapture
            | Failure::SourceExited
    )
}

struct Actor {
    #[cfg(windows)]
    audio: Option<crate::mixing::AudioHost>,
    audio_failed: bool,
    next_strip: u64,
    move_pending: bool,
    config: ControllerConfig,
    snapshot: ControlSnapshot,
    snapshots: watch::Sender<ControlSnapshot>,
    events: mpsc::Sender<Event>,
    catalog_epoch: u64,
    source_epoch: u64,
    source_scanning: bool,
    child: Option<ChildControl>,
    pending: bool,
    preflight: Option<tokio::task::JoinHandle<()>>,
    configuration: Option<ConfigurationCheck>,
    next_launch: Option<Instant>,
    deadline: Option<Instant>,
    next_meter: Instant,
    terminal_error: Option<AppError>,
    retired: Option<RetiredSession>,
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
            #[cfg(windows)]
            audio: None,
            audio_failed: false,
            next_strip: 1,
            move_pending: false,
            config,
            snapshot: ControlSnapshot::default(),
            snapshots,
            events,
            catalog_epoch: 0,
            source_epoch: 0,
            source_scanning: false,
            child: None,
            pending: false,
            preflight: None,
            configuration: None,
            next_launch: None,
            deadline: None,
            next_meter: Instant::now(),
            terminal_error: None,
            retired: None,
            preferences_error: None,
            shutting_down: false,
            shutdown_waiters: Vec::new(),
        }
    }
    fn publish(&mut self) {
        if let Some(destination) = self.snapshot.destinations.first_mut() {
            destination.connection = self.snapshot.connection.clone();
        }
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
                if let Some(destination) = self.snapshot.destinations.first() {
                    self.snapshot.selection.server_id = Some(destination.server_id.clone());
                    self.snapshot.selection.channel_id = Some(destination.channel_id.clone());
                }
                if let Some(output) = prefs.output {
                    self.snapshot.output = output;
                }
                if let Some(destination) = self.snapshot.destinations.first() {
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
                    if Instant::now() >= self.next_meter {
                        self.next_meter = Instant::now() + Duration::from_millis(100);
                        if !matches!(self.snapshot.connection, Connection::Disconnecting | Connection::CleanupIncomplete { .. }) {
                            let capture = self.child.as_ref().and_then(ChildControl::take_meter);
                            if let Some(capture) = capture { self.snapshot.capture = protocol::clean_capture(capture); self.publish(); }
                        }
                    }
                    self.poll_audio();
                    if self.move_pending && self.child.is_none() && !self.pending && !self.shutting_down && self.next_launch.is_none_or(|at| Instant::now() >= at) {
                        self.move_pending = false;
                        if let Err(error) = self.connect() { self.fail(error); }
                    }
                    self.poll_deadline();
                }
            }
            if self.shutting_down
                && self.audio_stopped()
                && self.configuration.is_none()
                && (self.child.is_none()
                    || matches!(
                        self.snapshot.connection,
                        Connection::CleanupIncomplete { .. }
                    ))
            {
                let result = if self.child.is_some() || self.audio_cleanup_incomplete() {
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
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            audio.stop();
        }
        self.disconnect();
        if self
            .configuration
            .as_ref()
            .is_some_and(|check| check.control.cancel_validation())
        {
            let configuration = self.configuration.take().expect("cancelled validation");
            configuration.task.abort();
            let _ = configuration.reply.send(Err(unavailable()));
        }
        // Saving is committed under the same mutex as cancellation. A queued or
        // running blocking save must report its actual result before shutdown
        // can succeed; the shell separately owns its three-second Quit fallback.
    }
    fn refresh(&mut self) {
        self.refresh_sources();
        self.retired = None;
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
    fn poll_deadline(&mut self) {
        if self.deadline.is_some_and(|d| Instant::now() >= d)
            && !matches!(
                self.snapshot.connection,
                Connection::Disconnecting | Connection::CleanupIncomplete { .. }
            )
        {
            self.fail(Failure::Connection.error());
        }
    }
    fn command(&mut self, command: ControlCommand) -> Result<(), AppError> {
        match command {
            ControlCommand::RefreshSources => {
                self.refresh_sources();
                return Ok(());
            }
            ControlCommand::RefreshCatalog | ControlCommand::RecheckConfiguration => {
                if self.snapshot.connection.locks_route() || self.configuration.is_some() {
                    return Err(busy());
                }
                self.refresh();
                return Ok(());
            }
            ControlCommand::SelectSource { source_id } => {
                if self.snapshot.connection.locks_route() {
                    return Err(busy());
                }
                if !self
                    .snapshot
                    .sources
                    .iter()
                    .any(|s| s.id == source_id && s.available)
                {
                    return Err(error(
                        ErrorCode::InvalidSelection,
                        "Select an available audio source.",
                    ));
                }
                self.snapshot.selection.source_id = Some(source_id);
            }
            ControlCommand::SelectDestination {
                server_id,
                channel_id,
            } => {
                if self.snapshot.connection.locks_route() {
                    return Err(busy());
                }
                let server = self
                    .snapshot
                    .servers
                    .iter()
                    .find(|s| s.id == server_id)
                    .ok_or_else(|| {
                        error(ErrorCode::InvalidSelection, "Select an available server.")
                    })?;
                if channel_id
                    .as_ref()
                    .is_some_and(|id| !server.channels.iter().any(|c| c.id == *id))
                {
                    return Err(error(
                        ErrorCode::InvalidSelection,
                        "Select an available voice channel.",
                    ));
                }
                self.snapshot.selection.server_id = Some(server_id);
                self.snapshot.selection.channel_id = channel_id;
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
                if let Some(d) = self.snapshot.destinations.first_mut() {
                    d.volume_db = if percent == 0 {
                        None
                    } else {
                        Some(20.0 * (percent as f32 / 100.0).log10())
                    };
                }
                if let Some(child) = &self.child {
                    child.output(self.snapshot.output.clone());
                }
            }
            ControlCommand::SetMuted { muted } => {
                self.snapshot.output.muted = muted;
                if let Some(d) = self.snapshot.destinations.first_mut() {
                    d.muted = muted;
                }
                if let Some(child) = &self.child {
                    child.output(self.snapshot.output.clone());
                }
            }
            ControlCommand::Connect => return self.connect(),
            ControlCommand::Disconnect => {
                self.move_pending = false;
                self.terminal_error = None;
                self.retired = None;
                self.disconnect();
                return Ok(());
            }
            command => return self.mix_command(command),
        }
        self.publish();
        save_preferences(&self.config, &self.snapshot)?;
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
    fn destination_exists(&self, id: &str) -> Result<(), AppError> {
        if self
            .snapshot
            .destinations
            .iter()
            .any(|d| d.destination_id == id)
        {
            Ok(())
        } else {
            Err(error(
                ErrorCode::InvalidSelection,
                "This destination no longer exists.",
            ))
        }
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
                if self.child.is_some() || self.pending {
                    return Err(busy());
                }
                if !self.snapshot.destinations.is_empty() {
                    return Err(error(
                        ErrorCode::InvalidSelection,
                        "One Discord room is supported.",
                    ));
                }
                self.validate_destination(&server_id, &channel_id)?;
                self.snapshot.selection.server_id = Some(server_id.clone());
                self.snapshot.selection.channel_id = Some(channel_id.clone());
                self.snapshot.connection = Connection::Disconnected;
                self.snapshot.destinations.push(DestinationStrip {
                    destination_id: "discord-1".into(),
                    server_id,
                    channel_id,
                    connection: Connection::Disconnected,
                    volume_db: Some(0.0),
                    muted: false,
                });
                self.snapshot.output = Output {
                    volume_linear: Some(1.0),
                    volume_percent: 100,
                    muted: false,
                };
            }
            ControlCommand::RemoveDestination { destination_id } => {
                self.destination_exists(&destination_id)?;
                self.move_pending = false;
                self.disconnect();
                self.snapshot.destinations.clear();
                self.snapshot.selection.server_id = None;
                self.snapshot.selection.channel_id = None;
            }
            ControlCommand::ChangeDestination {
                destination_id,
                server_id,
                channel_id,
            } => {
                self.destination_exists(&destination_id)?;
                self.validate_destination(&server_id, &channel_id)?;
                let reconnect = self.child.is_some()
                    || self.pending
                    || matches!(
                        self.snapshot.connection,
                        Connection::Connected | Connection::Recovering { .. }
                    );
                if reconnect {
                    self.disconnect();
                    self.move_pending = true;
                }
                self.snapshot.selection.server_id = Some(server_id.clone());
                self.snapshot.selection.channel_id = Some(channel_id.clone());
                let destination = &mut self.snapshot.destinations[0];
                destination.server_id = server_id;
                destination.channel_id = channel_id;
            }
            ControlCommand::ConnectDestination { destination_id } => {
                self.destination_exists(&destination_id)?;
                return self.connect();
            }
            ControlCommand::DisconnectDestination { destination_id } => {
                self.destination_exists(&destination_id)?;
                self.move_pending = false;
                self.terminal_error = None;
                self.retired = None;
                self.disconnect();
            }
            ControlCommand::SetDestinationVolume {
                destination_id,
                volume_db,
            } => {
                self.destination_exists(&destination_id)?;
                if !crate::mixing::valid_gain(volume_db) {
                    return Err(error(
                        ErrorCode::InvalidVolume,
                        "Volume must be Off or between −60 and 0 dB.",
                    ));
                }
                self.snapshot.destinations[0].volume_db = volume_db;
                self.snapshot.output.volume_linear = Some(crate::mixing::linear(volume_db));
                self.snapshot.output.volume_percent =
                    (crate::mixing::linear(volume_db) * 100.0).round() as u8;
                if let Some(child) = &self.child {
                    child.output(self.snapshot.output.clone());
                }
            }
            ControlCommand::SetDestinationMuted {
                destination_id,
                muted,
            } => {
                self.destination_exists(&destination_id)?;
                self.snapshot.destinations[0].muted = muted;
                self.snapshot.output.muted = muted;
                if let Some(child) = &self.child {
                    child.output(self.snapshot.output.clone());
                }
            }
            ControlCommand::SetSettings { settings } => {
                self.snapshot.settings = settings;
            }
            _ => return Err(error(ErrorCode::Internal, "Unsupported command.")),
        }
        self.publish();
        save_preferences(&self.config, &self.snapshot)
    }
    fn connect(&mut self) -> Result<(), AppError> {
        if self.child.is_some() || self.pending || self.snapshot.connection.locks_route() {
            return Err(busy());
        }
        if self.next_launch.is_some_and(|d| Instant::now() < d) {
            return Err(error(
                ErrorCode::Busy,
                "Wait before starting another gateway session.",
            ));
        }
        if self.snapshot.bot.status != BotStatus::Ready
            || !matches!(self.snapshot.discovery, Discovery::Ready)
        {
            return Err(Failure::Configuration.error());
        }
        self.sync_audio()?;
        self.initialization()?;
        self.snapshot.session_id = self
            .snapshot
            .session_id
            .checked_add(1)
            .ok_or_else(unavailable)?;
        self.pending = true;
        self.terminal_error = None;
        self.retired = None;
        self.snapshot.connection = Connection::Connecting {
            stage: ConnectStage::Checking,
        };
        self.snapshot.capture.phase = CapturePhase::Stopped;
        self.snapshot.capture.level_dbfs = None;
        self.snapshot.capture.output_level_dbfs = None;
        self.deadline = Some(Instant::now() + CONNECT_DEADLINE);
        self.publish();
        let generation = self.snapshot.session_id;
        let config = self.config.clone();
        let events = self.events.clone();
        self.start_preflight(generation, async move {
            start_limit(&config, generation, &events).await
        });
        Ok(())
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
        self.retired = None;
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
        self.child.is_some()
            || self.pending
            || self.snapshot.connection.locks_route()
            || self.configuration.is_some()
            || self.shutting_down
    }
    fn start_preflight(
        &mut self,
        generation: u32,
        operation: impl std::future::Future<Output = Result<StartLimit, AppError>> + Send + 'static,
    ) {
        let events = self.events.clone();
        self.preflight = Some(tokio::spawn(async move {
            let result = tokio::time::timeout(Duration::from_secs(15), operation)
                .await
                .unwrap_or_else(|_| Err(Failure::Connection.error()));
            let _ = events.send(Event::Preflight(generation, result)).await;
        }));
    }
    fn initialization(&self) -> Result<Init, AppError> {
        let selection = &self.snapshot.selection;
        let server = self
            .snapshot
            .servers
            .iter()
            .find(|s| Some(&s.id) == selection.server_id.as_ref())
            .ok_or_else(|| error(ErrorCode::InvalidSelection, "Select an available server."))?;
        let channel = server
            .channels
            .iter()
            .find(|c| Some(&c.id) == selection.channel_id.as_ref())
            .ok_or_else(|| {
                error(
                    ErrorCode::InvalidSelection,
                    "Select an available voice channel.",
                )
            })?;
        let init = Init {
            token_file: self
                .config
                .token_file
                .clone()
                .ok_or_else(|| Failure::Configuration.error())?,
            source_id: "shared-mix".into(),
            server_id: server.id.clone(),
            channel_id: channel.id.clone(),
            output: self.snapshot.output.clone(),
        };
        protocol::validate_init(&init).map_err(|_| Failure::Configuration.error())?;
        Ok(init)
    }
    fn disconnect(&mut self) {
        self.snapshot.telemetry.gateway_ping_ms = None;
        self.pending = false;
        if let Some(preflight) = self.preflight.take() {
            preflight.abort();
        }
        self.deadline = None;
        if let Some(child) = &self.child {
            child.stop();
            if !matches!(
                self.snapshot.connection,
                Connection::CleanupIncomplete { .. }
            ) {
                self.snapshot.connection = Connection::Disconnecting;
            }
        } else {
            self.snapshot.connection = Connection::Disconnected;
            self.snapshot.capture = Capture {
                phase: CapturePhase::Stopped,
                ..ControlSnapshot::default().capture
            };
        }
        self.publish();
    }
    fn fail(&mut self, error: AppError) {
        self.terminal_error = Some(error.clone());
        self.disconnect();
        if self.child.is_none() {
            self.snapshot.connection = Connection::Error {
                code: error.code,
                message: error.message,
            };
            self.publish();
        }
    }
    fn terminal_status(&mut self, generation: u32, code: Failure) {
        if generation != self.snapshot.session_id || self.shutting_down {
            return;
        }
        let e = code.error();
        if self.child.is_some() {
            if matches!(
                self.snapshot.connection,
                Connection::Disconnecting | Connection::CleanupIncomplete { .. }
            ) {
                // User Stop wins. During failure cleanup only refine a generic cause.
                if specific_failure(code)
                    && self
                        .terminal_error
                        .as_ref()
                        .is_some_and(|current| generic_failure(&current.code))
                {
                    self.terminal_error = Some(e);
                }
            } else {
                self.fail(e);
            }
        } else if specific_failure(code)
            && self.retired.as_ref().is_some_and(|retired| {
                retired.generation == generation
                    && retired.catalog_epoch == self.catalog_epoch
                    && Instant::now() <= retired.until
            })
            && matches!(&self.snapshot.connection, Connection::Error { code, .. } if generic_failure(code))
        {
            self.snapshot.connection = Connection::Error {
                code: e.code,
                message: e.message,
            };
            self.publish();
        }
    }
    fn event(&mut self, event: Event) {
        match event {
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
            #[cfg(any(windows, test))]
            Event::Stage(generation, stage) => {
                if generation == self.snapshot.session_id && self.pending && !self.shutting_down {
                    self.connect_stage(stage);
                }
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
            Event::Preflight(generation, result) => {
                if generation != self.snapshot.session_id || !self.pending || self.shutting_down {
                    return;
                }
                self.pending = false;
                self.preflight = None;
                let limit = match result {
                    Ok(limit) => limit,
                    Err(e) => {
                        self.fail(e);
                        return;
                    }
                };
                if limit.remaining == 0 {
                    self.next_launch =
                        Some(Instant::now() + limit.reset_after.max(LAUNCH_COOLDOWN));
                    self.fail(error(
                        ErrorCode::Busy,
                        "Discord's gateway session allowance is exhausted. Wait before connecting.",
                    ));
                    return;
                }
                let init = match self.initialization() {
                    Ok(init) => init,
                    Err(e) => {
                        self.fail(e);
                        return;
                    }
                };
                self.next_launch = Some(
                    Instant::now()
                        + if limit.remaining == 1 {
                            limit.reset_after.max(LAUNCH_COOLDOWN)
                        } else {
                            LAUNCH_COOLDOWN
                        },
                );
                // Register ownership before process creation can block. Cancel can
                // stop this mailbox even while the OS is still creating the child.
                self.child = Some(launch(
                    self.config.executable.clone(),
                    generation,
                    init,
                    self.events.clone(),
                    #[cfg(windows)]
                    self.audio.as_ref().map(|a| a.bus.clone()),
                ));
            }
            Event::LaunchFailed(generation) => {
                if generation != self.snapshot.session_id || self.child.is_none() {
                    return;
                }
                // Creation failed: there is no process whose exit must be observed.
                self.child = None;
                if matches!(
                    self.snapshot.connection,
                    Connection::Disconnecting | Connection::CleanupIncomplete { .. }
                ) {
                    let terminal = self.terminal_error.take();
                    self.disconnect();
                    if let Some(error) = terminal {
                        self.fail(error);
                    }
                } else {
                    self.fail(error(
                        ErrorCode::ConnectionFailed,
                        "The engine could not be started.",
                    ));
                }
            }
            Event::Status(generation, status) => {
                if let Status::Failed { code } = status {
                    self.terminal_status(generation, code);
                    return;
                }
                if generation != self.snapshot.session_id
                    || self.child.is_none()
                    || matches!(
                        self.snapshot.connection,
                        Connection::Disconnecting | Connection::CleanupIncomplete { .. }
                    )
                {
                    return;
                }
                match status {
                    Status::Connecting { stage } => {
                        self.connect_stage(stage);
                        return;
                    }
                    Status::Connected => {
                        self.snapshot.connection = Connection::Connected;
                        self.deadline = None;
                    }
                    Status::Recovering => {
                        if !matches!(self.snapshot.connection, Connection::Recovering { .. }) {
                            self.deadline = Some(Instant::now() + RECOVERY_DEADLINE);
                        }
                        self.snapshot.connection = Connection::Recovering {
                            attempt: 1,
                            message: "Recovering the existing voice session.".into(),
                        };
                    }
                    Status::DeviceMissing => {
                        if !matches!(self.snapshot.connection, Connection::Recovering { .. }) {
                            self.deadline = Some(Instant::now() + RECOVERY_DEADLINE);
                        }
                        self.snapshot.connection = Connection::Recovering {
                            attempt: 1,
                            message: "Waiting for the selected recording source.".into(),
                        };
                        // The pipe reader coalesces samples separately from
                        // status events. Discard an earlier running sample so
                        // the next meter tick cannot undo the missing state.
                        if let Some(child) = &self.child {
                            child.take_meter();
                        }
                        self.snapshot.capture.phase = CapturePhase::Missing;
                        self.snapshot.capture.level_dbfs = None;
                        self.snapshot.capture.output_level_dbfs = None;
                    }
                    Status::GatewayLatency { milliseconds } => {
                        self.snapshot.telemetry.gateway_ping_ms = milliseconds
                            .filter(|value| value.is_finite() && (0.0..=60_000.0).contains(value));
                    }
                    Status::Meter { capture } => {
                        self.snapshot.capture = protocol::clean_capture(capture)
                    }
                    Status::Failed { .. } => unreachable!("terminal status handled above"),
                    Status::Stopped => {
                        self.fail(Failure::Connection.error());
                        return;
                    }
                }
                self.publish();
            }
            Event::ProtocolFailed(generation) => {
                if generation == self.snapshot.session_id
                    && self.child.is_some()
                    && !matches!(
                        self.snapshot.connection,
                        Connection::Disconnecting | Connection::CleanupIncomplete { .. }
                    )
                {
                    self.fail(Failure::Protocol.error());
                }
            }
            Event::Exited(generation, success) => {
                if generation != self.snapshot.session_id || self.child.is_none() {
                    return;
                }
                let stopping = matches!(
                    self.snapshot.connection,
                    Connection::Disconnecting | Connection::CleanupIncomplete { .. }
                );
                self.retired = if !stopping || self.terminal_error.is_some() {
                    Some(RetiredSession {
                        generation,
                        catalog_epoch: self.catalog_epoch,
                        until: Instant::now() + DIAGNOSTIC_GRACE,
                    })
                } else {
                    None
                };
                self.child = None;
                self.deadline = None;
                self.snapshot.capture = ControlSnapshot::default().capture;
                self.snapshot.connection = if let Some(e) = self.terminal_error.take() {
                    Connection::Error {
                        code: e.code,
                        message: e.message,
                    }
                } else if stopping {
                    Connection::Disconnected
                } else {
                    let e = if success {
                        Failure::Connection.error()
                    } else {
                        error(
                            ErrorCode::ConnectionFailed,
                            "The engine exited unexpectedly.",
                        )
                    };
                    Connection::Error {
                        code: e.code,
                        message: e.message,
                    }
                };
                self.publish();
            }
            Event::CleanupIncomplete(generation) => {
                if generation != self.snapshot.session_id || self.child.is_none() {
                    return;
                }
                self.snapshot.connection = Connection::CleanupIncomplete {
                    message: cleanup_error().message,
                };
                self.deadline = None;
                self.publish();
            }
        }
    }

    fn connect_stage(&mut self, stage: ConnectStage) {
        // A buffered stage cannot revive a cancelled/connected/recovering session
        // or move progress backwards. Stage updates never extend the deadline.
        if let Connection::Connecting { stage: current } = &mut self.snapshot.connection
            && stage > *current
        {
            *current = stage;
            if stage == ConnectStage::Capture {
                self.snapshot.capture.phase = CapturePhase::Opening;
                self.snapshot.capture.level_dbfs = None;
                self.snapshot.capture.output_level_dbfs = None;
            }
            self.publish();
        }
    }
}

struct Mailbox {
    output: Option<Output>,
    stopped_at: Option<Instant>,
    meter: Option<Capture>,
}
#[derive(Clone)]
struct ChildControl(Arc<(Mutex<Mailbox>, Condvar)>);
impl ChildControl {
    fn new() -> Self {
        Self(Arc::new((
            Mutex::new(Mailbox {
                output: None,
                stopped_at: None,
                meter: None,
            }),
            Condvar::new(),
        )))
    }
    fn output(&self, output: Output) {
        let (lock, wake) = &*self.0;
        let mut mail = lock.lock().unwrap_or_else(|e| e.into_inner());
        if mail.stopped_at.is_none() {
            mail.output = Some(output);
            wake.notify_one();
        }
    }
    fn stop(&self) {
        let (lock, wake) = &*self.0;
        let mut mail = lock.lock().unwrap_or_else(|e| e.into_inner());
        if mail.stopped_at.is_none() {
            mail.stopped_at = Some(Instant::now());
            mail.output = None;
            wake.notify_one();
        }
    }
    fn stopped_at(&self) -> Option<Instant> {
        self.0
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopped_at
    }
    fn meter(&self, capture: Capture) {
        self.0.0.lock().unwrap_or_else(|e| e.into_inner()).meter = Some(capture);
    }
    fn take_meter(&self) -> Option<Capture> {
        self.0
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .meter
            .take()
    }
}

#[cfg(windows)]
type OwnedProcess = crate::process::EngineProcess;
#[cfg(not(windows))]
struct OwnedProcess(std::process::Child);
#[cfg(not(windows))]
impl OwnedProcess {
    // Portable supervision implementation for offline regression tests.
    fn spawn(executable: &std::path::Path, args: &[std::ffi::OsString]) -> std::io::Result<Self> {
        use std::process::{Command, Stdio};
        Command::new(executable)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map(Self)
    }
    fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.0.stdin.take()
    }
    fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.0.stdout.take()
    }
    fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.0.stderr.take()
    }
    fn try_wait(&mut self) -> std::io::Result<Option<u32>> {
        self.0
            .try_wait()
            .map(|s| s.map(|s| s.code().unwrap_or(1) as u32))
    }
    fn terminate(&mut self) -> std::io::Result<()> {
        self.0.kill()
    }
}
#[cfg(not(windows))]
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn launch(
    executable: PathBuf,
    generation: u32,
    init: Init,
    events: mpsc::Sender<Event>,
    #[cfg(windows)] bus: Option<Arc<crate::mix_bus::BusHandle>>,
) -> ChildControl {
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsHandle, AsRawHandle};
        let inherited = bus
            .ok_or_else(|| std::io::Error::other("Audio mix unavailable"))
            .and_then(|bus| bus.inheritable());
        let mut init = init;
        if let Ok(handle) = &inherited {
            init.source_id = format!("mix-handle:{}", handle.as_raw_handle() as usize);
        }
        launch_with(generation, init, events, move || {
            let handle = inherited?;
            OwnedProcess::spawn_with_handles(
                &executable,
                &["--engine-worker".into()],
                &[handle.as_handle()],
            )
        })
    }
    #[cfg(not(windows))]
    launch_with(generation, init, events, move || {
        OwnedProcess::spawn(&executable, &["--engine-worker".into()])
    })
}

fn launch_with(
    generation: u32,
    init: Init,
    events: mpsc::Sender<Event>,
    spawn: impl FnOnce() -> std::io::Result<OwnedProcess> + Send + 'static,
) -> ChildControl {
    let control = ChildControl::new();
    let launch_control = control.clone();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || match spawn() {
        Ok(process) => {
            let _runtime = runtime.enter();
            supervise(process, generation, init, events, launch_control);
        }
        Err(_) => {
            let _ = events.blocking_send(Event::LaunchFailed(generation));
        }
    });
    control
}

fn supervise(
    mut process: OwnedProcess,
    generation: u32,
    init: Init,
    events: mpsc::Sender<Event>,
    control: ChildControl,
) {
    {
        let mail = control.0.0.lock().unwrap_or_else(|e| e.into_inner());
        if mail.stopped_at.is_some() {
            // Never initialize a worker created after Cancel. Its OS lifetime is
            // still supervised, using the original 750 ms stop deadline.
            drop(mail);
            supervise_exit(process, generation, events, control);
            return;
        }
    }
    let pipes = (
        process.take_stdin(),
        process.take_stdout(),
        process.take_stderr(),
    );
    let (Some(mut stdin), Some(stdout), Some(mut stderr)) = pipes else {
        control.stop();
        let _ = events.blocking_send(Event::ProtocolFailed(generation));
        supervise_exit(process, generation, events, control);
        return;
    };
    let writer_control = control.clone();
    let writer_events = events.clone();
    tokio::task::spawn_blocking(move || {
        let result = write_commands(&mut stdin, generation, init, &writer_control);
        if result.is_err() {
            let _ = writer_events.blocking_send(Event::ProtocolFailed(generation));
        }
    });
    let reader_events = events.clone();
    let reader_control = control.clone();
    tokio::task::spawn_blocking(move || {
        let mut reader = BufReader::with_capacity(protocol::MAX_LINE_BYTES, stdout);
        let mut validator = Validator::new(generation).expect("nonzero generation");
        loop {
            match protocol::read::<Status>(&mut reader) {
                Ok(Some(frame)) if validator.accept(&frame).is_ok() => {
                    if let Status::Meter { capture } = frame.body {
                        reader_control.meter(capture);
                    } else if reader_events
                        .blocking_send(Event::Status(generation, frame.body))
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(None) => {
                    if reader_control.stopped_at().is_none() {
                        let _ = reader_events.blocking_send(Event::ProtocolFailed(generation));
                    }
                    break;
                }
                _ => {
                    let _ = reader_events.blocking_send(Event::ProtocolFailed(generation));
                    break;
                }
            }
        }
    });
    // stderr is drained concurrently and discarded, including library diagnostics.
    tokio::task::spawn_blocking(move || {
        let mut bytes = [0_u8; 4096];
        loop {
            match stderr.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });
    supervise_exit(process, generation, events, control);
}

fn write_commands(
    writer: &mut impl std::io::Write,
    generation: u32,
    mut init: Init,
    control: &ChildControl,
) -> Result<(), protocol::ProtocolError> {
    let (lock, wake) = &*control.0;
    {
        let mut mail = lock.lock().unwrap_or_else(|e| e.into_inner());
        if mail.stopped_at.is_some() {
            return Ok(());
        }
        if let Some(output) = mail.output.take() {
            init.output = output;
        }
        // Commit Init here, inside the executing writer, serialized with Stop.
        // Release the mailbox before pipe I/O so an already committed write
        // cannot prevent cancellation or the independent exit deadline.
    }
    let mut sequence = 1;
    protocol::write(
        writer,
        &Frame {
            version: protocol::VERSION,
            generation,
            sequence,
            body: Command::Init(init),
        },
    )?;
    loop {
        let mut mail = lock.lock().unwrap_or_else(|e| e.into_inner());
        while mail.output.is_none() && mail.stopped_at.is_none() {
            mail = wake.wait(mail).unwrap_or_else(|e| e.into_inner());
        }
        let stop = mail.stopped_at.is_some();
        let body = if stop {
            Command::Stop
        } else {
            Command::Output {
                output: mail.output.take().expect("output exists"),
            }
        };
        drop(mail);
        sequence += 1;
        protocol::write(
            writer,
            &Frame {
                version: protocol::VERSION,
                generation,
                sequence,
                body,
            },
        )?;
        if stop {
            break;
        }
    }
    Ok(())
}

fn supervise_exit(
    mut process: OwnedProcess,
    generation: u32,
    events: mpsc::Sender<Event>,
    supervisor_control: ChildControl,
) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(25));
        let mut terminated_at = None;
        let mut cleanup_reported = false;
        loop {
            tick.tick().await;
            match process.try_wait() {
                Ok(Some(code)) => {
                    supervisor_control.stop();
                    let _ = events.send(Event::Exited(generation, code == 0)).await;
                    break;
                }
                Err(_) => supervisor_control.stop(),
                Ok(None) => {}
            }
            if supervisor_control
                .stopped_at()
                .is_some_and(|t| Instant::now() >= t + STOP_GRACE)
                && terminated_at.is_none()
            {
                let _ = process.terminate();
                terminated_at = Some(Instant::now());
            }
            if terminated_at.is_some_and(|t| Instant::now() >= t + EXIT_GRACE) && !cleanup_reported
            {
                cleanup_reported = true;
                let _ = events.send(Event::CleanupIncomplete(generation)).await;
            }
            if events.is_closed() {
                let _ = process.terminate();
                break;
            }
        }
    });
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
#[cfg(not(windows))]
async fn start_limit(
    _: &ControllerConfig,
    _: u32,
    _: &mpsc::Sender<Event>,
) -> Result<StartLimit, AppError> {
    Err(Failure::Configuration.error())
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
async fn start_limit(
    config: &ControllerConfig,
    generation: u32,
    events: &mpsc::Sender<Event>,
) -> Result<StartLimit, AppError> {
    let config = config.clone();
    let http = tokio::task::spawn_blocking(move || http(&config))
        .await
        .map_err(|_| unavailable())??;
    let _ = events
        .send(Event::Stage(generation, ConnectStage::Preflight))
        .await;
    let data = http
        .get_bot_gateway()
        .await
        .map_err(rest_error)?
        .session_start_limit;
    Ok(StartLimit {
        remaining: data.remaining,
        reset_after: Duration::from_millis(data.reset_after),
    })
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
    fn child() -> ChildControl {
        ChildControl(Arc::new((
            Mutex::new(Mailbox {
                output: None,
                stopped_at: None,
                meter: None,
            }),
            Condvar::new(),
        )))
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
    async fn responsive_output_and_cancel(controller: &Controller) {
        // A blocked actor makes these acknowledgements time out, even though
        // snapshot() itself can return the previous watch value immediately.
        for command in [
            ControlCommand::SetMuted { muted: true },
            ControlCommand::SetVolume { percent: 37 },
            ControlCommand::Disconnect,
        ] {
            tokio::time::timeout(Duration::from_millis(250), controller.dispatch(command))
                .await
                .expect("long operations must not block command acknowledgements")
                .unwrap();
        }
        assert!(controller.snapshot().output.muted);
        assert_eq!(controller.snapshot().output.volume_percent, 37);
    }
    #[tokio::test]
    async fn delayed_preflight_keeps_commands_responsive_and_cancel_rejects_late_result() {
        let mut a = actor();
        let (events, event_rx) = mpsc::channel(32);
        a.events = events.clone();
        a.pending = true;
        a.snapshot.connection = Connection::Connecting {
            stage: ConnectStage::Checking,
        };
        let (started, start_rx) = oneshot::channel();
        let (mut release, release_rx) = oneshot::channel();
        a.start_preflight(4, async move {
            let _ = started.send(());
            let _ = release_rx.await;
            Ok(StartLimit {
                remaining: 10,
                reset_after: Duration::from_secs(60),
            })
        });
        let controller = running_actor_on(a, event_rx);
        start_rx.await.unwrap();
        events
            .send(Event::Stage(4, ConnectStage::Preflight))
            .await
            .unwrap();
        let mut snapshots = controller.subscribe();
        tokio::time::timeout(Duration::from_secs(1), async {
            while controller.snapshot().connection
                != (Connection::Connecting {
                    stage: ConnectStage::Preflight,
                })
            {
                snapshots.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        responsive_output_and_cancel(&controller).await;
        assert_eq!(controller.snapshot().connection, Connection::Disconnected);
        // Aborting preflight drops its in-flight future, without waiting for REST.
        tokio::time::timeout(Duration::from_millis(250), release.closed())
            .await
            .unwrap();
        assert!(release.send(()).is_err());
        events
            .send(Event::Preflight(
                4,
                Ok(StartLimit {
                    remaining: 10,
                    reset_after: Duration::ZERO,
                }),
            ))
            .await
            .unwrap();
        controller
            .dispatch(ControlCommand::SetMuted { muted: true })
            .await
            .unwrap();
        assert_eq!(controller.snapshot().connection, Connection::Disconnected);
        controller.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn delayed_join_keeps_commands_responsive_and_stays_locked_until_exit() {
        let mut a = actor();
        a.child = Some(child());
        let child = a.child.as_ref().unwrap().clone();
        a.snapshot.connection = Connection::Connecting {
            stage: ConnectStage::Join,
        };
        let (controller, events) = running_actor(a);
        responsive_output_and_cancel(&controller).await;
        assert!(child.stopped_at().is_some());
        assert_eq!(controller.snapshot().connection, Connection::Disconnecting);
        events
            .send(Event::Status(
                4,
                Status::Connecting {
                    stage: ConnectStage::Capture,
                },
            ))
            .await
            .unwrap();
        events
            .send(Event::Status(4, Status::Connected))
            .await
            .unwrap();
        controller
            .dispatch(ControlCommand::SetMuted { muted: true })
            .await
            .unwrap();
        assert_eq!(controller.snapshot().connection, Connection::Disconnecting);
        assert_eq!(
            controller
                .dispatch(ControlCommand::Connect)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        events.send(Event::Exited(4, true)).await.unwrap();
        controller.shutdown().await.unwrap();
        assert_eq!(controller.snapshot().connection, Connection::Disconnected);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn delayed_process_creation_keeps_actor_responsive_and_owns_cancelled_late_child() {
        let mut a = actor();
        a.snapshot.connection = Connection::Connecting {
            stage: ConnectStage::Preflight,
        };
        let (events, mut event_rx) = mpsc::channel(32);
        let (started, start_rx) = oneshot::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let (created, created_rx) = oneshot::channel();
        let init = a.initialization().unwrap();
        let control = launch_with(4, init, events, move || {
            let _ = started.send(());
            release_rx.recv().unwrap(); // Represents a blocked CreateProcessW.
            let process = OwnedProcess::spawn(
                std::path::Path::new("/usr/bin/python3"),
                &[
                    "-c".into(),
                    // Ignore both stdin EOF and Stop; the parent must terminate it.
                    "import time; time.sleep(30)".into(),
                ],
            )?;
            let _ = created.send(());
            Ok(process)
        });
        a.child = Some(control.clone());
        let (controller, actor_events) = running_actor(a);
        start_rx.await.unwrap();
        responsive_output_and_cancel(&controller).await;
        assert_eq!(controller.snapshot().connection, Connection::Disconnecting);
        assert_eq!(
            controller
                .dispatch(ControlCommand::Connect)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        let stopped = control.stopped_at().unwrap();
        // Ensure the original grace has expired *before* the late child exists.
        tokio::time::sleep(STOP_GRACE + Duration::from_millis(25)).await;
        assert_eq!(control.stopped_at(), Some(stopped));
        release.send(()).unwrap();
        created_rx.await.unwrap();
        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                let event = event_rx.recv().await.unwrap();
                let exited = matches!(event, Event::Exited(4, _));
                actor_events.send(event).await.unwrap();
                if exited {
                    break;
                }
            }
        })
        .await
        .expect("late child must use the original stop deadline and report real exit");
        controller.shutdown().await.unwrap();
        assert_eq!(controller.snapshot().connection, Connection::Disconnected);
    }
    #[test]
    fn stages_are_monotonic_generation_scoped_and_do_not_extend_connect_deadline() {
        let mut a = actor();
        a.pending = true;
        a.child = Some(child());
        a.snapshot.connection = Connection::Connecting {
            stage: ConnectStage::Checking,
        };
        a.deadline = Some(Instant::now() + CONNECT_DEADLINE);
        let deadline = a.deadline;
        a.event(Event::Stage(3, ConnectStage::Preflight));
        assert_eq!(
            a.snapshot.connection,
            Connection::Connecting {
                stage: ConnectStage::Checking
            }
        );
        a.event(Event::Stage(4, ConnectStage::Preflight));
        for stage in [
            ConnectStage::Gateway,
            ConnectStage::Join,
            ConnectStage::Capture,
        ] {
            a.event(Event::Status(4, Status::Connecting { stage }));
            assert_eq!(a.snapshot.connection, Connection::Connecting { stage });
            assert_eq!(a.deadline, deadline);
        }
        assert_eq!(a.snapshot.capture.phase, CapturePhase::Opening);
        a.event(Event::Status(
            4,
            Status::Connecting {
                stage: ConnectStage::Gateway,
            },
        ));
        assert_eq!(
            a.snapshot.connection,
            Connection::Connecting {
                stage: ConnectStage::Capture
            }
        );
        a.event(Event::Status(4, Status::Connected));
        a.event(Event::Status(
            4,
            Status::Connecting {
                stage: ConnectStage::Capture,
            },
        ));
        assert_eq!(a.snapshot.connection, Connection::Connected);
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
    async fn configuration_rejects_active_pending_and_cleanup_sessions_without_running_operation() {
        for (connection, pending, has_child) in [
            (Connection::Disconnected, true, false),
            (
                Connection::Connecting {
                    stage: ConnectStage::Join,
                },
                false,
                true,
            ),
            (Connection::Connected, false, true),
            (
                Connection::CleanupIncomplete {
                    message: "Pending exit".into(),
                },
                false,
                true,
            ),
        ] {
            let mut a = actor();
            a.snapshot.connection = connection;
            a.pending = pending;
            if has_child {
                a.child = Some(child());
            }
            let before = a.snapshot.clone();
            let (reply, result) = oneshot::channel();
            a.start_configuration(reply, |_| async {
                panic!("rejected configuration must not validate or save")
            });
            assert_eq!(result.await.unwrap().unwrap_err().code, ErrorCode::Busy);
            assert_eq!(a.snapshot, before);
            assert!(a.configuration.is_none());
        }
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
    fn queued_writer_rechecks_cancellation_and_latest_output_before_init_commit() {
        struct StopAfterWrite {
            bytes: Vec<u8>,
            control: ChildControl,
        }
        impl std::io::Write for StopAfterWrite {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                // Cancellation after commitment must also be able to take the
                // mailbox lock while pipe I/O is in progress.
                self.control.stop();
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        for cancelled in [true, false] {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .max_blocking_threads(1)
                .build()
                .unwrap();
            runtime.block_on(async {
                let (started, started_rx) = oneshot::channel();
                let (release, release_rx) = std::sync::mpsc::channel();
                let blocker = tokio::task::spawn_blocking(move || {
                    let _ = started.send(());
                    let _ = release_rx.recv();
                });
                started_rx.await.unwrap();
                let control = child();
                let writer_control = control.clone();
                let init = actor().initialization().unwrap();
                let writer = tokio::task::spawn_blocking(move || {
                    let mut sink = StopAfterWrite {
                        bytes: Vec::new(),
                        control: writer_control.clone(),
                    };
                    write_commands(&mut sink, 4, init, &writer_control).unwrap();
                    sink.bytes
                });
                // The sole blocking thread is occupied: this writer is queued.
                control.output(Output {
                    volume_linear: None,
                    muted: true,
                    volume_percent: 37,
                });
                if cancelled {
                    control.stop();
                }
                release.send(()).unwrap();
                blocker.await.unwrap();
                let bytes = writer.await.unwrap();
                if cancelled {
                    assert!(
                        bytes.is_empty(),
                        "queued Cancel must suppress Init entirely"
                    );
                } else {
                    let mut reader = std::io::Cursor::new(bytes);
                    let frame = protocol::read::<Command>(&mut reader).unwrap().unwrap();
                    let Command::Init(init) = frame.body else {
                        panic!("Init expected");
                    };
                    assert!(init.output.muted);
                    assert_eq!(init.output.volume_percent, 37);
                    assert!(matches!(
                        protocol::read::<Command>(&mut reader)
                            .unwrap()
                            .unwrap()
                            .body,
                        Command::Stop
                    ));
                    assert!(protocol::read::<Command>(&mut reader).unwrap().is_none());
                }
            });
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn late_worker_initialization_uses_output_changes_made_during_process_creation() {
        let mut a = actor();
        a.snapshot.connection = Connection::Connecting {
            stage: ConnectStage::Preflight,
        };
        let (events, event_rx) = mpsc::channel(32);
        a.events = events.clone();
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let init = a.initialization().unwrap();
        a.child = Some(launch_with(4, init, events, move || {
            let _ = started.send(());
            release_rx.recv().unwrap();
            // The only emitted stage proves the *initial* frame already had the
            // latest output controls; no real capture or network is involved.
            let script = "import sys,json\nf=json.loads(sys.stdin.readline())\nassert f['body']['output']=={'muted':True,'volumePercent':37}\nprint(json.dumps({'version':1,'generation':4,'sequence':1,'body':{'type':'connecting','stage':'join'}}),flush=True)\nfor line in sys.stdin:\n if json.loads(line)['body']['type']=='stop': break\n";
            OwnedProcess::spawn(
                std::path::Path::new("/usr/bin/python3"),
                &["-c".into(), script.into()],
            )
        }));
        let controller = running_actor_on(a, event_rx);
        started_rx.await.unwrap();
        controller
            .dispatch(ControlCommand::SetMuted { muted: true })
            .await
            .unwrap();
        controller
            .dispatch(ControlCommand::SetVolume { percent: 37 })
            .await
            .unwrap();
        release.send(()).unwrap();
        let mut snapshots = controller.subscribe();
        tokio::time::timeout(Duration::from_secs(1), async {
            while controller.snapshot().connection
                != (Connection::Connecting {
                    stage: ConnectStage::Join,
                })
            {
                snapshots.changed().await.unwrap();
            }
        })
        .await
        .expect("late Init must carry the most recent output controls");
        controller.shutdown().await.unwrap();
        assert_eq!(controller.snapshot().connection, Connection::Disconnected);
    }
    #[test]
    fn disconnect_during_join_or_recovery_requires_observed_exit() {
        for phase in [
            Connection::Connecting {
                stage: ConnectStage::Join,
            },
            Connection::Connected,
            Connection::Recovering {
                attempt: 1,
                message: "Recovering".into(),
            },
        ] {
            let mut a = actor();
            a.child = Some(child());
            a.snapshot.connection = phase;
            a.command(ControlCommand::Disconnect).unwrap();
            assert!(matches!(a.snapshot.connection, Connection::Disconnecting));
            a.event(Event::Status(4, Status::Connected));
            a.event(Event::Status(4, Status::Recovering));
            a.event(Event::Status(4, Status::Stopped));
            assert!(matches!(a.snapshot.connection, Connection::Disconnecting));
            assert_eq!(
                a.command(ControlCommand::Connect).unwrap_err().code,
                ErrorCode::Busy
            );
            a.event(Event::Exited(4, false));
            assert!(matches!(a.snapshot.connection, Connection::Disconnected));
            assert_eq!(a.snapshot.capture.phase, CapturePhase::Stopped);
        }
    }
    #[test]
    fn stale_callbacks_never_change_current_session_or_cleanup() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connecting {
            stage: ConnectStage::Join,
        };
        let before = a.snapshot.clone();
        a.event(Event::Status(3, Status::Connected));
        a.event(Event::Exited(3, true));
        a.event(Event::CleanupIncomplete(3));
        a.event(Event::ProtocolFailed(3));
        assert_eq!(a.snapshot, before);
        assert!(a.child.is_some());
        a.pending = false;
        a.event(Event::Preflight(
            4,
            Ok(StartLimit {
                remaining: 1,
                reset_after: Duration::from_secs(60),
            }),
        ));
        assert_eq!(a.snapshot, before);
    }
    #[test]
    fn incomplete_cleanup_locks_connect_and_route_until_exit() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.disconnect();
        a.event(Event::CleanupIncomplete(4));
        assert!(matches!(
            a.snapshot.connection,
            Connection::CleanupIncomplete { .. }
        ));
        assert_eq!(
            a.command(ControlCommand::Connect).unwrap_err().code,
            ErrorCode::Busy
        );
        assert_eq!(
            a.command(ControlCommand::SelectSource {
                source_id: "recording".into()
            })
            .unwrap_err()
            .code,
            ErrorCode::Busy
        );
        a.disconnect();
        assert!(matches!(
            a.snapshot.connection,
            Connection::CleanupIncomplete { .. }
        ));
        a.event(Event::Exited(4, false));
        assert!(matches!(a.snapshot.connection, Connection::Disconnected));
    }
    #[test]
    fn terminal_failure_waits_for_exit_and_never_respawns() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Recovering {
            attempt: 1,
            message: "Recovering".into(),
        };
        a.event(Event::Status(
            4,
            Status::Failed {
                code: Failure::Permission,
            },
        ));
        assert!(matches!(a.snapshot.connection, Connection::Disconnecting));
        a.event(Event::Exited(4, true));
        assert!(matches!(
            a.snapshot.connection,
            Connection::Error {
                code: ErrorCode::PermissionDenied,
                ..
            }
        ));
        assert!(a.child.is_none());
        assert!(!a.pending);
    }
    #[test]
    fn specific_terminal_cause_survives_either_exit_status_order() {
        for (failure, expected) in [
            (Failure::Capture, ErrorCode::CaptureFailed),
            (Failure::Permission, ErrorCode::PermissionDenied),
            (Failure::Authentication, ErrorCode::Authentication),
        ] {
            for exit_first in [false, true] {
                let mut a = actor();
                a.child = Some(child());
                a.snapshot.connection = Connection::Connected;
                if exit_first {
                    a.event(Event::Exited(4, false));
                }
                a.event(Event::Status(4, Status::Failed { code: failure }));
                if !exit_first {
                    assert!(matches!(a.snapshot.connection, Connection::Disconnecting));
                    a.event(Event::Exited(4, false));
                }
                assert!(
                    matches!(&a.snapshot.connection, Connection::Error { code, .. } if *code == expected)
                );
                let before = a.snapshot.clone();
                a.event(Event::Status(4, Status::Connected));
                a.event(Event::Status(4, Status::Recovering));
                a.event(Event::Status(4, Status::Stopped));
                assert_eq!(a.snapshot, before);
                assert_eq!(a.snapshot.bot.status, BotStatus::Ready);
                assert!(a.child.is_none());
                assert!(!a.pending);
            }
        }
    }
    #[test]
    fn device_loss_clears_both_signal_readings_before_another_meter_event() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.snapshot.capture.phase = CapturePhase::Running;
        a.snapshot.capture.level_dbfs = Some(-6.0);
        a.snapshot.capture.output_level_dbfs = Some(-12.0);
        a.child.as_ref().unwrap().meter(a.snapshot.capture.clone());
        a.event(Event::Status(4, Status::DeviceMissing));
        assert_eq!(a.snapshot.capture.phase, CapturePhase::Missing);
        assert_eq!(a.snapshot.capture.level_dbfs, None);
        assert_eq!(a.snapshot.capture.output_level_dbfs, None);
        assert!(a.child.as_ref().unwrap().take_meter().is_none());
    }

    #[test]
    fn buffered_specific_failure_refines_generic_protocol_cleanup() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.event(Event::ProtocolFailed(4));
        a.event(Event::Status(
            4,
            Status::Failed {
                code: Failure::Capture,
            },
        ));
        a.event(Event::Exited(4, false));
        assert!(matches!(
            a.snapshot.connection,
            Connection::Error {
                code: ErrorCode::CaptureFailed,
                ..
            }
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn retired_failure_window_is_bounded_and_cannot_fault_fresh_configuration_or_session() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.event(Event::Exited(4, false));
        tokio::time::advance(DIAGNOSTIC_GRACE + Duration::from_millis(1)).await;
        let before = a.snapshot.clone();
        a.event(Event::Status(
            4,
            Status::Failed {
                code: Failure::Authentication,
            },
        ));
        assert_eq!(a.snapshot, before);
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.event(Event::Exited(4, false));
        a.refresh();
        let catalog = Catalog {
            id: None,
            avatar_url: None,
            servers: a.snapshot.servers.clone(),
            name: "Rechecked Bot".into(),
        };
        a.event(Event::Catalog(a.catalog_epoch, Ok(catalog)));
        let rechecked = a.snapshot.clone();
        a.event(Event::Status(
            4,
            Status::Failed {
                code: Failure::Authentication,
            },
        ));
        assert_eq!(a.snapshot, rechecked);
        a.connect().unwrap();
        let next = a.snapshot.clone();
        a.event(Event::Status(
            4,
            Status::Failed {
                code: Failure::Authentication,
            },
        ));
        a.event(Event::Status(4, Status::Connected));
        assert_eq!(a.snapshot, next);
        assert_eq!(a.snapshot.session_id, 5);
        assert_eq!(a.snapshot.bot.status, BotStatus::Ready);
    }
    #[test]
    fn requested_disconnect_rejects_failure_before_and_after_exit() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.command(ControlCommand::Disconnect).unwrap();
        a.event(Event::Status(
            4,
            Status::Failed {
                code: Failure::Authentication,
            },
        ));
        a.event(Event::Exited(4, false));
        a.event(Event::Status(
            4,
            Status::Failed {
                code: Failure::Permission,
            },
        ));
        assert!(matches!(a.snapshot.connection, Connection::Disconnected));
        assert_eq!(a.snapshot.bot.status, BotStatus::Ready);
    }
    #[tokio::test(start_paused = true)]
    async fn cooldown_and_gateway_limits_survive_worker_exit() {
        let mut a = actor();
        a.pending = true;
        a.event(Event::Preflight(
            4,
            Ok(StartLimit {
                remaining: 0,
                reset_after: Duration::from_secs(60),
            }),
        ));
        assert!(a.child.is_none());
        assert_eq!(a.connect().unwrap_err().code, ErrorCode::Busy);
        tokio::time::advance(Duration::from_secs(59)).await;
        assert_eq!(a.connect().unwrap_err().code, ErrorCode::Busy);
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(a.connect().is_ok());
        a.disconnect();
        a.pending = true;
        a.event(Event::Preflight(
            5,
            Ok(StartLimit {
                remaining: 10,
                reset_after: Duration::from_secs(60),
            }),
        ));
        // Even a failed launch retains a parent-owned five-second cooldown.
        assert_eq!(a.connect().unwrap_err().code, ErrorCode::Busy);
        a.event(Event::LaunchFailed(5));
        tokio::time::advance(Duration::from_secs(5)).await;
        assert!(a.connect().is_ok());
    }
    #[tokio::test(start_paused = true)]
    async fn repeated_recovery_status_cannot_extend_parent_deadline() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.snapshot.output = Output {
            volume_linear: None,
            volume_percent: 40,
            muted: true,
        };
        a.event(Event::Status(4, Status::Recovering));
        tokio::time::advance(RECOVERY_DEADLINE - Duration::from_secs(1)).await;
        a.event(Event::Status(4, Status::Recovering));
        a.poll_deadline();
        assert!(matches!(
            a.snapshot.connection,
            Connection::Recovering { .. }
        ));
        tokio::time::advance(Duration::from_secs(1)).await;
        a.poll_deadline();
        assert!(matches!(a.snapshot.connection, Connection::Disconnecting));
        a.event(Event::Status(4, Status::Connected));
        assert!(matches!(a.snapshot.connection, Connection::Disconnecting));
        a.event(Event::Exited(4, true));
        assert!(matches!(
            a.snapshot.connection,
            Connection::Error {
                code: ErrorCode::ConnectionFailed,
                ..
            }
        ));
        assert!(a.snapshot.output.muted);
        assert_eq!(a.snapshot.output.volume_percent, 40);
    }
    #[test]
    fn output_and_meter_mailboxes_are_bounded_and_stop_wins() {
        let child = child();
        for i in 0..10000 {
            child.output(Output {
                volume_linear: None,
                volume_percent: (i % 101) as u8,
                muted: i % 2 == 0,
            });
            let capture = Capture {
                queued_frames: i,
                ..ControlSnapshot::default().capture
            };
            child.meter(capture);
        }
        assert_eq!(child.take_meter().unwrap().queued_frames, 9999);
        assert!(child.take_meter().is_none());
        child.stop();
        let stopped = child.stopped_at();
        child.output(Output {
            volume_linear: None,
            volume_percent: 100,
            muted: false,
        });
        child.stop();
        assert!(child.0.0.lock().unwrap().output.is_none());
        assert_eq!(stopped, child.stopped_at());
    }
    #[test]
    fn validates_selection_volume_and_keeps_private_paths_out_of_snapshots() {
        let mut a = actor();
        a.snapshot.selection.channel_id = None;
        assert_eq!(
            a.initialization().unwrap_err().code,
            ErrorCode::InvalidSelection
        );
        assert_eq!(
            a.command(ControlCommand::SetVolume { percent: 101 })
                .unwrap_err()
                .code,
            ErrorCode::InvalidVolume
        );
        assert_eq!(
            a.command(ControlCommand::SelectSource {
                source_id: "unknown".into()
            })
            .unwrap_err()
            .code,
            ErrorCode::InvalidSelection
        );
        assert!(
            !serde_json::to_string(&a.snapshot)
                .unwrap()
                .contains("echocord-runtime-test-missing-token")
        );
        assert!(!format!("{:?}", config()).contains("echocord-runtime-test-missing-token"));
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
    #[cfg(unix)]
    #[tokio::test]
    async fn blocked_stdin_cannot_delay_termination_and_exit_receipt() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("echocord-supervision-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("worker.py");
        // No credentials/network. Both pipes flood with stdin completely ignored.
        let frame = Frame {
            version: protocol::VERSION,
            generation: 1,
            sequence: 1,
            body: Status::Meter {
                capture: ControlSnapshot::default().capture,
            },
        };
        let fixture = serde_json::to_string(&frame).unwrap();
        let script = format!(
            "#!/usr/bin/python3\nimport os,time,json\nframe=json.loads({fixture:?})\nfor i in range(128):\n frame['sequence']=i+1\n frame['body']['capture']['queuedFrames']=i\n os.write(1,(json.dumps(frame)+'\\n').encode())\n os.write(2,b'x'*4096)\nwhile True: time.sleep(1)\n"
        );
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (events, mut receiver) = mpsc::channel(32);
        let init = Init {
            token_file: "private".into(),
            source_id: "recording".into(),
            server_id: "1".into(),
            channel_id: "2".into(),
            output: ControlSnapshot::default().output,
        };
        let child = launch_with(1, init, events.clone(), move || {
            OwnedProcess::spawn(
                std::path::Path::new("/usr/bin/python3"),
                &[executable.into_os_string()],
            )
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if child.take_meter().is_some_and(|c| c.queued_frames == 127) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("stdout and stderr must drain concurrently");
        // Repeated writes eventually fill the pipe; the deadline uses no pipe locks.
        let mut blocked = false;
        for _ in 0..1024 {
            child.output(Output {
                volume_linear: None,
                volume_percent: 50,
                muted: true,
            });
            if tokio::time::timeout(Duration::from_millis(50), async {
                while child.0.0.lock().unwrap().output.is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_err()
            {
                blocked = true;
                break;
            }
        }
        child.stop();
        let result =
            tokio::time::timeout(STOP_GRACE + EXIT_GRACE + Duration::from_secs(1), async {
                while let Some(event) = receiver.recv().await {
                    if matches!(event, Event::Exited(1, _)) {
                        return;
                    }
                }
                panic!("exit receipt missing");
            })
            .await;
        std::fs::remove_dir_all(directory).unwrap();
        result.expect("termination must be independent of pipe writes");
        assert!(blocked, "the regression must actually block a stdin write");
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn unsolicited_stdout_eof_stops_live_child_but_does_not_prove_exit() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("echocord-stdout-eof-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("worker.py");
        let frame = Frame {
            version: protocol::VERSION,
            generation: 4,
            sequence: 1,
            body: Status::Connected,
        };
        let fixture = serde_json::to_string(&frame).unwrap();
        let script = format!(
            "#!/usr/bin/python3\nimport os,time,sys\nsys.stdin.readline()\nos.write(1,({fixture:?}+'\\n').encode())\nos.close(1)\nwhile True: time.sleep(1)\n"
        );
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (events, mut receiver) = mpsc::channel(32);
        let mut a = actor();
        let init = a.initialization().unwrap();
        let child = launch_with(4, init, events.clone(), move || {
            OwnedProcess::spawn(
                std::path::Path::new("/usr/bin/python3"),
                &[executable.into_os_string()],
            )
        });
        a.child = Some(child);
        a.snapshot.connection = Connection::Connecting {
            stage: ConnectStage::Gateway,
        };
        let mut connected = false;
        let mut eof = false;
        tokio::time::timeout(STOP_GRACE + EXIT_GRACE + Duration::from_secs(1), async {
            while let Some(event) = receiver.recv().await {
                let exited = matches!(event, Event::Exited(4, _));
                if matches!(event, Event::Status(4, Status::Connected)) {
                    connected = true;
                }
                if matches!(event, Event::ProtocolFailed(4)) {
                    eof = true;
                }
                a.event(event);
                if eof && !exited {
                    assert!(matches!(a.snapshot.connection, Connection::Disconnecting));
                    assert!(a.child.is_some());
                }
                if exited {
                    assert!(connected && eof);
                    assert!(a.child.is_none());
                    assert!(matches!(
                        a.snapshot.connection,
                        Connection::Error {
                            code: ErrorCode::Internal,
                            ..
                        }
                    ));
                    return;
                }
            }
            panic!("exit receipt missing");
        })
        .await
        .expect("stdout EOF must initiate bounded stop of a live worker");
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(not(windows))]
    #[test]
    fn targeted_source_controls_leave_live_destination_owned_and_running() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.snapshot.sources.push(RecordingSource {
            id: "second".into(),
            name: "Second".into(),
            kind: SourceKind::Recording,
            available: true,
            detail: None,
        });
        a.command(ControlCommand::AddSource {
            source_id: "recording".into(),
        })
        .unwrap();
        a.command(ControlCommand::AddSource {
            source_id: "second".into(),
        })
        .unwrap();
        let first = a.snapshot.inputs[0].strip_id.clone();
        a.command(ControlCommand::SetSourceGain {
            strip_id: first.clone(),
            gain_db: Some(-59.5),
        })
        .unwrap();
        a.command(ControlCommand::SetSourceMuted {
            strip_id: first.clone(),
            muted: true,
        })
        .unwrap();
        a.command(ControlCommand::StopCapture {
            strip_id: first.clone(),
        })
        .unwrap();
        a.command(ControlCommand::StartCapture {
            strip_id: first.clone(),
        })
        .unwrap();
        a.command(ControlCommand::RemoveSource { strip_id: first })
            .unwrap();
        assert_eq!(a.snapshot.connection, Connection::Connected);
        assert!(a.child.as_ref().unwrap().stopped_at().is_none());
        assert_eq!(a.snapshot.inputs.len(), 1);
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
    #[test]
    fn gateway_latency_is_measured_sanitized_session_scoped_and_cleared_on_stop() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.event(Event::Status(
            3,
            Status::GatewayLatency {
                milliseconds: Some(12.0),
            },
        ));
        assert_eq!(a.snapshot.telemetry.gateway_ping_ms, None);
        a.event(Event::Status(
            4,
            Status::GatewayLatency {
                milliseconds: Some(-1.0),
            },
        ));
        assert_eq!(a.snapshot.telemetry.gateway_ping_ms, None);
        a.event(Event::Status(
            4,
            Status::GatewayLatency {
                milliseconds: Some(12.5),
            },
        ));
        assert_eq!(a.snapshot.telemetry.gateway_ping_ms, Some(12.5));
        a.disconnect();
        a.event(Event::Status(
            4,
            Status::GatewayLatency {
                milliseconds: Some(15.0),
            },
        ));
        assert_eq!(a.snapshot.telemetry.gateway_ping_ms, None);
    }
    #[test]
    fn selecting_or_starting_the_current_active_source_is_idempotent() {
        for phase in [InputPhase::Opening, InputPhase::Capturing] {
            let mut a = actor();
            a.child = Some(child());
            a.snapshot.connection = Connection::Connected;
            a.snapshot.inputs.push(InputStrip {
                strip_id: "input-1".into(),
                source_id: "recording".into(),
                kind: SourceKind::Recording,
                display_name: "Recording".into(),
                detail: None,
                phase,
                fault: None,
                gain_db: Some(-13.4),
                muted: true,
                colour_slot: 2,
                restored: false,
            });
            let before = a.snapshot.clone();
            a.command(ControlCommand::ReplaceSource {
                strip_id: "input-1".into(),
                source_id: "recording".into(),
            })
            .unwrap();
            assert_eq!(a.snapshot, before);
            a.command(ControlCommand::StartCapture {
                strip_id: "input-1".into(),
            })
            .unwrap();
            assert_eq!(a.snapshot, before);
            assert!(a.child.as_ref().unwrap().stopped_at().is_none());
        }
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
}
