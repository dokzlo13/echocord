//! Internal same-executable worker, entered before the desktop runtime starts.
use crate::{
    model::*,
    protocol::{self, Command, Failure, Frame, Init, Status, Validator},
};
use std::time::Duration;
use tokio::{
    io::{AsyncWriteExt, BufReader},
    sync::{mpsc, watch},
};

const INIT_DEADLINE: Duration = Duration::from_secs(10);
#[cfg(windows)]
const JOIN_DEADLINE: Duration = Duration::from_secs(35);
#[cfg(any(windows, test))]
const RECOVERY_DEADLINE: Duration = Duration::from_secs(35);
#[cfg(any(windows, test))]
const GATEWAY_POLL_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(windows)]
const DEVICE_GRACE: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq, Eq)]
enum StopCause {
    Running,
    Requested,
    Failed(Failure),
}

#[cfg(any(windows, test))]
fn capture_handoff_stop(cause: StopCause) -> Option<Result<(), Failure>> {
    match cause {
        StopCause::Running => None,
        StopCause::Requested => Some(Ok(())),
        StopCause::Failed(failure) => Some(Err(failure)),
    }
}

/// Repeated recovery notifications cannot extend an episode indefinitely.
#[cfg(any(windows, test))]
struct RecoveryWindow {
    since: Option<tokio::time::Instant>,
    limit: Duration,
}
#[cfg(any(windows, test))]
impl RecoveryWindow {
    fn new(limit: Duration) -> Self {
        Self { since: None, limit }
    }
    fn begin(&mut self) -> bool {
        if self.since.is_some() {
            return false;
        }
        self.since = Some(tokio::time::Instant::now());
        true
    }
    fn active(&self) -> bool {
        self.since.is_some()
    }
    fn expired(&self) -> bool {
        self.since
            .is_some_and(|t| tokio::time::Instant::now() >= t + self.limit)
    }
    fn reset(&mut self) -> bool {
        self.since.take().is_some()
    }
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Default)]
struct NetworkRecovery {
    gateway: bool,
    guild: bool,
}
#[cfg(any(windows, test))]
impl NetworkRecovery {
    fn active(self) -> bool {
        self.gateway || self.guild
    }
}

#[cfg(any(windows, test))]
struct MonitorTask(tokio::task::JoinHandle<()>);
#[cfg(any(windows, test))]
impl Drop for MonitorTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(any(windows, test))]
struct PermissionMonitor {
    updates: watch::Receiver<Option<Result<(), Failure>>>,
    _task: MonitorTask,
}
#[cfg(any(windows, test))]
impl PermissionMonitor {
    fn spawn<F, Fut>(
        mut check: F,
        period: Duration,
        stops: watch::Sender<StopCause>,
        mut stop_rx: watch::Receiver<StopCause>,
    ) -> Self
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), Failure>> + Send + 'static,
    {
        let (updates, receiver) = watch::channel(None);
        let task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            tick.tick().await; // Initial permissions were checked before join.
            loop {
                tokio::select! {
                    biased;
                    _ = cancelled(&mut stop_rx) => break,
                    _ = tick.tick() => {},
                }
                let result = tokio::select! {
                    biased;
                    _ = cancelled(&mut stop_rx) => break,
                    result = check() => result,
                };
                if let Err(failure) = result
                    && failure != Failure::Connection
                {
                    stop(&stops, StopCause::Failed(failure));
                }
                updates.send_replace(Some(result));
            }
        });
        Self {
            updates: receiver,
            _task: MonitorTask(task),
        }
    }
}

#[cfg(any(windows, test))]
enum LiveControl {
    Output(Output),
    Permission(Result<(), Failure>),
    Closed,
}

/// Retains the in-flight HTTP task when output wins the selection.
#[cfg(any(windows, test))]
async fn next_live_control(
    output: &mut watch::Receiver<Output>,
    permissions: &mut PermissionMonitor,
) -> LiveControl {
    tokio::select! {
        changed = output.changed() => match changed {
            Ok(()) => LiveControl::Output(output.borrow_and_update().clone()),
            Err(_) => LiveControl::Closed,
        },
        changed = permissions.updates.changed() => match changed {
            Ok(()) => permissions.updates.borrow_and_update().as_ref().copied().map_or(LiveControl::Closed, LiveControl::Permission),
            Err(_) => LiveControl::Permission(Err(Failure::Connection)),
        },
    }
}

