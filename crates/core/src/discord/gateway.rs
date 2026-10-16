//! One credential-bearing gateway; ordered voice hooks are the correlation boundary.
use super::{
    correlation::{Correlator, Phase, ServerAction, StateAction},
    protocol::*,
    recovery::{PermissionSchedule, RefreshRetry, RetryReadiness},
};
use crate::protocol::{self, Failure, Frame, Validator};
use futures::channel::mpsc::UnboundedSender;
use serenity::{
    all::{GatewayIntents, GuildId, UserId, VoiceState},
    gateway::{ShardRunnerMessage, VoiceGatewayManager},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    sync::{Mutex, mpsc, watch},
    time::Instant,
};

type Call = Arc<Mutex<songbird::Call>>;
struct Timing {
    started: Instant,
    leaving: Option<Instant>,
    failed_since: Option<Instant>,
    permissions: PermissionSchedule,
    verification: RefreshRetry,
}
impl Timing {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            leaving: None,
            failed_since: None,
            permissions: PermissionSchedule::default(),
            verification: RefreshRetry::default(),
        }
    }
}
struct Inner {
    correlation: Correlator,
    jobs: super::work::TaskBudget,
    calls: BTreeMap<String, Call>,
    times: BTreeMap<String, Timing>,
    sender: Option<UnboundedSender<ShardRunnerMessage>>,
    registered: bool,
    connected: bool,
    user: UserId,
    http: Arc<serenity::http::Http>,
    statuses: mpsc::Sender<GatewayStatus>,
    stops: watch::Sender<bool>,
}
impl Inner {
    fn sender_healthy(&mut self) -> bool {
        if self.registered && self.sender.as_ref().is_none_or(|sender| sender.is_closed()) {
            self.fatal(Failure::Connection);
            false
        } else {
            true
        }
    }
    fn emit(&self, status: GatewayStatus) {
        if self.statuses.try_send(status).is_err() {
            self.stops.send_replace(true);
        }
    }
    fn fatal(&mut self, code: Failure) {
        self.sender = None;
        self.connected = false;
        let keys = self
            .correlation
            .slots
            .values()
            .map(|s| s.key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            self.correlation.cancel(&key);
            self.jobs.cancel(&key);
        }
        self.emit(GatewayStatus::Failed { code });
        self.stops.send_replace(true);
    }
    fn transmit(&mut self, key: &RoomKey, join: bool) -> bool {
        if !self.sender_healthy() {
            return false;
        }
        let eligible = self.correlation.slot(key).is_some_and(|s| {
            if join {
                s.wanted && s.phase == Phase::Joining && !s.issued
            } else {
                s.issued && !s.wanted
            }
        });
        if !eligible || !self.connected {
            return false;
        }
        let channel = join.then_some(key.channel_id.as_str());
        let body = serde_json::json!({"op":4,"d":{"guild_id":key.server_id,"channel_id":channel,"self_deaf":true,"self_mute":false}});
        if self.sender.as_ref().is_none_or(|sender| {
            sender
                .unbounded_send(ShardRunnerMessage::Message(body.to_string().into()))
                .is_err()
        }) {
            self.fatal(Failure::Connection);
            return false;
        }
        if join {
            self.correlation.mark_join_sent(key)
        } else {
            self.correlation.leave_sent(key);
            true
        }
    }
    fn cancel(&mut self, key: &RoomKey, failed: Option<Failure>) {
        if self.correlation.slot(key).is_none() {
            return;
        }
        if let Some(code) = failed {
            self.emit(GatewayStatus::RoomFailed {
                key: key.clone(),
                code,
            });
        }
        self.jobs.cancel(key);
        let left = self.correlation.cancel(key);
        if let Some(time) = self.times.get_mut(&key.server_id) {
            time.leaving.get_or_insert_with(Instant::now);
        }
        if left {
            self.emit(GatewayStatus::Left { key: key.clone() });
        } else if !self.transmit(key, false) {
            self.emit(GatewayStatus::CleanupIncomplete { key: key.clone() });
        }
    }
}
struct Gated {
    inner: Weak<Mutex<Inner>>,
    key: RoomKey,
}
#[serenity::async_trait]
impl songbird::shards::VoiceUpdate for Gated {
    async fn update_voice_state(
        &self,
        guild: songbird::id::GuildId,
        channel: Option<songbird::id::ChannelId>,
        _: bool,
        _: bool,
    ) -> songbird::error::JoinResult<()> {
        let inner = self
            .inner
            .upgrade()
            .ok_or(songbird::error::JoinError::NoSender)?;
        let mut inner = inner.lock().await;
        if guild.to_string() != self.key.server_id
            || channel.map(|id| id.to_string()).as_deref() != Some(self.key.channel_id.as_str())
            || !inner.transmit(&self.key, true)
        {
            return Err(songbird::error::JoinError::NoSender);
        }
        inner.emit(GatewayStatus::Joining {
            key: self.key.clone(),
        });
        Ok(())
    }
}
struct Hooks(Arc<Mutex<Inner>>);
#[serenity::async_trait]
impl VoiceGatewayManager for Hooks {
    async fn initialise(&self, count: u32, user: UserId) {
        let mut inner = self.0.lock().await;
        if count != 1 || inner.user != user {
            inner.fatal(Failure::Authentication);
        }
    }
    async fn register_shard(&self, id: u32, sender: UnboundedSender<ShardRunnerMessage>) {
        let mut inner = self.0.lock().await;
        if id != 0 || inner.registered {
            inner.fatal(Failure::Connection);
            return;
        }
        inner.registered = true;
        inner.connected = true;
        inner.sender = Some(sender);
        inner.emit(GatewayStatus::Ready);
    }
    async fn deregister_shard(&self, _: u32) {
        self.0.lock().await.fatal(Failure::Connection);
    }
    async fn state_update(&self, guild: GuildId, state: &VoiceState) {
        let guild = guild.to_string();
        let (key, call, forward) = {
            let mut inner = self.0.lock().await;
            if state.user_id != inner.user {
                return;
            }
            let Some(key) = inner.correlation.slots.get(&guild).map(|s| s.key.clone()) else {
                return;
            };
            let action = inner.correlation.state(
                &guild,
                state.channel_id.map(|id| id.to_string()).as_deref(),
                &state.session_id,
                !state.mute && !state.suppress && !state.self_mute,
            );
            match action {
                StateAction::Failed(code) => inner.cancel(&key, Some(code)),
                StateAction::LeaveAgain => {
                    if !inner.transmit(&key, false) {
                        inner.emit(GatewayStatus::CleanupIncomplete { key: key.clone() });
                    }
                }
                StateAction::Left => inner.emit(GatewayStatus::Left { key: key.clone() }),
                _ => {}
            }
            (
                key,
                inner.calls.get(&guild).cloned(),
                action == StateAction::Forward,
            )
        };
        if forward {
            if let Some(call) = call {
                call.lock()
                    .await
                    .update_state(state.session_id.clone(), state.channel_id);
            }
            verify(self.0.clone(), key).await;
        }
    }
    async fn server_update(&self, guild: GuildId, endpoint: &Option<String>, token: &str) {
        let guild = guild.to_string();
        let (key, call, forward) = {
            let mut inner = self.0.lock().await;
            let action = inner.correlation.server(&guild, endpoint.as_deref(), token);
            if inner.correlation.exhausted {
                inner.fatal(Failure::Protocol);
                return;
            }
            let Some(key) = inner.correlation.slots.get(&guild).map(|s| s.key.clone()) else {
                return;
            };
            match action {
                ServerAction::Failed(code) => inner.cancel(&key, Some(code)),
                ServerAction::Left => inner.emit(GatewayStatus::Left { key: key.clone() }),
                ServerAction::Reconfigure => {
                    inner.emit(GatewayStatus::Reconfiguring { key: key.clone() })
                }
                _ => {}
            }
            (
                key,
                inner.calls.get(&guild).cloned(),
                matches!(action, ServerAction::Forward | ServerAction::Reconfigure),
            )
        };
        if forward {
            if let (Some(call), Some(endpoint)) = (call, endpoint) {
                call.lock()
                    .await
                    .update_server(endpoint.clone(), token.into());
            }
            verify(self.0.clone(), key).await;
        }
    }
}
async fn membership(
    http: &serenity::http::Http,
    key: &RoomKey,
    user: UserId,
) -> Result<Option<VoiceState>, Failure> {
    let guild = GuildId::new(protocol::discord_id(&key.server_id).ok_or(Failure::Protocol)?);
    tokio::time::timeout(Duration::from_secs(5), async {
        match http.get_user_voice_state(guild, user).await {
            Ok(state) => Ok(Some(state)),
            Err(serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(
                ref response,
            ))) if response.error.code == 10065 => Ok(None),
            Err(error) => Err(super::permissions::classify_rest(error)),
        }
    })
    .await
    .unwrap_or(Err(Failure::Connection))
}
enum WorkKind {
    Initial,
    Verification,
    Permission,
}
async fn spawn_room(
    inner: Arc<Mutex<Inner>>,
    key: RoomKey,
    kind: WorkKind,
    work: impl std::future::Future<Output = ()> + Send + 'static,
) {
    let mut i = inner.lock().await;
    if !i.correlation.slot(&key).is_some_and(|slot| slot.wanted) {
        return;
    }
    if !i.jobs.spawn(key.clone(), work) {
        if matches!(kind, WorkKind::Initial) {
            i.cancel(&key, Some(Failure::Connection));
        } else {
            if matches!(kind, WorkKind::Verification)
                && let Some(slot) = i.correlation.slot_mut(&key)
            {
                slot.verifying = None;
            }
            if let Some(time) = i.times.get_mut(&key.server_id) {
                if matches!(kind, WorkKind::Permission) {
                    time.permissions.complete(Instant::now());
                }
                let since = *time.failed_since.get_or_insert_with(Instant::now);
                if since.elapsed() > Duration::from_secs(30) {
                    i.cancel(&key, Some(Failure::Connection));
                    return;
                }
            }
            i.emit(GatewayStatus::Health {
                key,
                recovering: true,
            });
        }
    }
}
async fn verify(inner: Arc<Mutex<Inner>>, key: RoomKey) {
    let request = {
        let mut i = inner.lock().await;
        let user = i.user;
        if !i.connected {
            return;
        }
        if let Some(time) = i.times.get(&key.server_id) {
            match time.verification.readiness(Instant::now()) {
                RetryReadiness::Waiting => return,
                RetryReadiness::Expired => {
                    i.cancel(&key, Some(Failure::Connection));
                    return;
                }
                RetryReadiness::Ready => {}
            }
        }
        let Some((counter, secret)) = i.correlation.begin_verification(&key, &user.to_string())
        else {
            return;
        };
        (counter, secret, i.http.clone(), user)
    };
    spawn_room(
        inner.clone(),
        key.clone(),
        WorkKind::Verification,
        async move {
            let (counter, secret, http, user) = request;
            let result = membership(&http, &key, user).await;
            let mut i = inner.lock().await;
            i.finish_verification(key, counter, secret, result);
        },
    )
    .await;
}
impl Inner {
    fn finish_permissions(&mut self, key: RoomKey, result: Result<(), Failure>) {
        if !self.correlation.slot(&key).is_some_and(|slot| slot.wanted) {
            return;
        }
        let Some(time) = self.times.get_mut(&key.server_id) else {
            return;
        };
        time.permissions.complete(Instant::now());
        match result {
            Ok(()) => {
                time.failed_since = None;
                self.emit(GatewayStatus::Health {
                    key,
                    recovering: false,
                });
            }
            Err(Failure::Connection) => {
                let since = *time.failed_since.get_or_insert_with(Instant::now);
                if since.elapsed() > Duration::from_secs(30) {
                    self.cancel(&key, Some(Failure::Connection));
                } else {
                    self.emit(GatewayStatus::Health {
                        key,
                        recovering: true,
                    });
                }
            }
            Err(code) => self.cancel(&key, Some(code)),
        }
    }
    fn finish_verification(
        &mut self,
        key: RoomKey,
        counter: u64,
        secret: VoiceSecret,
        result: Result<Option<VoiceState>, Failure>,
    ) {
        let Some(slot) = self.correlation.slot(&key) else {
            return;
        };
        if !slot.wanted || slot.verifying != Some(counter) {
            return;
        }
        if !self.connected || slot.counter != counter {
            self.correlation
                .slot_mut(&key)
                .expect("matched slot")
                .verifying = None;
            return;
        }
        let refreshing = slot.revision > 0;
        match result {
            Ok(Some(state))
                if state.channel_id.map(|c| c.to_string()).as_deref() == Some(&key.channel_id)
                    && state.session_id == secret.session_id
                    && !state.mute
                    && !state.suppress
                    && !state.self_mute =>
            {
                if let Some(revision) = self.correlation.verify(
                    &key,
                    counter,
                    &secret,
                    Some(&key.channel_id),
                    Some(&state.session_id),
                    true,
                ) {
                    if let Some(time) = self.times.get_mut(&key.server_id) {
                        time.verification.succeeded();
                    }
                    self.emit(GatewayStatus::Credentials {
                        key,
                        revision,
                        secret,
                    });
                }
            }
            Err(Failure::Connection) if refreshing => {
                self.correlation
                    .slot_mut(&key)
                    .expect("matched slot")
                    .verifying = None;
                if self
                    .times
                    .get_mut(&key.server_id)
                    .is_some_and(|time| time.verification.failed(Instant::now()))
                {
                    self.emit(GatewayStatus::Health {
                        key,
                        recovering: true,
                    });
                } else {
                    self.cancel(&key, Some(Failure::Connection));
                }
            }
            Ok(_) => self.cancel(&key, Some(Failure::Permission)),
            Err(code) => self.cancel(&key, Some(code)),
        }
    }
}
async fn join(inner: Arc<Mutex<Inner>>, key: RoomKey) {
    let request = {
        let mut i = inner.lock().await;
        if i.sender.is_none() || i.correlation.reserve(key.clone()).is_err() {
            i.emit(GatewayStatus::RoomFailed {
                key: key.clone(),
                code: Failure::Protocol,
            });
            i.emit(GatewayStatus::Left { key });
            return;
        }
        i.times.insert(key.server_id.clone(), Timing::new());
        (i.http.clone(), i.user)
    };
    spawn_room(inner.clone(), key.clone(), WorkKind::Initial, async move {
        let (http, user) = request;
        let guild = GuildId::new(protocol::discord_id(&key.server_id).expect("validated"));
        let channel = serenity::all::ChannelId::new(
            protocol::discord_id(&key.channel_id).expect("validated"),
        );
        // A bounded retry handles an ordered observation racing the REST response.
        for _ in 0..3 {
            let counter = {
                let i = inner.lock().await;
                let Some(slot) = i
                    .correlation
                    .slot(&key)
                    .filter(|s| s.wanted && s.phase == Phase::Checking)
                else {
                    return;
                };
                slot.counter
            };
            let result = async {
                super::permissions::permissions(&http, user, guild, channel).await?;
                membership(&http, &key, user).await
            }
            .await;
            let call = {
                let mut i = inner.lock().await;
                let Some(slot) = i.correlation.slot(&key).filter(|s| s.wanted) else {
                    return;
                };
                if slot.counter != counter {
                    continue;
                }
                match result {
                    Ok(None) => {}
                    Ok(Some(ref state)) if state.channel_id.is_none() => {}
                    Ok(Some(_)) => {
                        i.cancel(&key, Some(Failure::Permission));
                        return;
                    }
                    Err(code) => {
                        i.cancel(&key, Some(code));
                        return;
                    }
                }
                if !i.correlation.preflight(&key, counter, true) {
                    return;
                }
                let gate = Gated {
                    inner: Arc::downgrade(&inner),
                    key: key.clone(),
                };
                let call = Arc::new(Mutex::new(songbird::Call::from_config(
                    guild,
                    songbird::shards::Shard::Generic(Arc::new(gate)),
                    user,
                    songbird::Config::default().gateway_timeout(Some(Duration::from_secs(30))),
                )));
                i.calls.insert(key.server_id.clone(), call.clone());
                call
            };
            let future = call.lock().await.join_gateway(channel).await;
            let success = match future {
                Ok(future) => tokio::time::timeout(Duration::from_secs(35), future)
                    .await
                    .is_ok_and(|result| result.is_ok()),
                Err(_) => false,
            };
            if !success {
                let mut i = inner.lock().await;
                if i.correlation
                    .slot(&key)
                    .is_some_and(|s| s.wanted && s.revision == 0)
                {
                    i.cancel(&key, Some(Failure::Connection));
                }
            }
            return;
        }
        inner.lock().await.cancel(&key, Some(Failure::Connection));
    })
    .await;
}
async fn maintenance(inner: Arc<Mutex<Inner>>) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
        let keys = {
            let mut i = inner.lock().await;
            if *i.stops.borrow() {
                return;
            }
            let now = Instant::now();
            let keys = i
                .correlation
                .slots
                .values()
                .map(|s| s.key.clone())
                .collect::<Vec<_>>();
            for key in &keys {
                let phase = i.correlation.slot(key).map(|s| s.phase);
                let time = i.times.get(&key.server_id).expect("room timing");
                if time
                    .leaving
                    .is_some_and(|at| now.duration_since(at) > Duration::from_secs(12))
                    && phase != Some(Phase::Left)
                {
                    if let Some(slot) = i.correlation.slot_mut(key) {
                        slot.phase = Phase::Blocked;
                    }
                    i.emit(GatewayStatus::CleanupIncomplete { key: key.clone() });
                } else if matches!(phase, Some(Phase::Checking | Phase::Joining))
                    && now.duration_since(time.started) > Duration::from_secs(40)
                {
                    i.cancel(key, Some(Failure::Connection));
                }
            }
            keys
        };
        for key in keys {
            verify(inner.clone(), key.clone()).await;
            let request = {
                let mut i = inner.lock().await;
                if !i
                    .correlation
                    .slot(&key)
                    .is_some_and(|s| s.wanted && s.issued)
                {
                    continue;
                }
                let Some(time) = i.times.get_mut(&key.server_id) else {
                    continue;
                };
                if !time.permissions.begin(Instant::now()) {
                    continue;
                }
                (i.http.clone(), i.user, key.clone())
            };
            let inner = inner.clone();
            spawn_room(
                inner.clone(),
                key.clone(),
                WorkKind::Permission,
                async move {
                    let (http, user, key) = request;
                    let result = super::permissions::permissions(
                        &http,
                        user,
                        GuildId::new(protocol::discord_id(&key.server_id).expect("valid")),
                        serenity::all::ChannelId::new(
                            protocol::discord_id(&key.channel_id).expect("valid"),
                        ),
                    )
                    .await;
                    // Apply the result immediately; only the next request waits.
                    inner.lock().await.finish_permissions(key, result);
                },
            )
            .await;
        }
    }
}
pub async fn run_worker() -> Result<(), Failure> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let frame = tokio::time::timeout(
        Duration::from_secs(10),
        protocol::read_async::<GatewayCommand>(&mut reader),
    )
    .await
    .map_err(|_| Failure::Protocol)?
    .map_err(|_| Failure::Protocol)?
    .ok_or(Failure::Protocol)?;
    let generation = frame.generation;
    let mut validator = Validator::new(generation).map_err(|_| Failure::Protocol)?;
    validator.accept(&frame).map_err(|_| Failure::Protocol)?;
    let GatewayCommand::Init { token_file } = frame.body else {
        return Err(Failure::Protocol);
    };
    let (stops, mut stop_rx) = watch::channel(false);
    let (commands, mut command_rx) = mpsc::channel(16);
    let input_stop = stops.clone();
    tokio::spawn(async move {
        while let Ok(Some(frame)) = protocol::read_async::<GatewayCommand>(&mut reader).await {
            if validator.accept(&frame).is_err() {
                break;
            }
            if matches!(
                frame.body,
                GatewayCommand::Stop | GatewayCommand::Init { .. }
            ) {
                break;
            }
            if commands.send(frame.body).await.is_err() {
                break;
            }
        }
        input_stop.send_replace(true);
    });
    let (statuses, mut status_rx) = mpsc::channel(64);
    let output_stop = stops.clone();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        let mut sequence = 0;
        while let Some(body) = status_rx.recv().await {
            sequence += 1;
            let Ok(bytes) = protocol::encode(&Frame {
                version: protocol::VERSION,
                generation,
                sequence,
                body,
            }) else {
                break;
            };
            if stdout.write_all(&bytes).await.is_err() || stdout.flush().await.is_err() {
                break;
            }
        }
        output_stop.send_replace(true);
    });
    let setup = async {
        let token = tokio::task::spawn_blocking(move || crate::storage::load_token(&token_file))
            .await
            .map_err(|_| Failure::Configuration)?
            .map_err(|_| Failure::Configuration)?;
        let http = Arc::new(serenity::http::Http::new(&token));
        let user = http
            .get_current_user()
            .await
            .map_err(super::permissions::classify_rest)?;
        if !user.bot {
            return Err(Failure::Authentication);
        }
        let allowance = http
            .get_bot_gateway()
            .await
            .map_err(super::permissions::classify_rest)?
            .session_start_limit;
        let _ = statuses
            .send(GatewayStatus::Allowance {
                remaining: allowance.remaining,
                reset_after_ms: allowance.reset_after,
            })
            .await;
        if allowance.remaining == 0 {
            return Err(Failure::Connection);
        }
        let inner = Arc::new(Mutex::new(Inner {
            correlation: Correlator::default(),
            jobs: crate::discord::work::TaskBudget::default(),
            calls: BTreeMap::new(),
            times: BTreeMap::new(),
            sender: None,
            registered: false,
            connected: false,
            user: user.id,
            http,
            statuses: statuses.clone(),
            stops: stops.clone(),
        }));
        let client = serenity::Client::builder(
            &token,
            GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES,
        )
        .voice_manager(Hooks(inner.clone()))
        .await
        .map_err(|_| Failure::Connection)?;
        Ok((client, inner))
    };
    let result = tokio::select! {biased;_=stop_rx.changed()=>Ok(None),result=tokio::time::timeout(Duration::from_secs(20),setup)=>result.map_err(|_|Failure::Connection).and_then(|r|r.map(Some))};
    let result = match result {
        Ok(Some((mut client, inner))) => {
            let manager = client.shard_manager.clone();
            let gateway_stop = stops.clone();
            let gateway = tokio::spawn(async move {
                let _ = client.start().await;
                gateway_stop.send_replace(true);
            });
            let upkeep = tokio::spawn(maintenance(inner.clone()));
            let mut ping = tokio::time::interval(Duration::from_secs(1));
            let mut recovering: Option<Instant> = None;
            loop {
                tokio::select! {biased;_=stop_rx.changed()=>break,
                    Some(command)=command_rx.recv()=>match command {
                        GatewayCommand::Join {key}=>join(inner.clone(),key).await,
                        GatewayCommand::Refresh {key}=>{
                            let requested=inner.lock().await.correlation.request_refresh(&key);
                            if requested {verify(inner.clone(),key).await;}
                        },
                        GatewayCommand::Leave {key}=>inner.lock().await.cancel(&key,None),
                        GatewayCommand::Forget {key}=>{let mut i=inner.lock().await;if i.correlation.forget(&key){i.jobs.cancel(&key);i.calls.remove(&key.server_id);i.times.remove(&key.server_id);}},_=>break},
                    _=ping.tick()=>{let (connected,milliseconds)={let runners=manager.runners.lock().await;let runner=runners.values().next();let connected=runner.is_some_and(|r|r.stage==serenity::gateway::ConnectionStage::Connected);(connected,runner.filter(|_|connected).and_then(|r|r.latency.map(|latency|latency.as_secs_f32()*1000.0)))};
                    let mut i=inner.lock().await;
                    if i.registered {
                        if !i.sender_healthy() {continue;}
                        if !connected {i.connected=false;let since=*recovering.get_or_insert_with(Instant::now);i.emit(GatewayStatus::Recovery {active:true});if since.elapsed()>Duration::from_secs(35){i.fatal(Failure::Connection);}}
                        else if recovering.take().is_some() {i.connected=true;i.emit(GatewayStatus::Recovery {active:false});let keys=i.correlation.slots.values().filter(|s|!s.wanted&&s.issued&&!s.left()).map(|s|s.key.clone()).collect::<Vec<_>>();for key in keys {i.transmit(&key,false);}}
                    }
                    let _=statuses.try_send(GatewayStatus::Ping {milliseconds});}
                }
            }
            {
                let mut i = inner.lock().await;
                let keys = i
                    .correlation
                    .slots
                    .values()
                    .map(|s| s.key.clone())
                    .collect::<Vec<_>>();
                for key in keys {
                    i.cancel(&key, None);
                }
            }
            upkeep.abort();
            let _ = tokio::time::timeout(Duration::from_millis(400), manager.shutdown_all()).await;
            gateway.abort();
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(code) => Err(code),
    };
    let _ = statuses
        .send(match result {
            Ok(()) => GatewayStatus::Stopped,
            Err(code) => GatewayStatus::Failed { code },
        })
        .await;
    drop(statuses);
    let _ = tokio::time::timeout(Duration::from_millis(100), writer).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use songbird::shards::VoiceUpdate;
    fn inner() -> (
        Arc<Mutex<Inner>>,
        futures::channel::mpsc::UnboundedReceiver<ShardRunnerMessage>,
        mpsc::Receiver<GatewayStatus>,
    ) {
        let (sender, receiver) = futures::channel::mpsc::unbounded();
        let (statuses, status_rx) = mpsc::channel(64);
        let (stops, _) = watch::channel(false);
        (
            Arc::new(Mutex::new(Inner {
                correlation: Correlator::default(),
                jobs: crate::discord::work::TaskBudget::default(),
                calls: BTreeMap::new(),
                times: BTreeMap::new(),
                sender: Some(sender),
                registered: true,
                connected: true,
                user: UserId::new(3),
                http: Arc::new(serenity::http::Http::new("synthetic-unused")),
                statuses,
                stops,
            })),
            receiver,
            status_rx,
        )
    }
    fn key() -> RoomKey {
        RoomKey {
            destination_id: "discord-1".into(),
            intent: 1,
            server_id: "1".into(),
            channel_id: "2".into(),
        }
    }
    fn verified_room(i: &mut Inner, key: &RoomKey) {
        i.correlation.reserve(key.clone()).unwrap();
        i.correlation.preflight(key, 0, true);
        i.correlation.mark_join_sent(key);
        i.correlation.state(
            &key.server_id,
            Some(&key.channel_id),
            "fixture-session",
            true,
        );
        i.correlation
            .server(&key.server_id, Some("fixture-endpoint"), "fixture-token");
        let (counter, secret) = i.correlation.begin_verification(key, "3").unwrap();
        assert_eq!(
            i.correlation.verify(
                key,
                counter,
                &secret,
                Some(&key.channel_id),
                Some("fixture-session"),
                true
            ),
            Some(1)
        );
        i.times.insert(key.server_id.clone(), Timing::new());
    }
    fn membership_fixture(channel: &str) -> VoiceState {
        serde_json::from_value(serde_json::json!({
            "guild_id": "1", "channel_id": channel, "user_id": "3",
            "session_id": "fixture-session", "deaf": false, "mute": false,
            "self_deaf": true, "self_mute": false, "self_video": false, "suppress": false
        }))
        .unwrap()
    }
    #[tokio::test]
    async fn closed_registered_sender_fails_the_cohort_without_trusting_shard_stage() {
        for transmit in [false, true] {
            let (inner, receiver, mut statuses) = inner();
            let mut i = inner.lock().await;
            let key = key();
            verified_room(&mut i, &key);
            drop(receiver);
            assert!(!if transmit {
                i.transmit(&key, false)
            } else {
                i.sender_healthy()
            });
            assert!(!i.connected);
            assert!(!i.correlation.slot(&key).unwrap().wanted);
            assert!(*i.stops.borrow());
            assert!(matches!(
                statuses.try_recv(),
                Ok(GatewayStatus::Failed {
                    code: Failure::Connection
                })
            ));
        }
    }
    #[tokio::test(start_paused = true)]
    async fn permission_denial_applies_immediately_while_transient_errors_get_grace() {
        let (inner, _receiver, mut statuses) = inner();
        let mut i = inner.lock().await;
        let key = key();
        verified_room(&mut i, &key);
        let now = Instant::now();
        i.finish_permissions(key.clone(), Err(Failure::Connection));
        assert!(i.correlation.slot(&key).unwrap().wanted);
        assert!(matches!(
            statuses.try_recv(),
            Ok(GatewayStatus::Health {
                recovering: true,
                ..
            })
        ));
        assert!(
            !i.times
                .get_mut(&key.server_id)
                .unwrap()
                .permissions
                .begin(now)
        );
        i.finish_permissions(key.clone(), Err(Failure::Permission));
        assert_eq!(Instant::now(), now);
        assert!(!i.correlation.slot(&key).unwrap().wanted);
        assert!(matches!(
            statuses.try_recv(),
            Ok(GatewayStatus::RoomFailed {
                code: Failure::Permission,
                ..
            })
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn credential_refresh_retries_transient_reads_and_keeps_the_membership_fence() {
        let (inner, _receiver, mut statuses) = inner();
        let mut i = inner.lock().await;
        let key = key();
        verified_room(&mut i, &key);
        let mut peer = key.clone();
        peer.server_id = "4".into();
        peer.destination_id = "discord-2".into();
        verified_room(&mut i, &peer);
        i.correlation
            .server(&key.server_id, Some("next-endpoint"), "next-token");
        let (counter, secret) = i.correlation.begin_verification(&key, "3").unwrap();
        i.finish_verification(
            key.clone(),
            counter,
            secret.clone(),
            Err(Failure::Connection),
        );
        assert!(i.correlation.slot(&key).unwrap().wanted);
        assert!(i.correlation.slot(&key).unwrap().verifying.is_none());
        assert_eq!(i.correlation.slot(&key).unwrap().revision, 1);
        assert_eq!(
            i.times[&key.server_id]
                .verification
                .readiness(Instant::now()),
            RetryReadiness::Waiting
        );
        assert!(matches!(
            statuses.try_recv(),
            Ok(GatewayStatus::Health {
                recovering: true,
                ..
            })
        ));
        tokio::time::advance(Duration::from_secs(1)).await;
        let (counter, secret) = i.correlation.begin_verification(&key, "3").unwrap();
        i.finish_verification(
            key.clone(),
            counter,
            secret,
            Ok(Some(membership_fixture("2"))),
        );
        assert_eq!(i.correlation.slot(&key).unwrap().revision, 2);
        assert!(matches!(
            statuses.try_recv(),
            Ok(GatewayStatus::Credentials { revision: 2, .. })
        ));
        assert_eq!(i.correlation.slot(&peer).unwrap().revision, 1);
        assert!(i.correlation.slot(&peer).unwrap().wanted);

        i.correlation
            .server(&key.server_id, Some("third-endpoint"), "third-token");
        let (counter, secret) = i.correlation.begin_verification(&key, "3").unwrap();
        i.finish_verification(
            key.clone(),
            counter,
            secret,
            Ok(Some(membership_fixture("99"))),
        );
        assert!(!i.correlation.slot(&key).unwrap().wanted);
        assert!(matches!(
            statuses.try_recv(),
            Ok(GatewayStatus::RoomFailed {
                code: Failure::Permission,
                ..
            })
        ));
        assert!(i.correlation.slot(&peer).unwrap().wanted);
    }
    #[tokio::test(start_paused = true)]
    async fn credential_retry_cannot_survive_deadline_or_resurrect_cancelled_intent() {
        let (inner, _receiver, mut statuses) = inner();
        let mut i = inner.lock().await;
        let key = key();
        verified_room(&mut i, &key);
        i.correlation
            .server(&key.server_id, Some("next-endpoint"), "next-token");
        let (counter, secret) = i.correlation.begin_verification(&key, "3").unwrap();
        i.finish_verification(key.clone(), counter, secret, Err(Failure::Connection));
        let _ = statuses.try_recv();
        tokio::time::advance(Duration::from_secs(30)).await;
        let (counter, secret) = i.correlation.begin_verification(&key, "3").unwrap();
        i.finish_verification(
            key.clone(),
            counter,
            secret.clone(),
            Err(Failure::Connection),
        );
        assert!(!i.correlation.slot(&key).unwrap().wanted);
        assert!(matches!(
            statuses.try_recv(),
            Ok(GatewayStatus::RoomFailed {
                code: Failure::Connection,
                ..
            })
        ));
        i.finish_verification(
            key.clone(),
            counter,
            secret,
            Ok(Some(membership_fixture("2"))),
        );
        assert!(!i.correlation.slot(&key).unwrap().wanted);
        assert!(!matches!(
            statuses.try_recv(),
            Ok(GatewayStatus::Credentials { .. })
        ));
    }
    #[tokio::test]
    async fn cancelled_generic_sender_cannot_replay_a_join_after_registration() {
        let (inner, mut receiver, _) = inner();
        let key = key();
        {
            let mut i = inner.lock().await;
            i.correlation.reserve(key.clone()).unwrap();
            assert!(i.correlation.preflight(&key, 0, true));
            i.correlation.cancel(&key);
        }
        let sender = Gated {
            inner: Arc::downgrade(&inner),
            key,
        };
        assert!(
            sender
                .update_voice_state(
                    GuildId::new(1).into(),
                    Some(serenity::all::ChannelId::new(2).into()),
                    true,
                    false
                )
                .await
                .is_err()
        );
        assert!(receiver.try_recv().is_err());
    }
    #[tokio::test]
    async fn fatal_epoch_disarms_all_pending_verifications_before_publication() {
        let (inner, _, mut statuses) = inner();
        let key = key();
        let mut i = inner.lock().await;
        i.correlation.reserve(key.clone()).unwrap();
        i.correlation.preflight(&key, 0, true);
        i.correlation.mark_join_sent(&key);
        i.correlation
            .state("1", Some("2"), "private-session-canary", true);
        i.correlation
            .server("1", Some("private-endpoint-canary"), "private-token-canary");
        let (counter, secret) = i.correlation.begin_verification(&key, "3").unwrap();
        i.fatal(Failure::Connection);
        assert!(
            i.correlation
                .verify(
                    &key,
                    counter,
                    &secret,
                    Some("2"),
                    Some("private-session-canary"),
                    true
                )
                .is_none()
        );
        assert!(!i.correlation.slot(&key).unwrap().wanted);
        assert!(!i.transmit(&key, true));
        assert!(matches!(
            statuses.try_recv(),
            Ok(GatewayStatus::Failed {
                code: Failure::Connection
            })
        ));
        assert!(statuses.try_recv().is_err());
    }
    #[tokio::test]
    async fn saturated_cancelled_work_defers_a_healthy_peers_background_check() {
        let (inner, _, mut statuses) = inner();
        let key = key();
        {
            let mut i = inner.lock().await;
            i.correlation.reserve(key.clone()).unwrap();
            i.correlation.preflight(&key, 0, true);
            i.correlation.mark_join_sent(&key);
            let mut timing = Timing::new();
            assert!(timing.permissions.begin(Instant::now()));
            i.times.insert(key.server_id.clone(), timing);
            for intent in 2..14 {
                let mut cancelled = key.clone();
                cancelled.intent = intent;
                assert!(i.jobs.spawn(cancelled.clone(), std::future::pending()));
                i.jobs.cancel(&cancelled);
            }
        }
        spawn_room(inner.clone(), key.clone(), WorkKind::Permission, async {
            panic!("over-budget work must not execute")
        })
        .await;
        let mut i = inner.lock().await;
        assert!(i.correlation.slot(&key).unwrap().wanted);
        assert!(
            i.times
                .get_mut(&key.server_id)
                .unwrap()
                .permissions
                .begin(Instant::now() + Duration::from_secs(5))
        );
        assert!(matches!(
            statuses.try_recv(),
            Ok(GatewayStatus::Health {
                recovering: true,
                ..
            })
        ));
        assert!(statuses.try_recv().is_err());
    }
}
