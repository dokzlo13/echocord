//! Parent control surface. This module never creates a gateway client or handles PCM.
use crate::{
    model::*,
    protocol::{self, Command, Failure, Frame, Init, Status, Validator},
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::{BufReader, Read},
    path::{Path, PathBuf},
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
    Dispatch(ControlCommand, oneshot::Sender<Result<(), AppError>>),
    Shutdown(oneshot::Sender<Result<(), AppError>>),
}
enum Event {
    Catalog(u64, Result<Catalog, AppError>),
    Preflight(u32, Result<StartLimit, AppError>),
    Status(u32, Status),
    ProtocolFailed(u32),
    Exited(u32, bool),
    CleanupIncomplete(u32),
}

impl Controller {
    /// Requires a Tokio runtime. Startup performs REST discovery only.
    pub fn spawn(config: ControllerConfig) -> Self {
        let (commands, receiver) = mpsc::channel(32);
        let (snapshots, snapshot_rx) = watch::channel(ControlSnapshot::default());
        let (events, event_rx) = mpsc::channel(32);
        tokio::spawn(Actor::new(config, snapshots, events).run(receiver, event_rx));
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
    selection: Selection,
    output: Option<Output>,
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
    let prefs: Preferences =
        serde_json::from_slice(&bytes).map_err(|_| Failure::Configuration.error())?;
    if prefs
        .output
        .as_ref()
        .is_some_and(|o| o.volume_percent > 100)
    {
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
    let bytes = serde_json::to_vec(&Preferences {
        selection: snapshot.selection.clone(),
        output: Some(snapshot.output.clone()),
    })
    .map_err(|_| Failure::Configuration.error())?;
    static NEXT_FILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let suffix = NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), suffix));
    let result = (|| {
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
    sources: Vec<RecordingSource>,
    servers: Vec<Server>,
    name: String,
}
struct StartLimit {
    remaining: u64,
    reset_after: Duration,
}

struct Actor {
    config: ControllerConfig,
    snapshot: ControlSnapshot,
    snapshots: watch::Sender<ControlSnapshot>,
    events: mpsc::Sender<Event>,
    catalog_epoch: u64,
    child: Option<ChildControl>,
    pending: bool,
    next_launch: Option<Instant>,
    deadline: Option<Instant>,
    next_meter: Instant,
    terminal_error: Option<AppError>,
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
            config,
            snapshot: ControlSnapshot::default(),
            snapshots,
            events,
            catalog_epoch: 0,
            child: None,
            pending: false,
            next_launch: None,
            deadline: None,
            next_meter: Instant::now(),
            terminal_error: None,
            preferences_error: None,
            shutting_down: false,
            shutdown_waiters: Vec::new(),
        }
    }
    fn publish(&mut self) {
        self.snapshot.revision = self.snapshot.revision.wrapping_add(1);
        self.snapshots.send_replace(self.snapshot.clone());
    }
    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Request>,
        mut events: mpsc::Receiver<Event>,
    ) {
        match load_preferences(&self.config) {
            Ok(prefs) => {
                self.snapshot.selection = prefs.selection;
                if let Some(output) = prefs.output {
                    self.snapshot.output = output;
                }
            }
            Err(_) => {
                self.preferences_error = Some(error(
                    ErrorCode::Configuration,
                    "Saved preferences could not be loaded. Select a route again.",
                ))
            }
        }
        self.refresh();
        let mut tick = tokio::time::interval(Duration::from_millis(25));
        loop {
            tokio::select! {
                request = commands.recv(), if !self.shutting_down => match request {
                    Some(Request::Dispatch(command, reply)) => { let result = self.command(command); let _ = reply.send(result); }
                    Some(Request::Shutdown(reply)) => { self.shutdown_waiters.push(reply); self.shutting_down = true; self.disconnect(); }
                    None => { self.shutting_down = true; self.disconnect(); }
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
                    self.poll_deadline();
                }
            }
            if self.shutting_down
                && (self.child.is_none()
                    || matches!(
                        self.snapshot.connection,
                        Connection::CleanupIncomplete { .. }
                    ))
            {
                let result = if self.child.is_some() {
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
    fn refresh(&mut self) {
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
            let result = tokio::time::timeout(Duration::from_secs(30), discover(&config))
                .await
                .unwrap_or_else(|_| Err(Failure::Connection.error()));
            let _ = events.send(Event::Catalog(epoch, result)).await;
        });
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
            ControlCommand::RefreshCatalog | ControlCommand::RecheckConfiguration => {
                if self.snapshot.connection.locks_route() {
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
                        "Select an available recording source.",
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
                if let Some(child) = &self.child {
                    child.output(self.snapshot.output.clone());
                }
            }
            ControlCommand::SetMuted { muted } => {
                self.snapshot.output.muted = muted;
                if let Some(child) = &self.child {
                    child.output(self.snapshot.output.clone());
                }
            }
            ControlCommand::Connect => return self.connect(),
            ControlCommand::Disconnect => {
                self.terminal_error = None;
                self.disconnect();
                return Ok(());
            }
        }
        self.publish();
        save_preferences(&self.config, &self.snapshot)?;
        if self.preferences_error.take().is_some() && self.snapshot.bot.status == BotStatus::Ready {
            self.snapshot.discovery = Discovery::Ready;
            self.publish();
        }
        Ok(())
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
        self.initialization()?;
        self.snapshot.session_id = self
            .snapshot
            .session_id
            .checked_add(1)
            .ok_or_else(unavailable)?;
        self.pending = true;
        self.terminal_error = None;
        self.snapshot.connection = Connection::Connecting;
        self.snapshot.capture.phase = CapturePhase::Stopped;
        self.deadline = Some(Instant::now() + CONNECT_DEADLINE);
        self.publish();
        let generation = self.snapshot.session_id;
        let config = self.config.clone();
        let events = self.events.clone();
        tokio::spawn(async move {
            let result = tokio::time::timeout(Duration::from_secs(15), start_limit(&config))
                .await
                .unwrap_or_else(|_| Err(Failure::Connection.error()));
            let _ = events.send(Event::Preflight(generation, result)).await;
        });
        Ok(())
    }
    fn initialization(&self) -> Result<Init, AppError> {
        let selection = &self.snapshot.selection;
        let source = self
            .snapshot
            .sources
            .iter()
            .find(|s| Some(&s.id) == selection.source_id.as_ref() && s.available)
            .ok_or_else(|| {
                error(
                    ErrorCode::InvalidSelection,
                    "Select an available recording source.",
                )
            })?;
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
            source_id: source.id.clone(),
            server_id: server.id.clone(),
            channel_id: channel.id.clone(),
            output: self.snapshot.output.clone(),
        };
        protocol::validate_init(&init).map_err(|_| Failure::Configuration.error())?;
        Ok(init)
    }
    fn disconnect(&mut self) {
        self.pending = false;
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
    fn event(&mut self, event: Event) {
        match event {
            Event::Catalog(epoch, result) => {
                if epoch != self.catalog_epoch || self.shutting_down {
                    return;
                }
                match result {
                    Ok(catalog) => {
                        self.snapshot.sources = catalog.sources;
                        self.snapshot.servers = catalog.servers;
                        self.snapshot.bot = BotProfile {
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
                match launch(
                    &self.config.executable,
                    generation,
                    init,
                    self.events.clone(),
                ) {
                    Ok(child) => {
                        self.child = Some(child);
                    }
                    Err(_) => self.fail(error(
                        ErrorCode::ConnectionFailed,
                        "The engine could not be started.",
                    )),
                }
            }
            Event::Status(generation, status) => {
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
                    Status::Connecting => {}
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
                        self.snapshot.capture.phase = CapturePhase::Missing;
                        self.snapshot.capture.level_dbfs = None;
                    }
                    Status::Meter { capture } => {
                        self.snapshot.capture = protocol::clean_capture(capture)
                    }
                    Status::Failed { code } => {
                        self.fail(code.error());
                        return;
                    }
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
}

struct Mailbox {
    output: Option<Output>,
    stopped_at: Option<Instant>,
    meter: Option<Capture>,
}
#[derive(Clone)]
struct ChildControl(Arc<(Mutex<Mailbox>, Condvar)>);
impl ChildControl {
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
    fn spawn(executable: &Path, args: &[std::ffi::OsString]) -> std::io::Result<Self> {
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
    executable: &Path,
    generation: u32,
    init: Init,
    events: mpsc::Sender<Event>,
) -> std::io::Result<ChildControl> {
    let mut process = OwnedProcess::spawn(executable, &["--engine-worker".into()])?;
    let mut stdin = process
        .take_stdin()
        .ok_or_else(|| std::io::Error::other("Engine input unavailable"))?;
    let stdout = process
        .take_stdout()
        .ok_or_else(|| std::io::Error::other("Engine output unavailable"))?;
    let mut stderr = process
        .take_stderr()
        .ok_or_else(|| std::io::Error::other("Engine diagnostics unavailable"))?;
    let control = ChildControl(Arc::new((
        Mutex::new(Mailbox {
            output: None,
            stopped_at: None,
            meter: None,
        }),
        Condvar::new(),
    )));
    let writer_control = control.clone();
    let writer_events = events.clone();
    tokio::task::spawn_blocking(move || {
        let result = (|| {
            let mut sequence = 1;
            protocol::write(
                &mut stdin,
                &Frame {
                    version: protocol::VERSION,
                    generation,
                    sequence,
                    body: Command::Init(init),
                },
            )?;
            loop {
                let (lock, wake) = &*writer_control.0;
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
                    &mut stdin,
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
            Ok::<(), protocol::ProtocolError>(())
        })();
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
                Ok(None) => break,
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
    let supervisor_control = control.clone();
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
    Ok(control)
}

#[cfg(not(windows))]
async fn discover(_: &ControllerConfig) -> Result<Catalog, AppError> {
    Err(error(
        ErrorCode::Configuration,
        "Recording and Discord are supported on Windows.",
    ))
}
#[cfg(not(windows))]
async fn start_limit(_: &ControllerConfig) -> Result<StartLimit, AppError> {
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
async fn start_limit(config: &ControllerConfig) -> Result<StartLimit, AppError> {
    let data = http(config)?
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
    let sources = crate::audio::enumerate_sources()?;
    let http = http(config)?;
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
            let channels = http
                .get_channels(guild.id)
                .await
                .map_err(rest_error)?
                .into_iter()
                .filter(|c| c.kind == ChannelType::Voice)
                .map(|c| VoiceChannel {
                    id: c.id.to_string(),
                    name: c.name,
                })
                .collect();
            servers.push(Server {
                id: guild.id.to_string(),
                name: guild.name.clone(),
                channels,
            });
        }
        after = guilds.last().map(|g| g.id);
        if complete {
            return Ok(Catalog {
                sources,
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
            available: true,
            detail: None,
        });
        actor.snapshot.servers.push(Server {
            id: "1".into(),
            name: "Server".into(),
            channels: vec![VoiceChannel {
                id: "2".into(),
                name: "Voice".into(),
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
    fn disconnect_during_join_or_recovery_requires_observed_exit() {
        for phase in [
            Connection::Connecting,
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
        a.snapshot.connection = Connection::Connecting;
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
        tokio::time::advance(Duration::from_secs(5)).await;
        assert!(a.connect().is_ok());
    }
    #[tokio::test(start_paused = true)]
    async fn repeated_recovery_status_cannot_extend_parent_deadline() {
        let mut a = actor();
        a.child = Some(child());
        a.snapshot.connection = Connection::Connected;
        a.snapshot.output = Output {
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
            sources: a.snapshot.sources.clone(),
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
        // WSL can briefly return ETXTBSY immediately after creating an executable.
        let mut attempts = 0;
        let child = loop {
            match launch(&executable, 1, init.clone(), events.clone()) {
                Ok(child) => break child,
                Err(e) if e.raw_os_error() == Some(26) && attempts < 10 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(e) => panic!("synthetic worker launch failed: {:?}", e.kind()),
            }
        };
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
}