/// Public runner snapshots, not REST success or stage callbacks, own this deadline.
#[cfg(any(windows, test))]
fn monitor_gateway<F, Fut>(
    mut snapshot: F,
    health: watch::Sender<NetworkRecovery>,
    stops: watch::Sender<StopCause>,
    mut stop_rx: watch::Receiver<StopCause>,
) -> MonitorTask
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Option<bool>> + Send + 'static,
{
    MonitorTask(tokio::spawn(async move {
        let mut recovery = RecoveryWindow::new(RECOVERY_DEADLINE);
        let mut tick = tokio::time::interval(GATEWAY_POLL_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = cancelled(&mut stop_rx) => break,
                _ = tick.tick() => {},
            }
            let connected = tokio::select! {
                biased;
                _ = cancelled(&mut stop_rx) => break,
                result = tokio::time::timeout(GATEWAY_POLL_INTERVAL, snapshot()) => result.ok().flatten() == Some(true),
            };
            if connected {
                recovery.reset();
            } else {
                recovery.begin();
            }
            health.send_if_modified(|health| {
                if health.gateway == !connected {
                    false
                } else {
                    health.gateway = !connected;
                    true
                }
            });
            if recovery.expired() {
                stop(&stops, StopCause::Failed(Failure::Connection));
                break;
            }
        }
    }))
}

fn stop(sender: &watch::Sender<StopCause>, cause: StopCause) {
    sender.send_if_modified(|current| {
        if *current == StopCause::Running {
            *current = cause;
            true
        } else {
            false
        }
    });
}
#[cfg(any(windows, test))]
async fn cancelled(receiver: &mut watch::Receiver<StopCause>) -> StopCause {
    loop {
        let cause = *receiver.borrow_and_update();
        if cause != StopCause::Running {
            return cause;
        }
        if receiver.changed().await.is_err() {
            return StopCause::Requested;
        }
    }
}

async fn input_loop(
    mut reader: impl tokio::io::AsyncBufRead + Unpin,
    generation: u32,
    initial_sequence: u64,
    output: watch::Sender<Output>,
    stop_sender: watch::Sender<StopCause>,
) {
    let mut validator = Validator::new(generation).expect("validated generation");
    let initial = Frame {
        version: protocol::VERSION,
        generation,
        sequence: initial_sequence,
        body: (),
    };
    let _ = validator.accept(&initial);
    loop {
        let frame = match protocol::read_async::<Command>(&mut reader).await {
            Ok(Some(frame)) if validator.accept(&frame).is_ok() => frame,
            Ok(None) => {
                stop(&stop_sender, StopCause::Requested);
                break;
            }
            _ => {
                stop(&stop_sender, StopCause::Failed(Failure::Protocol));
                break;
            }
        };
        match frame.body {
            Command::Stop => {
                stop(&stop_sender, StopCause::Requested);
                break;
            }
            Command::Output { output: next }
                if next.volume_percent <= 100
                    && next
                        .volume_linear
                        .is_none_or(|gain| gain.is_finite() && (0.0..=1.0).contains(&gain)) =>
            {
                output.send_replace(next);
            }
            _ => {
                stop(&stop_sender, StopCause::Failed(Failure::Protocol));
                break;
            }
        }
    }
}

