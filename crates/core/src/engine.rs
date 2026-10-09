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
#[cfg(windows)]
const RECOVERY_DEADLINE: Duration = Duration::from_secs(35);
#[cfg(windows)]
const DEVICE_GRACE: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq, Eq)]
enum StopCause {
    Running,
    Requested,
    Failed(Failure),
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
            Command::Output { output: next } if next.volume_percent <= 100 => {
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
    let _ = statuses.send(Status::Connecting).await;
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
    let token = protocol::read_token(&init.token_file).map_err(|_| Failure::Configuration)?;
    let http = serenity::http::Http::new(&token);
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
    let work = async {
        let startup = async {
            loop {
                let is_ready = *ready_rx.borrow_and_update();
                if is_ready {
                    break;
                }
                ready_rx.changed().await.map_err(|_| Failure::Connection)?;
            }
            permissions(&http, guild, channel).await?;
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
        capture_loop(
            &init,
            &http,
            call,
            signals.clone(),
            output,
            recovery_rx,
            stops.clone(),
            work_stop_rx,
            statuses.clone(),
            meters,
        )
        .await
    };
    let mut work = Box::pin(work);
    let result = tokio::select! {
        biased;
        cause = cancelled(&mut stop_rx) => match cause { StopCause::Failed(f) => Err(f), _ => Ok(()) },
        result = &mut work => result,
    };
    drop(work); // Releases CaptureGuard before awaiting any gateway cleanup.
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
            self.recovering.send_modify(|health| {
                health.gateway = event.new != serenity::gateway::ConnectionStage::Connected
            });
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
struct CaptureGuard(crate::audio::CaptureControl);
#[cfg(windows)]
impl Drop for CaptureGuard {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
async fn capture_loop(
    init: &Init,
    http: &serenity::http::Http,
    call: std::sync::Arc<tokio::sync::Mutex<songbird::Call>>,
    signals: Signals,
    mut output: watch::Receiver<Output>,
    mut recovery: watch::Receiver<NetworkRecovery>,
    stops: watch::Sender<StopCause>,
    mut stop_rx: watch::Receiver<StopCause>,
    statuses: mpsc::Sender<Status>,
    meters: watch::Sender<Option<Capture>>,
) -> Result<(), Failure> {
    let mut capture: Option<CaptureGuard> = None;
    let mut missing = RecoveryWindow::new(DEVICE_GRACE);
    let mut recovering = RecoveryWindow::new(RECOVERY_DEADLINE);
    let mut permission_missing = false;
    let mut next_device_try = tokio::time::Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut permission_tick = tokio::time::interval(Duration::from_secs(5));
    permission_tick.tick().await;
    loop {
        tokio::select! {
            biased;
            cause = cancelled(&mut stop_rx) => return match cause { StopCause::Failed(f) => Err(f), _ => Ok(()) },
            changed = output.changed() => {
                if changed.is_err() { return Ok(()); }
                if let Some(capture) = &capture { let output = output.borrow_and_update(); capture.0.set_output(output.volume_percent, output.muted); }
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
            _ = permission_tick.tick() => {
                match permissions(http, signals.guild, signals.channel).await {
                    Ok(()) => {
                        permission_missing = false;
                        let gateway_recovering = recovery.borrow().active();
                        if !gateway_recovering && recovering.reset() && capture.is_some() && !missing.active() { let _ = statuses.send(Status::Connected).await; }
                    },
                    Err(Failure::Connection) => { permission_missing = true; if recovering.begin() { let _ = statuses.send(Status::Recovering).await; } }
                    Err(f) => { stop(&stops, StopCause::Failed(f)); return Err(f); }
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
                    if phase != CapturePhase::Missing { return Err(Failure::Capture); }
                    missing.begin();
                    let _ = statuses.send(Status::DeviceMissing).await;
                }
                if missing.expired() { return Err(Failure::Capture); }
                if capture.is_none() && now >= next_device_try {
                    next_device_try = now + Duration::from_secs(1);
                    match crate::audio::start_capture(&init.source_id) {
                        Ok(session) => {
                            let guard = CaptureGuard(session.control);
                            let output = output.borrow_and_update().clone();
                            guard.0.set_output(output.volume_percent, output.muted);
                            if *stop_rx.borrow() != StopCause::Running { return Ok(()); }
                            let mut call = call.lock().await;
                            if call.current_channel().map(|id| id.0.get()) != Some(signals.channel.get()) { return Err(Failure::Permission); }
                            if *stop_rx.borrow() != StopCause::Running { return Ok(()); }
                            call.play_only_input(songbird::input::RawAdapter::new(session.input, 48000, 2).into());
                            capture = Some(guard); missing.reset();
                            if !recovering.active() { let _ = statuses.send(Status::Connected).await; }
                        }
                        Err(_) if missing.active() => {},
                        Err(_) => return Err(Failure::Capture),
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
                volume_percent: 100,
                muted: false,
            },
        };
        for bytes in [frame(1, Command::Stop), frame(2, Command::Init(init))] {
            let (output, _) = watch::channel(Output {
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
}