/// Uses redirected standard handles. No diagnostics contain incoming payloads.
pub async fn run_worker() -> Result<(), AppError> {
    let mut reader = BufReader::with_capacity(protocol::MAX_LINE_BYTES, tokio::io::stdin());
    let frame = tokio::time::timeout(INIT_DEADLINE, protocol::read_async::<Command>(&mut reader))
        .await
        .map_err(|_| Failure::Protocol.error())?
        .map_err(|_| Failure::Protocol.error())?
        .ok_or_else(|| Failure::Protocol.error())?;
    let mut validator = Validator::new(frame.generation).map_err(|_| Failure::Protocol.error())?;
    validator
        .accept(&frame)
        .map_err(|_| Failure::Protocol.error())?;
    let Command::Init(init) = frame.body else {
        return Err(Failure::Protocol.error());
    };
    protocol::validate_init(&init).map_err(|_| Failure::Protocol.error())?;
    let generation = frame.generation;
    let (outputs, output_rx) = watch::channel(init.output.clone());
    let (stops, stop_rx) = watch::channel(StopCause::Running);
    let (statuses, mut status_rx) = mpsc::channel::<Status>(16);
    let (meters, mut meter_rx) = watch::channel(None::<Capture>);
    let input = tokio::spawn(input_loop(
        reader,
        generation,
        frame.sequence,
        outputs,
        stops.clone(),
    ));
    let writer_stops = stops.clone();
    let writer = tokio::spawn(async move {
        let mut writer = tokio::io::stdout();
        let mut sequence = 0_u64;
        loop {
            let body = tokio::select! {
                biased;
                status = status_rx.recv() => match status { Some(status) => status, None => break },
                changed = meter_rx.changed() => {
                    if changed.is_err() { break; }
                    let Some(capture) = meter_rx.borrow_and_update().clone() else { continue; };
                    Status::Meter { capture }
                }
            };
            sequence += 1;
            let frame = Frame {
                version: protocol::VERSION,
                generation,
                sequence,
                body,
            };
            let bytes = protocol::encode(&frame).map_err(|_| Failure::Protocol.error())?;
            if writer.write_all(&bytes).await.is_err() || writer.flush().await.is_err() {
                stop(&writer_stops, StopCause::Failed(Failure::Protocol));
                return Err(Failure::Protocol.error());
            }
        }
        Ok(())
    });
    let result = run_session(
        init,
        output_rx,
        stops.clone(),
        stop_rx,
        statuses.clone(),
        meters.clone(),
    )
    .await;
    stop(&stops, StopCause::Requested);
    input.abort();
    let _ = input.await;
    if let Err(code) = result {
        let _ = statuses.send(Status::Failed { code }).await;
    }
    let _ = statuses.send(Status::Stopped).await;
    drop(statuses);
    drop(meters);
    let mut writer = writer;
    let writer_result = match tokio::time::timeout(Duration::from_secs(1), &mut writer).await {
        Ok(result) => result.map_err(|_| Failure::Protocol.error())?,
        Err(_) => {
            writer.abort();
            Err(Failure::Protocol.error())
        }
    };
    result.map_err(Failure::error).and(writer_result)
}

#[cfg(not(windows))]
async fn run_session(
    _: Init,
    _: watch::Receiver<Output>,
    _: watch::Sender<StopCause>,
    _: watch::Receiver<StopCause>,
    _: mpsc::Sender<Status>,
    _: watch::Sender<Option<Capture>>,
) -> Result<(), Failure> {
    Err(Failure::Configuration)
}

#[cfg(windows)]
async fn run_session(
    init: Init,
    output: watch::Receiver<Output>,
    stops: watch::Sender<StopCause>,
    mut stop_rx: watch::Receiver<StopCause>,
    statuses: mpsc::Sender<Status>,
    meters: watch::Sender<Option<Capture>>,
) -> Result<(), Failure> {
    use serenity::{Client, all::GatewayIntents};
    use songbird::{
        SerenityInit, Songbird,
        driver::retry::{Retry, Strategy},
    };
    use std::{
        num::NonZeroU8,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64},
        },
    };
    let token_file = init.token_file.clone();
    crate::audio::validate_source(&init.source_id).map_err(|_| Failure::SourceExited)?;
    let token = tokio::task::spawn_blocking(move || protocol::read_token(&token_file))
        .await
        .map_err(|_| Failure::Configuration)?
        .map_err(|_| Failure::Configuration)?;
    let http = Arc::new(serenity::http::Http::new(&token));
    let guild = serenity::all::GuildId::new(
        protocol::discord_id(&init.server_id).ok_or(Failure::Protocol)?,
    );
    let channel = serenity::all::ChannelId::new(
        protocol::discord_id(&init.channel_id).ok_or(Failure::Protocol)?,
    );
    let (ready, mut ready_rx) = watch::channel(false);
    let (recovering, recovery_rx) = watch::channel(NetworkRecovery::default());
    let signals = Signals {
        guild,
        channel,
        bot: Arc::new(AtomicU64::new(0)),
        armed: Arc::new(AtomicBool::new(false)),
        ready,
        recovering,
        stops: stops.clone(),
    };
    // Three retries, five seconds per attempt, and bounded resume attempts.
    // No application loop calls join again after an unsuccessful join.
    let manager = Songbird::serenity_from_config(
        songbird::Config::default()
            .driver_timeout(Some(Duration::from_secs(5)))
            .gateway_timeout(Some(Duration::from_secs(10)))
            .driver_retry(Retry {
                strategy: Strategy::Every(Duration::from_secs(1).into()),
                retry_limit: NonZeroU8::new(3),
            }),
    );
    // Permission REST work belongs to preflight, before any gateway is opened.
    // Both this wait and later gateway/join work race the independent Stop input.
    let permissions = tokio::select! {
        biased;
        cause = cancelled(&mut stop_rx) => return match cause { StopCause::Failed(f) => Err(f), _ => Ok(()) },
        result = permissions(&http, guild, channel) => result,
    };
    permissions?;
    let _ = statuses
        .send(Status::Connecting {
            stage: ConnectStage::Gateway,
        })
        .await;
    let builder = Client::builder(
        &token,
        GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES,
    )
    .event_handler(signals.clone())
    .register_songbird_with(manager.clone());
    let mut client = tokio::select! {
        biased;
        cause = cancelled(&mut stop_rx) => return match cause { StopCause::Failed(f) => Err(f), _ => Ok(()) },
        client = tokio::time::timeout(Duration::from_secs(15), builder) => client.map_err(|_| Failure::Connection)?.map_err(classify_rest)?,
    };
    let shard_manager = client.shard_manager.clone();
    let gateway_stops = stops.clone();
    let gateway = tokio::spawn(async move {
        if client.start().await.is_err() {
            stop(&gateway_stops, StopCause::Failed(Failure::Connection));
        } else {
            stop(&gateway_stops, StopCause::Requested);
        }
    });
    let work_stop_rx = stop_rx.clone();
    let capture_cleanup_failed = Arc::new(AtomicBool::new(false));
    let work = async {
        let startup = async {
            loop {
                let is_ready = *ready_rx.borrow_and_update();
                if is_ready {
                    break;
                }
                ready_rx.changed().await.map_err(|_| Failure::Connection)?;
            }
            signals
                .armed
                .store(true, std::sync::atomic::Ordering::Release);
            let call = manager.get_or_insert(guild);
            {
                let mut call = call.lock().await;
                call.add_global_event(
                    songbird::Event::Core(songbird::CoreEvent::DriverDisconnect),
                    signals.clone(),
                );
            }
            let _ = statuses
                .send(Status::Connecting {
                    stage: ConnectStage::Join,
                })
                .await;
            manager
                .join(guild, channel)
                .await
                .map_err(|_| Failure::Connection)?;
            Ok::<_, Failure>(call)
        };
        let call = tokio::time::timeout(JOIN_DEADLINE, startup)
            .await
            .map_err(|_| Failure::Connection)??;
        if *work_stop_rx.borrow() != StopCause::Running {
            return Ok(());
        }
        let _ = statuses
            .send(Status::Connecting {
                stage: ConnectStage::Capture,
            })
            .await;
        capture_loop(
            &init,
            http.clone(),
            shard_manager.clone(),
            call,
            signals.clone(),
            output,
            recovery_rx,
            stops.clone(),
            work_stop_rx,
            statuses.clone(),
            meters,
            capture_cleanup_failed.clone(),
        )
        .await
    };
    let mut work = Box::pin(work);
    let mut result = tokio::select! {
        biased;
        cause = cancelled(&mut stop_rx) => match cause { StopCause::Failed(f) => Err(f), _ => Ok(()) },
        result = &mut work => result,
    };
    drop(work); // Releases CaptureGuard before awaiting any gateway cleanup.
    if capture_cleanup_failed.load(std::sync::atomic::Ordering::Acquire) {
        result = Err(Failure::Capture);
    }
    // Cancel intent before cleanup. The parent still requires observed process exit.
    stop(&stops, StopCause::Requested);
    signals
        .armed
        .store(false, std::sync::atomic::Ordering::Release);
    let _ = tokio::time::timeout(Duration::from_secs(2), manager.remove(guild)).await;
    let _ = tokio::time::timeout(Duration::from_secs(1), shard_manager.shutdown_all()).await;
    gateway.abort();
    let _ = gateway.await;
    result
}

#[cfg(windows)]
async fn permissions(
    http: &serenity::http::Http,
    guild: serenity::all::GuildId,
    channel: serenity::all::ChannelId,
) -> Result<(), Failure> {
    use serenity::all::{Channel, ChannelType, Permissions};
    let work = async {
        let user = http.get_current_user().await.map_err(classify_rest)?;
        let guild_data = guild.to_partial_guild(http).await.map_err(classify_rest)?;
        let member = guild_data
            .member(http, user.id)
            .await
            .map_err(classify_rest)?;
        let Channel::Guild(channel_data) = channel.to_channel(http).await.map_err(classify_rest)?
        else {
            return Err(Failure::Permission);
        };
        if channel_data.guild_id != guild || channel_data.kind != ChannelType::Voice {
            return Err(Failure::Permission);
        }
        let permissions = guild_data.user_permissions_in(&channel_data, &member);
        if !permissions
            .contains(Permissions::VIEW_CHANNEL | Permissions::CONNECT | Permissions::SPEAK)
        {
            return Err(Failure::Permission);
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .unwrap_or(Err(Failure::Connection))
}
#[cfg(windows)]
fn classify_rest(e: serenity::Error) -> Failure {
    if matches!(&e, serenity::Error::Http(e) if e.status_code().is_some_and(|s| s.as_u16() == 404))
    {
        return Failure::Permission;
    }
    match crate::controller::rest_error(e).code {
        ErrorCode::Authentication => Failure::Authentication,
        ErrorCode::PermissionDenied => Failure::Permission,
        _ => Failure::Connection,
    }
}

#[cfg(windows)]
#[derive(Clone)]
struct Signals {
    guild: serenity::all::GuildId,
    channel: serenity::all::ChannelId,
    bot: std::sync::Arc<std::sync::atomic::AtomicU64>,
    armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ready: watch::Sender<bool>,
    recovering: watch::Sender<NetworkRecovery>,
    stops: watch::Sender<StopCause>,
}
#[cfg(windows)]
#[serenity::async_trait]
impl serenity::client::EventHandler for Signals {
    async fn ready(&self, _: serenity::client::Context, ready: serenity::all::Ready) {
        self.bot
            .store(ready.user.id.get(), std::sync::atomic::Ordering::Release);
        self.ready.send_replace(true);
    }
    async fn voice_state_update(
        &self,
        _: serenity::client::Context,
        _: Option<serenity::all::VoiceState>,
        new: serenity::all::VoiceState,
    ) {
        if self.armed.load(std::sync::atomic::Ordering::Acquire)
            && new.user_id.get() == self.bot.load(std::sync::atomic::Ordering::Acquire)
            && (new.guild_id != Some(self.guild)
                || new.channel_id != Some(self.channel)
                || new.mute
                || new.suppress)
        {
            stop(&self.stops, StopCause::Failed(Failure::Permission));
        }
    }
    async fn channel_delete(
        &self,
        _: serenity::client::Context,
        channel: serenity::all::GuildChannel,
        _: Option<Vec<serenity::all::Message>>,
    ) {
        if channel.id == self.channel {
            stop(&self.stops, StopCause::Failed(Failure::Permission));
        }
    }
    async fn guild_delete(
        &self,
        _: serenity::client::Context,
        guild: serenity::all::UnavailableGuild,
        _: Option<serenity::all::Guild>,
    ) {
        if guild.id == self.guild {
            if guild.unavailable {
                self.recovering.send_modify(|health| health.guild = true);
            } else {
                stop(&self.stops, StopCause::Failed(Failure::Permission));
            }
        }
    }
    async fn guild_create(
        &self,
        _: serenity::client::Context,
        guild: serenity::all::Guild,
        _: Option<bool>,
    ) {
        if guild.id == self.guild {
            self.recovering.send_modify(|health| health.guild = false);
        }
    }
    async fn shard_stage_update(
        &self,
        _: serenity::client::Context,
        event: serenity::gateway::ShardStageUpdateEvent,
    ) {
        if *self.ready.borrow() {
            // Disconnection is an early hint; only a current runner snapshot clears it.
            if event.new != serenity::gateway::ConnectionStage::Connected {
                self.recovering.send_modify(|health| health.gateway = true);
            }
        }
    }
}
#[cfg(windows)]
#[serenity::async_trait]
impl songbird::EventHandler for Signals {
    async fn act(&self, event: &songbird::EventContext<'_>) -> Option<songbird::Event> {
        if let songbird::EventContext::DriverDisconnect(_) = event {
            stop(&self.stops, StopCause::Failed(Failure::Connection));
        }
        None
    }
}

#[cfg(windows)]
struct CaptureGuard(
    crate::audio::CaptureControl,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
);
#[cfg(windows)]
impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if !self.0.stop() {
            self.1.store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
async fn capture_loop(
    init: &Init,
    http: std::sync::Arc<serenity::http::Http>,
    shard_manager: std::sync::Arc<serenity::gateway::ShardManager>,
    call: std::sync::Arc<tokio::sync::Mutex<songbird::Call>>,
    signals: Signals,
    mut output: watch::Receiver<Output>,
    mut recovery: watch::Receiver<NetworkRecovery>,
    stops: watch::Sender<StopCause>,
    mut stop_rx: watch::Receiver<StopCause>,
    statuses: mpsc::Sender<Status>,
    meters: watch::Sender<Option<Capture>>,
    cleanup_failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), Failure> {
    let mut capture: Option<CaptureGuard> = None;
    let mut missing = RecoveryWindow::new(DEVICE_GRACE);
    let mut recovering = RecoveryWindow::new(RECOVERY_DEADLINE);
    let mut permission_missing = false;
    let mut next_device_try = tokio::time::Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let guild = signals.guild;
    let channel = signals.channel;
    let mut permission_monitor = PermissionMonitor::spawn(
        move || {
            let http = http.clone();
            async move { permissions(&http, guild, channel).await }
        },
        Duration::from_secs(5),
        stops.clone(),
        stop_rx.clone(),
    );
    let _gateway_monitor = monitor_gateway(
        move || {
            let shard_manager = shard_manager.clone();
            async move {
                shard_manager
                    .runners
                    .lock()
                    .await
                    .get(&serenity::all::ShardId(0))
                    .map(|runner| runner.stage == serenity::gateway::ConnectionStage::Connected)
            }
        },
        signals.recovering.clone(),
        stops.clone(),
        stop_rx.clone(),
    );
    loop {
        tokio::select! {
            biased;
            cause = cancelled(&mut stop_rx) => return match cause { StopCause::Failed(f) => Err(f), _ => Ok(()) },
            control = next_live_control(&mut output, &mut permission_monitor) => {
                match control {
                    LiveControl::Closed => return Ok(()),
                    LiveControl::Output(output) => {
                        if let Some(capture) = &capture { capture.0.set_linear_output(output.volume_linear.unwrap_or(output.volume_percent as f32 / 100.0), output.muted); }
                    }
                    LiveControl::Permission(Ok(())) => {
                        permission_missing = false;
                        let gateway_recovering = recovery.borrow().active();
                        if !gateway_recovering && recovering.reset() && capture.is_some() && !missing.active() { let _ = statuses.send(Status::Connected).await; }
                    }
                    LiveControl::Permission(Err(Failure::Connection)) => { permission_missing = true; if recovering.begin() { let _ = statuses.send(Status::Recovering).await; } }
                    LiveControl::Permission(Err(f)) => { stop(&stops, StopCause::Failed(f)); return Err(f); }
                }
            }
            changed = recovery.changed() => {
                if changed.is_err() { return Err(Failure::Connection); }
                let gateway_recovering = recovery.borrow_and_update().active();
                if gateway_recovering {
                    if recovering.begin() { let _ = statuses.send(Status::Recovering).await; }
                } else if !permission_missing {
                    recovering.reset();
                    if capture.is_some() && !missing.active() { let _ = statuses.send(Status::Connected).await; }
                }
            }
            _ = tick.tick() => {
                let now = tokio::time::Instant::now();
                if recovering.expired() { return Err(Failure::Connection); }
                if capture.as_ref().is_some_and(|c| matches!(c.0.snapshot().phase, CapturePhase::Missing | CapturePhase::Error | CapturePhase::Stopped)) {
                    let phase = capture.as_ref().expect("capture exists").0.snapshot().phase;
                    capture = None;
                    call.lock().await.stop();
                    meters.send_replace(Some(Capture { phase: phase.clone(), ..ControlSnapshot::default().capture }));
                    if phase != CapturePhase::Missing {
                        if crate::audio::application::is_application_id(&init.source_id) {
                            return Err(if crate::audio::validate_source(&init.source_id).is_err() { Failure::SourceExited } else { Failure::ApplicationCapture });
                        }
                        return Err(Failure::Capture);
                    }
                    missing.begin();
                    let _ = statuses.send(Status::DeviceMissing).await;
                }
                if missing.expired() { return Err(Failure::Capture); }
                if capture.is_none() && now >= next_device_try {
                    next_device_try = now + Duration::from_secs(1);
                    match crate::audio::start_capture_with_cancel(&init.source_id, &|| *stop_rx.borrow() != StopCause::Running) {
                        Ok(session) => {
                            let guard = CaptureGuard(session.control, cleanup_failed.clone());
                            let output = output.borrow_and_update().clone();
                            guard.0.set_linear_output(output.volume_linear.unwrap_or(output.volume_percent as f32 / 100.0), output.muted);
                            if let Some(result) = capture_handoff_stop(*stop_rx.borrow()) { return result; }
                            let mut call = call.lock().await;
                            if call.current_channel().map(|id| id.0.get()) != Some(signals.channel.get()) { return Err(Failure::Permission); }
                            if let Some(result) = capture_handoff_stop(*stop_rx.borrow()) { return result; }
                            call.play_only_input(songbird::input::RawAdapter::new(session.input, 48000, 2).into());
                            capture = Some(guard); missing.reset();
                            if !recovering.active() { let _ = statuses.send(Status::Connected).await; }
                        }
                        Err(_) => {
                            if let Some(result) = capture_handoff_stop(*stop_rx.borrow()) { return result; }
                            if !missing.active() {
                                return Err(if crate::audio::application::is_application_id(&init.source_id) { Failure::ApplicationCapture } else { Failure::Capture });
                            }
                        },
                    }
                }
                if let Some(capture) = &capture { meters.send_replace(Some(protocol::clean_capture(capture.0.snapshot()))); }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn capture_open_handoff_preserves_failure_after_outer_cancel_polled_pending() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        for cause in [
            StopCause::Failed(Failure::Connection),
            StopCause::Failed(Failure::Protocol),
            StopCause::Requested,
        ] {
            let (stops, receiver) = watch::channel(StopCause::Running);
            let mut outer = receiver.clone();
            let mut cancellation = Box::pin(cancelled(&mut outer));
            let mut context = Context::from_waker(Waker::noop());
            assert!(matches!(
                cancellation.as_mut().poll(&mut context),
                Poll::Pending
            ));
            // The blocking open finishes in this same outer poll after stop arrived.
            // Its handoff guard must preserve failure before outer cancellation is repolled.
            stops.send_replace(cause);
            let result = capture_handoff_stop(*receiver.borrow()).unwrap();
            match cause {
                StopCause::Failed(failure) => assert_eq!(result, Err(failure)),
                StopCause::Requested => assert_eq!(result, Ok(())),
                StopCause::Running => unreachable!(),
            }
        }
    }
    fn frame(sequence: u64, body: Command) -> Vec<u8> {
        protocol::encode(&Frame {
            version: protocol::VERSION,
            generation: 1,
            sequence,
            body,
        })
        .unwrap()
    }
    #[tokio::test]
    async fn eof_and_stop_are_terminal_during_join_or_recovery() {
        for bytes in [Vec::new(), frame(2, Command::Stop)] {
            let (output, _) = watch::channel(Output {
                volume_linear: None,
                volume_percent: 100,
                muted: false,
            });
            let (stops, mut receiver) = watch::channel(StopCause::Running);
            input_loop(bytes.as_slice(), 1, 1, output, stops).await;
            assert!(matches!(
                cancelled(&mut receiver).await,
                StopCause::Requested
            ));
        }
    }
    #[tokio::test]
    async fn stale_replayed_and_duplicate_initialization_fail_closed() {
        let init = Init {
            token_file: "private".into(),
            source_id: "source".into(),
            server_id: "1".into(),
            channel_id: "2".into(),
            output: Output {
                volume_linear: None,
                volume_percent: 100,
                muted: false,
            },
        };
        for bytes in [frame(1, Command::Stop), frame(2, Command::Init(init))] {
            let (output, _) = watch::channel(Output {
                volume_linear: None,
                volume_percent: 100,
                muted: false,
            });
            let (stops, mut receiver) = watch::channel(StopCause::Running);
            input_loop(bytes.as_slice(), 1, 1, output, stops).await;
            assert!(matches!(
                cancelled(&mut receiver).await,
                StopCause::Failed(Failure::Protocol)
            ));
        }
    }
    #[test]
    fn cancellation_cannot_be_reversed_by_late_failure_or_success() {
        let (sender, receiver) = watch::channel(StopCause::Running);
        stop(&sender, StopCause::Requested);
        stop(&sender, StopCause::Failed(Failure::Connection));
        assert!(matches!(*receiver.borrow(), StopCause::Requested));
    }
    #[tokio::test(start_paused = true)]
    async fn recovery_and_device_grace_have_absolute_deadlines() {
        for limit in [Duration::from_secs(30), Duration::from_secs(35)] {
            let mut window = RecoveryWindow::new(limit);
            assert!(!window.expired());
            assert!(window.begin());
            tokio::time::advance(limit - Duration::from_secs(1)).await;
            assert!(!window.begin());
            assert!(!window.expired());
            tokio::time::advance(Duration::from_secs(1)).await;
            assert!(window.expired());
            assert!(!window.begin());
            assert!(window.reset());
            assert!(!window.active());
            assert!(window.begin());
            assert!(!window.expired());
        }
    }
    #[test]
    fn recovery_requires_both_gateway_and_guild_to_return() {
        let mut health = NetworkRecovery {
            gateway: true,
            guild: true,
        };
        health.gateway = false;
        assert!(health.active());
        health.guild = false;
        assert!(!health.active());
        health.gateway = true;
        assert!(health.active());
    }
    #[tokio::test(start_paused = true)]
    async fn stalled_permission_request_does_not_delay_applied_mute_or_volume() {
        let (stops, stop_rx) = watch::channel(StopCause::Running);
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let (response, response_rx) = tokio::sync::oneshot::channel();
        let mut request = Some((started, response_rx));
        let mut permissions = PermissionMonitor::spawn(
            move || {
                let (started, response) = request.take().expect("only one in-flight request");
                async move {
                    let _ = started.send(());
                    response.await.unwrap_or(Err(Failure::Connection))
                }
            },
            Duration::from_secs(5),
            stops,
            stop_rx,
        );
        let initial = Output {
            volume_linear: None,
            volume_percent: 100,
            muted: false,
        };
        let (outputs, mut output_rx) = watch::channel(initial.clone());
        let mut applied = initial;
        assert_eq!(applied, *outputs.borrow());
        started_rx.await.unwrap();
        for next in [
            Output {
                volume_linear: None,
                volume_percent: 40,
                muted: true,
            },
            Output {
                volume_linear: None,
                volume_percent: 0,
                muted: false,
            },
        ] {
            outputs.send_replace(next.clone());
            let before = tokio::time::Instant::now();
            match tokio::time::timeout(
                Duration::from_millis(50),
                next_live_control(&mut output_rx, &mut permissions),
            )
            .await
            .unwrap()
            {
                LiveControl::Output(output) => applied = output,
                _ => panic!("output must apply while the request is stalled"),
            }
            assert_eq!(applied, next);
            assert!(tokio::time::Instant::now() < before + Duration::from_millis(50));
            assert!(permissions.updates.borrow().is_none());
        }
        response.send(Ok(())).unwrap();
        assert!(matches!(
            next_live_control(&mut output_rx, &mut permissions).await,
            LiveControl::Permission(Ok(()))
        ));
        drop(outputs);
        assert!(matches!(
            next_live_control(&mut output_rx, &mut permissions).await,
            LiveControl::Closed
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn missing_runner_exhausts_gateway_deadline_despite_healthy_rest() {
        for runner in [None, Some(false)] {
            let (stops, stop_rx) = watch::channel(StopCause::Running);
            let (health, health_rx) = watch::channel(NetworkRecovery::default());
            let _gateway = monitor_gateway(
                move || async move { runner },
                health,
                stops.clone(),
                stop_rx.clone(),
            );
            let permissions = PermissionMonitor::spawn(
                || async { Ok(()) },
                Duration::from_secs(5),
                stops,
                stop_rx.clone(),
            );
            tokio::task::yield_now().await;
            assert!(health_rx.borrow().gateway);
            for _ in 0..6 {
                tokio::time::advance(Duration::from_secs(5)).await;
                tokio::task::yield_now().await;
                assert_eq!(*permissions.updates.borrow(), Some(Ok(())));
                assert!(matches!(*stop_rx.borrow(), StopCause::Running));
                assert!(health_rx.borrow().gateway);
            }
            tokio::time::advance(RECOVERY_DEADLINE - Duration::from_secs(30)).await;
            tokio::task::yield_now().await;
            assert!(matches!(
                *stop_rx.borrow(),
                StopCause::Failed(Failure::Connection)
            ));
        }
    }
    #[tokio::test(start_paused = true)]
    async fn permission_monitor_cancels_stalled_http_when_stop_arrives() {
        let (stops, stop_rx) = watch::channel(StopCause::Running);
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let (response, response_rx) = tokio::sync::oneshot::channel::<Result<(), Failure>>();
        let mut request = Some((started, response_rx));
        let monitor = PermissionMonitor::spawn(
            move || {
                let (started, response) = request.take().unwrap();
                async move {
                    let _ = started.send(());
                    response.await.unwrap_or(Err(Failure::Permission))
                }
            },
            Duration::from_secs(5),
            stops.clone(),
            stop_rx.clone(),
        );
        started_rx.await.unwrap();
        stop(&stops, StopCause::Requested);
        tokio::task::yield_now().await;
        assert!(response.send(Err(Failure::Authentication)).is_err());
        assert!(matches!(*stop_rx.borrow(), StopCause::Requested));
        assert!(monitor.updates.borrow().is_none());
    }
}
