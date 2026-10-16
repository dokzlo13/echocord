//! Parent-side room ownership. Effects are private; snapshots contain no connection secrets.
use super::protocol::*;
use crate::{
    model::*,
    protocol::{Failure, clean_capture},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub enum Effect {
    LaunchGateway {
        generation: u32,
    },
    StopGateway,
    Gateway(GatewayCommand),
    LaunchVoice {
        key: RoomKey,
        generation: u32,
        revision: u32,
        secret: VoiceSecret,
    },
    StopVoice {
        destination_id: String,
        generation: u32,
    },
}
pub struct Room {
    pub key: RoomKey,
    pub session_id: u32,
    pub connection: Connection,
    pub capture: Capture,
    pub observed_at_ms: Option<f64>,
    pub wanted: bool,
    pub joined: bool,
    pub voice: Option<u32>,
    pub remote_left: bool,
    voice_stopping: bool,
    revision: u32,
    pending: Option<(u32, VoiceSecret)>,
    failure: Option<AppError>,
    refreshes: [Option<Instant>; 2],
    since: Instant,
}
pub struct Manager {
    pub rooms: BTreeMap<String, Room>,
    pub gateway: GatewaySnapshot,
    pub gateway_alive: bool,
    pub ping: Option<f32>,
    pub ping_observed_at_ms: Option<f64>,
    pub effects: Vec<Effect>,
    next_intent: u64,
    next_voice: u32,
    ready: bool,
    failed: bool,
    stopping: bool,
    since: Instant,
    next_launch: Option<Instant>,
}
impl Default for Manager {
    fn default() -> Self {
        Self {
            rooms: BTreeMap::new(),
            gateway: GatewaySnapshot::default(),
            gateway_alive: false,
            ping: None,
            ping_observed_at_ms: None,
            effects: Vec::new(),
            next_intent: 1,
            next_voice: 1,
            ready: false,
            failed: false,
            stopping: false,
            since: Instant::now(),
            next_launch: None,
        }
    }
}
fn busy(message: &str) -> AppError {
    AppError {
        code: ErrorCode::Busy,
        message: message.into(),
    }
}
impl Manager {
    pub fn busy(&self) -> bool {
        self.gateway_alive || !self.rooms.is_empty()
    }
    pub fn contains(&self, id: &str) -> bool {
        self.rooms.contains_key(id)
    }
    pub fn start(&mut self, destination: &DestinationStrip) -> Result<(), AppError> {
        if self.rooms.contains_key(&destination.destination_id) {
            return Err(busy("This room is still active or finishing cleanup."));
        }
        if self
            .rooms
            .values()
            .any(|room| room.key.server_id == destination.server_id)
        {
            return Err(busy(
                "This server is still owned by another room or its cleanup.",
            ));
        }
        if self.stopping
            || self.failed && self.gateway_alive
            || self.gateway.phase == GatewayPhase::Recovering
        {
            return Err(busy("Wait for the previous gateway to finish stopping."));
        }
        if !self.gateway_alive && self.next_launch.is_some_and(|at| Instant::now() < at) {
            return Err(busy("Wait before starting another gateway session."));
        }
        let intent = self.next_intent;
        self.next_intent = self
            .next_intent
            .checked_add(1)
            .ok_or_else(|| Failure::Protocol.error())?;
        let key = RoomKey {
            destination_id: destination.destination_id.clone(),
            intent,
            server_id: destination.server_id.clone(),
            channel_id: destination.channel_id.clone(),
        };
        if !valid_key(&key) {
            return Err(Failure::Protocol.error());
        }
        let session = self.allocate_voice()?;
        let room = Room {
            key: key.clone(),
            session_id: session,
            connection: Connection::Connecting {
                stage: ConnectStage::Checking,
            },
            capture: ControlSnapshot::default().capture,
            observed_at_ms: None,
            wanted: true,
            joined: false,
            voice: None,
            remote_left: false,
            voice_stopping: false,
            revision: 0,
            pending: None,
            failure: None,
            refreshes: [None; 2],
            since: Instant::now(),
        };
        self.rooms.insert(key.destination_id.clone(), room);
        if !self.gateway_alive {
            self.gateway.generation = self
                .gateway
                .generation
                .checked_add(1)
                .ok_or_else(|| Failure::Protocol.error())?;
            self.gateway.phase = GatewayPhase::Connecting;
            self.gateway.message = None;
            self.gateway_alive = true;
            self.ready = false;
            self.failed = false;
            self.stopping = false;
            self.since = Instant::now();
            self.next_launch = Some(self.since + Duration::from_secs(5));
            self.effects.push(Effect::LaunchGateway {
                generation: self.gateway.generation,
            });
        } else if self.ready {
            self.join(&key.destination_id);
        }
        Ok(())
    }
    fn allocate_voice(&mut self) -> Result<u32, AppError> {
        let value = self.next_voice;
        self.next_voice = self
            .next_voice
            .checked_add(1)
            .ok_or_else(|| Failure::Protocol.error())?;
        Ok(value)
    }
    fn join(&mut self, id: &str) {
        if let Some(room) = self.rooms.get_mut(id)
            && room.wanted
            && !room.joined
        {
            room.joined = true;
            self.effects.push(Effect::Gateway(GatewayCommand::Join {
                key: room.key.clone(),
            }));
        }
    }
    pub fn stop(&mut self, id: &str, failure: Option<AppError>) {
        let Some(room) = self.rooms.get_mut(id) else {
            return;
        };
        if room.wanted {
            room.since = Instant::now();
        }
        room.wanted = false;
        room.voice_stopping = true;
        room.pending = None;
        if failure.is_some() {
            room.failure = failure;
        }
        if !matches!(room.connection, Connection::CleanupIncomplete { .. }) {
            room.connection = Connection::Disconnecting;
        }
        room.capture = ControlSnapshot::default().capture;
        room.observed_at_ms = None;
        if let Some(generation) = room.voice {
            self.effects.push(Effect::StopVoice {
                destination_id: id.into(),
                generation,
            });
        }
        if room.joined && self.gateway_alive && !self.failed {
            self.effects.push(Effect::Gateway(GatewayCommand::Leave {
                key: room.key.clone(),
            }));
        } else if !room.joined {
            room.remote_left = true;
        }
    }
    pub fn stop_all(&mut self) {
        let ids = self.rooms.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            self.stop(&id, None);
        }
    }
    pub fn gateway_status(&mut self, generation: u32, status: GatewayStatus) {
        if generation != self.gateway.generation || !self.gateway_alive {
            return;
        }
        match status {
            GatewayStatus::Recovery { active } if !self.failed && !self.stopping => {
                self.ready = !active;
                self.gateway.phase = if active {
                    GatewayPhase::Recovering
                } else {
                    GatewayPhase::Connected
                };
            }
            GatewayStatus::Ready if !self.failed && !self.stopping => {
                self.ready = true;
                self.gateway.phase = GatewayPhase::Connected;
                let ids = self.rooms.keys().cloned().collect::<Vec<_>>();
                for id in ids {
                    self.join(&id);
                }
            }
            GatewayStatus::Allowance {
                remaining,
                reset_after_ms,
            } => {
                if remaining == 0 {
                    self.next_launch = Some(
                        Instant::now() + Duration::from_millis(reset_after_ms.min(86_400_000)),
                    );
                }
            }
            GatewayStatus::Ping {
                milliseconds,
                observed_at_ms,
            } if !self.failed && !self.stopping => {
                self.ping_observed_at_ms = crate::audio::clean_observation_time(
                    observed_at_ms,
                    crate::audio::observation_clock_ms(),
                );
                self.ping = milliseconds.filter(|n| n.is_finite() && (0.0..=60_000.0).contains(n));
                if self.ping.is_none() || self.ping_observed_at_ms.is_none() {
                    self.ping = None;
                    self.ping_observed_at_ms = None;
                }
            }
            GatewayStatus::Failed { code } => self.gateway_failed(code),
            GatewayStatus::Stopped if !self.stopping => self.gateway_failed(Failure::Connection),
            GatewayStatus::Joining { key } => {
                if let Some(room) = self.current_mut(&key).filter(|r| r.wanted) {
                    room.connection = Connection::Connecting {
                        stage: ConnectStage::Join,
                    };
                }
            }
            GatewayStatus::Credentials {
                key,
                revision,
                secret,
            } => {
                if !valid_secret(&secret) {
                    self.gateway_failed(Failure::Protocol);
                    return;
                }
                if let Some(room) = self
                    .current_mut(&key)
                    .filter(|r| r.wanted && revision > r.revision)
                {
                    if room.connection == Connection::Connected {
                        room.since = Instant::now();
                    }
                    room.revision = revision;
                    room.pending = Some((revision, secret));
                    if let Some(generation) = room.voice {
                        room.voice_stopping = true;
                        if !matches!(room.connection, Connection::CleanupIncomplete { .. }) {
                            room.connection = Connection::Recovering {
                                attempt: 1,
                                message: "Updating this room’s voice connection.".into(),
                            };
                        }
                        self.effects.push(Effect::StopVoice {
                            destination_id: key.destination_id,
                            generation,
                        });
                    }
                }
            }
            GatewayStatus::Reconfiguring { key } => {
                if let Some(room) = self.current_mut(&key).filter(|r| r.wanted) {
                    if room.connection == Connection::Connected {
                        room.since = Instant::now();
                    }
                    room.pending = None;
                    room.voice_stopping = true;
                    room.capture = ControlSnapshot::default().capture;
                    room.observed_at_ms = None;
                    if !matches!(room.connection, Connection::CleanupIncomplete { .. }) {
                        room.connection = Connection::Recovering {
                            attempt: 1,
                            message: "Updating this room’s voice connection.".into(),
                        };
                    }
                    if let Some(generation) = room.voice {
                        self.effects.push(Effect::StopVoice {
                            destination_id: key.destination_id,
                            generation,
                        });
                    }
                }
            }
            GatewayStatus::Health { key, recovering } => {
                if let Some(room) = self.current_mut(&key).filter(|r| {
                    r.wanted
                        && !r.voice_stopping
                        && r.voice.is_some()
                        && matches!(
                            r.connection,
                            Connection::Connected | Connection::Recovering { .. }
                        )
                }) {
                    if recovering && room.connection == Connection::Connected {
                        room.since = Instant::now();
                    }
                    room.connection = if recovering {
                        Connection::Recovering {
                            attempt: 1,
                            message: "Checking this room’s permissions.".into(),
                        }
                    } else {
                        Connection::Connected
                    };
                }
            }
            GatewayStatus::Left { key } => {
                if let Some(room) = self.current_mut(&key).filter(|r| !r.wanted) {
                    room.remote_left = true;
                }
            }
            GatewayStatus::RoomFailed { key, code } => {
                if self.current_mut(&key).is_some() {
                    self.stop(&key.destination_id, Some(code.error()));
                }
            }
            GatewayStatus::CleanupIncomplete { key } => {
                if let Some(room) = self.current_mut(&key) {
                    room.connection=Connection::CleanupIncomplete {message:"Discord has not confirmed this room’s departure. Disconnect each Discord destination to reset the gateway.".into()};
                }
            }
            _ => {}
        }
    }
    fn current_mut(&mut self, key: &RoomKey) -> Option<&mut Room> {
        self.rooms
            .get_mut(&key.destination_id)
            .filter(|r| r.key == *key)
    }
    pub fn gateway_failed(&mut self, code: Failure) {
        if self.failed {
            return;
        }
        self.failed = true;
        self.ready = false;
        self.ping = None;
        self.ping_observed_at_ms = None;
        self.gateway.phase = GatewayPhase::Error;
        self.gateway.message = Some(code.error().message);
        let ids = self.rooms.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            self.stop(&id, Some(code.error()));
        }
        self.effects.push(Effect::StopGateway);
        self.stopping = true;
    }
    pub fn gateway_exit(&mut self, generation: u32, confirmed: bool) {
        if generation != self.gateway.generation || !self.gateway_alive {
            return;
        }
        if !confirmed {
            self.gateway.phase = GatewayPhase::CleanupIncomplete;
            self.gateway.message = Some("Gateway exit could not be confirmed.".into());
            return;
        }
        if !self.stopping {
            self.gateway_failed(Failure::Connection);
        }
        self.gateway_alive = false;
        self.ready = false;
        self.stopping = false;
        self.ping = None;
        self.ping_observed_at_ms = None;
        if !self.failed {
            let uncertain = self.rooms.values().any(|r| r.joined && !r.remote_left);
            self.gateway.phase = if uncertain {
                GatewayPhase::Error
            } else {
                GatewayPhase::Disconnected
            };
            self.gateway.message=uncertain.then(||"Local outputs stopped; Discord departure was not confirmed. Reconnect checks current membership before joining.".into());
        }
    }
    pub fn voice_status(&mut self, id: &str, generation: u32, status: VoiceStatus) {
        let Some(room) = self
            .rooms
            .get_mut(id)
            .filter(|r| r.voice == Some(generation) && r.wanted && !r.voice_stopping)
        else {
            return;
        };
        match status {
            VoiceStatus::Connected => {
                room.since = Instant::now();
                room.connection = Connection::Connected;
            }
            VoiceStatus::Meter {
                capture,
                observed_at_ms,
            } if matches!(
                room.connection,
                Connection::Connected | Connection::Recovering { .. }
            ) =>
            {
                room.observed_at_ms = crate::audio::clean_observation_time(
                    observed_at_ms,
                    crate::audio::observation_clock_ms(),
                );
                room.capture = clean_capture(capture);
                if room.observed_at_ms.is_none() {
                    room.capture.output_level_dbfs = None;
                }
            }
            VoiceStatus::RefreshRequired => {
                let now = Instant::now();
                for previous in &mut room.refreshes {
                    if previous.is_some_and(|at| now.duration_since(at) >= Duration::from_secs(60))
                    {
                        *previous = None;
                    }
                }
                let Some(available) = room.refreshes.iter_mut().find(|entry| entry.is_none())
                else {
                    self.stop(id, Some(Failure::Connection.error()));
                    return;
                };
                *available = Some(now);
                if room.connection == Connection::Connected {
                    room.since = now;
                }
                room.voice_stopping = true;
                room.pending = None;
                room.capture = ControlSnapshot::default().capture;
                room.observed_at_ms = None;
                room.connection = Connection::Recovering {
                    attempt: room
                        .refreshes
                        .iter()
                        .filter(|entry| entry.is_some())
                        .count() as u8,
                    message: "Refreshing this room’s voice transport.".into(),
                };
                self.effects.push(Effect::StopVoice {
                    destination_id: id.into(),
                    generation,
                });
                self.effects.push(Effect::Gateway(GatewayCommand::Refresh {
                    key: room.key.clone(),
                }));
            }
            VoiceStatus::Failed { code } => self.stop(id, Some(code.error())),
            VoiceStatus::Stopped => self.stop(id, Some(Failure::Connection.error())),
            _ => {}
        }
    }
    pub fn voice_exit(&mut self, id: &str, generation: u32, confirmed: bool) {
        let Some(room) = self
            .rooms
            .get_mut(id)
            .filter(|r| r.voice == Some(generation))
        else {
            return;
        };
        if !confirmed {
            room.connection = Connection::CleanupIncomplete {
                message: "Voice worker exit could not be confirmed.".into(),
            };
            return;
        }
        room.voice = None;
        room.capture = ControlSnapshot::default().capture;
        room.observed_at_ms = None;
        if room.wanted && !room.voice_stopping && room.pending.is_none() {
            self.stop(id, Some(Failure::Connection.error()));
        }
    }
    /// Advance local ownership; returned terminal cards may be removed/restarted by the controller.
    pub fn advance(&mut self, keep_gateway: bool) -> Vec<(String, Connection)> {
        let now = Instant::now();
        if self.gateway_alive
            && self.gateway.phase == GatewayPhase::Connecting
            && !self.stopping
            && now.duration_since(self.since) > Duration::from_secs(40)
        {
            self.gateway_failed(Failure::Connection);
        }
        let ids = self.rooms.keys().cloned().collect::<Vec<_>>();
        let mut done = Vec::new();
        for id in ids {
            let room = &self.rooms[&id];
            if room.wanted
                && matches!(
                    room.connection,
                    Connection::Connecting { .. } | Connection::Recovering { .. }
                )
                && now.duration_since(room.since) > Duration::from_secs(45)
            {
                self.stop(&id, Some(Failure::Connection.error()));
            }
            let room = self.rooms.get_mut(&id).expect("existing");
            if self.ready && room.wanted && room.voice.is_none() && room.pending.is_some() {
                let (revision, secret) = room.pending.take().expect("pending");
                let key = room.key.clone();
                let first = room.revision == 1 && room.session_id != 0;
                let generation = if first {
                    room.session_id
                } else {
                    match self.allocate_voice() {
                        Ok(generation) => generation,
                        Err(error) => {
                            self.stop(&id, Some(error));
                            continue;
                        }
                    }
                };
                let room = self.rooms.get_mut(&id).expect("existing");
                room.session_id = generation;
                room.voice = Some(generation);
                room.voice_stopping = false;
                room.connection = Connection::Connecting {
                    stage: ConnectStage::Join,
                };
                self.effects.push(Effect::LaunchVoice {
                    key,
                    generation,
                    revision,
                    secret,
                });
            }
            let room = &self.rooms[&id];
            if !room.wanted && room.voice.is_none() && (room.remote_left || !self.gateway_alive) {
                let mut room = self.rooms.remove(&id).expect("existing");
                if room.joined && !room.remote_left && room.failure.is_none() {
                    room.failure=Some(AppError {code:ErrorCode::ConnectionFailed,message:"Local output stopped; Discord departure was not confirmed. Reconnect checks current membership before joining.".into()});
                }
                if self.gateway_alive && !self.failed {
                    self.effects
                        .push(Effect::Gateway(GatewayCommand::Forget { key: room.key }));
                }
                done.push((
                    id,
                    room.failure
                        .map_or(Connection::Disconnected, |error| Connection::Error {
                            code: error.code,
                            message: error.message,
                        }),
                ));
            } else if !room.wanted && now.duration_since(room.since) > Duration::from_secs(12) {
                self.rooms.get_mut(&id).expect("existing").connection=Connection::CleanupIncomplete {message:"Room cleanup is still waiting for a confirmed departure or worker exit.".into()};
            }
        }
        // With no remaining wanted room, reset an ambiguous remote reservation by disposing
        // the whole gateway. A later explicit Connect must reconcile membership through REST.
        if !keep_gateway
            && self.gateway_alive
            && !self.stopping
            && self.rooms.values().all(|r| !r.wanted)
            && self.rooms.values().all(|r| {
                // A leave during gateway recovery cannot be sent immediately.
                // Keep its original deadline so a resume can retransmit it;
                // an early CleanupIncomplete notification is not that deadline.
                r.remote_left || now.duration_since(r.since) >= Duration::from_secs(12)
            })
        {
            self.stopping = true;
            self.gateway.phase = GatewayPhase::Disconnecting;
            self.effects.push(Effect::StopGateway);
        }
        done
    }
    pub fn publish(&self, snapshot: &mut ControlSnapshot) {
        snapshot.gateway = self.gateway.clone();
        snapshot.server_reservations.clear();
        for d in &mut snapshot.destinations {
            if d.kind != DestinationKind::Discord {
                continue;
            }
            let active = self.rooms.get(&d.destination_id);
            if let Some(room) = active {
                d.session_id = room.session_id;
                d.connection = room.connection.clone();
            }
            let phase = active
                .filter(|r| r.key.server_id == d.server_id && r.key.channel_id == d.channel_id)
                .map_or(ReservationPhase::Selected, reservation_phase);
            snapshot.server_reservations.push(ServerReservation {
                server_id: d.server_id.clone(),
                channel_id: d.channel_id.clone(),
                destination_id: d.destination_id.clone(),
                session_id: d.session_id,
                phase,
            });
            let capture = active.map(|r| &r.capture);
            let measured = DestinationTelemetry {
                observed_at_ms: active.and_then(|room| room.observed_at_ms),
                gateway_observed_at_ms: active
                    .filter(|room| room.wanted)
                    .and(self.ping_observed_at_ms),
                destination_id: d.destination_id.clone(),
                session_id: d.session_id,
                level_dbfs: capture.and_then(|c| c.output_level_dbfs),
                queued_frames: capture.map_or(0, |c| c.queued_frames),
                dropped_frames: capture.map_or(0, |c| c.dropped_frames),
                underrun_frames: capture.map_or(0, |c| c.underrun_frames),
                gateway_ping_ms: active.filter(|r| r.wanted).and(self.ping),
            };
            if let Some(old) = snapshot
                .telemetry
                .destinations
                .iter_mut()
                .find(|m| m.destination_id == d.destination_id)
            {
                *old = measured;
            } else {
                snapshot.telemetry.destinations.push(measured);
            }
        }
        for room in self.rooms.values() {
            if !snapshot.server_reservations.iter().any(|r| {
                r.server_id == room.key.server_id
                    && r.channel_id == room.key.channel_id
                    && r.destination_id == room.key.destination_id
            }) {
                snapshot.server_reservations.push(ServerReservation {
                    server_id: room.key.server_id.clone(),
                    channel_id: room.key.channel_id.clone(),
                    destination_id: room.key.destination_id.clone(),
                    session_id: room.session_id,
                    phase: reservation_phase(room),
                });
            }
        }
    }
}
fn reservation_phase(room: &Room) -> ReservationPhase {
    if matches!(room.connection, Connection::CleanupIncomplete { .. }) {
        ReservationPhase::Blocked
    } else if !room.wanted {
        ReservationPhase::Leaving
    } else if matches!(
        room.connection,
        Connection::Connected | Connection::Recovering { .. }
    ) {
        ReservationPhase::Connected
    } else {
        ReservationPhase::Joining
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn destination(id: &str, guild: &str) -> DestinationStrip {
        DestinationStrip {
            destination_id: id.into(),
            session_id: 0,
            kind: DestinationKind::Discord,
            device_id: None,
            display_name: None,
            server_id: guild.into(),
            channel_id: "20".into(),
            connection: Connection::Disconnected,
            volume_db: Some(-12.5),
            muted: false,
        }
    }
    fn secret(token: &str) -> VoiceSecret {
        VoiceSecret {
            endpoint: "private-endpoint-canary".into(),
            session_id: "private-session-canary".into(),
            token: token.into(),
            user_id: "10".into(),
        }
    }
    fn active(m: &mut Manager, id: &str, guild: &str) -> (RoomKey, u32) {
        m.start(&destination(id, guild)).unwrap();
        m.gateway_status(m.gateway.generation, GatewayStatus::Ready);
        let key = m.rooms[id].key.clone();
        m.gateway_status(
            m.gateway.generation,
            GatewayStatus::Credentials {
                key: key.clone(),
                revision: 1,
                secret: secret("private-token-canary"),
            },
        );
        m.advance(true);
        let generation = m.rooms[id].voice.unwrap();
        m.voice_status(id, generation, VoiceStatus::Connected);
        m.effects.clear();
        (key, generation)
    }

    #[test]
    fn room_and_gateway_measurements_keep_independent_origins_across_publication() {
        let mut manager = Manager::default();
        let (_, generation) = active(&mut manager, "discord-1", "1");
        let mut capture = ControlSnapshot::default().capture;
        capture.phase = CapturePhase::Running;
        capture.output_level_dbfs = Some(-24.0);
        manager.voice_status(
            "discord-1",
            generation,
            VoiceStatus::Meter {
                capture,
                observed_at_ms: Some(0.0),
            },
        );
        let ping_at = crate::audio::observation_clock_ms();
        manager.gateway_status(
            manager.gateway.generation,
            GatewayStatus::Ping {
                milliseconds: Some(42.0),
                observed_at_ms: Some(ping_at),
            },
        );
        let mut snapshot = ControlSnapshot::default();
        snapshot.destinations.push(destination("discord-1", "1"));
        manager.publish(&mut snapshot);
        let measured = snapshot.telemetry.destinations[0].clone();
        assert_eq!(measured.observed_at_ms, Some(0.0));
        assert_eq!(measured.gateway_observed_at_ms, Some(ping_at));
        for _ in 0..10 {
            manager.publish(&mut snapshot);
            assert_eq!(snapshot.telemetry.destinations[0], measured);
        }
        manager.gateway_status(
            manager.gateway.generation,
            GatewayStatus::Ping {
                milliseconds: Some(10.0),
                observed_at_ms: Some(f64::NAN),
            },
        );
        assert_eq!(manager.ping, None);
        assert_eq!(manager.ping_observed_at_ms, None);
        assert_eq!(manager.rooms["discord-1"].observed_at_ms, Some(0.0));
    }
    #[test]
    fn concurrent_rooms_share_one_gateway_and_stop_requires_both_receipts() {
        for leave_first in [true, false] {
            let mut m = Manager::default();
            let (one, generation) = active(&mut m, "discord-1", "1");
            let (_, peer) = active(&mut m, "discord-2", "2");
            assert_eq!(m.gateway.generation, 1);
            m.stop("discord-1", None);
            if leave_first {
                m.gateway_status(1, GatewayStatus::Left { key: one.clone() });
            } else {
                m.voice_exit("discord-1", generation, true);
            }
            assert!(m.advance(false).is_empty());
            assert!(m.contains("discord-1"));
            assert_eq!(m.rooms["discord-2"].voice, Some(peer));
            assert_eq!(m.rooms["discord-2"].connection, Connection::Connected);
            if leave_first {
                m.voice_exit("discord-1", generation, true);
            } else {
                m.gateway_status(1, GatewayStatus::Left { key: one });
            }
            assert_eq!(
                m.advance(false),
                vec![("discord-1".into(), Connection::Disconnected)]
            );
            assert!(m.gateway_alive);
            assert!(!m.effects.iter().any(|e| matches!(e, Effect::StopGateway)));
        }
    }
    #[test]
    fn refresh_fences_old_worker_and_resets_only_its_reader_session() {
        let mut m = Manager::default();
        let (key, old) = active(&mut m, "discord-1", "1");
        let (_, peer) = active(&mut m, "discord-2", "2");
        m.gateway_status(1, GatewayStatus::Reconfiguring { key: key.clone() });
        m.gateway_status(
            1,
            GatewayStatus::Credentials {
                key,
                revision: 2,
                secret: secret("next-token"),
            },
        );
        m.voice_status(
            "discord-1",
            old,
            VoiceStatus::Failed {
                code: Failure::Connection,
            },
        );
        assert!(m.rooms["discord-1"].wanted);
        assert!(m.advance(false).is_empty());
        assert_eq!(m.rooms["discord-1"].voice, Some(old));
        m.voice_exit("discord-1", old, true);
        m.advance(false);
        let new = m.rooms["discord-1"].voice.unwrap();
        assert_ne!(old, new);
        assert_eq!(new, m.rooms["discord-1"].session_id);
        assert_eq!(peer, m.rooms["discord-2"].session_id);
        m.voice_status("discord-1", old, VoiceStatus::Connected);
        assert!(matches!(
            m.rooms["discord-1"].connection,
            Connection::Connecting { .. }
        ));
    }
    #[test]
    fn cancel_before_gateway_ready_never_joins_or_accepts_late_credentials() {
        let mut m = Manager::default();
        m.start(&destination("discord-1", "1")).unwrap();
        let key = m.rooms["discord-1"].key.clone();
        m.effects.clear();
        m.stop("discord-1", None);
        m.gateway_status(1, GatewayStatus::Ready);
        m.gateway_status(
            1,
            GatewayStatus::Credentials {
                key,
                revision: 1,
                secret: secret("private-token-canary"),
            },
        );
        m.advance(false);
        assert!(!m.effects.iter().any(|e| matches!(
            e,
            Effect::Gateway(GatewayCommand::Join { .. }) | Effect::LaunchVoice { .. }
        )));
    }
    #[test]
    fn same_server_retirement_blocks_reuse_even_without_a_visible_card() {
        let mut m = Manager::default();
        let (_, old) = active(&mut m, "discord-1", "1");
        m.stop("discord-1", None);
        m.voice_exit("discord-1", old, true);
        assert!(m.start(&destination("discord-2", "1")).is_err());
        assert!(m.advance(false).is_empty());
        let mut snapshot = ControlSnapshot::default();
        m.publish(&mut snapshot);
        assert_eq!(snapshot.server_reservations.len(), 1);
        assert_eq!(
            snapshot.server_reservations[0].phase,
            ReservationPhase::Leaving
        );
    }
    #[test]
    fn fatal_gateway_disarms_all_rooms_and_ignores_late_epoch_data() {
        let mut m = Manager::default();
        let (key, one) = active(&mut m, "discord-1", "1");
        let (_, two) = active(&mut m, "discord-2", "2");
        m.gateway_failed(Failure::Connection);
        m.gateway_status(
            1,
            GatewayStatus::Credentials {
                key,
                revision: 2,
                secret: secret("late-token"),
            },
        );
        assert!(m.rooms.values().all(|r| !r.wanted));
        assert_eq!(
            m.effects
                .iter()
                .filter(|e| matches!(e, Effect::StopVoice { .. }))
                .count(),
            2
        );
        m.voice_exit("discord-1", one, true);
        m.voice_exit("discord-2", two, true);
        assert!(m.advance(false).is_empty());
        m.gateway_exit(1, true);
        assert_eq!(m.advance(false).len(), 2);
        m.gateway_status(1, GatewayStatus::Ready);
        assert!(!m.gateway_alive);
    }
    #[test]
    fn failed_local_cleanup_retains_reservation_until_observed_exit() {
        let mut m = Manager::default();
        let (key, one) = active(&mut m, "discord-1", "1");
        active(&mut m, "discord-2", "2");
        m.stop("discord-1", None);
        m.gateway_status(1, GatewayStatus::Left { key });
        m.voice_exit("discord-1", one, false);
        assert!(m.advance(false).is_empty());
        assert!(matches!(
            m.rooms["discord-1"].connection,
            Connection::CleanupIncomplete { .. }
        ));
        m.voice_exit("discord-1", one, true);
        assert_eq!(m.advance(false).len(), 1);
    }
    #[test]
    fn disconnect_during_gateway_recovery_waits_for_leave_confirmation_without_rejoining() {
        let mut m = Manager::default();
        let (key, generation) = active(&mut m, "discord-1", "1");
        m.gateway_status(1, GatewayStatus::Recovery { active: true });
        m.stop("discord-1", None);
        let since = m.rooms["discord-1"].since;
        m.gateway_status(1, GatewayStatus::CleanupIncomplete { key: key.clone() });
        m.voice_exit("discord-1", generation, true);
        m.stop("discord-1", None);
        assert_eq!(m.rooms["discord-1"].since, since);
        m.effects.clear();
        assert!(m.advance(false).is_empty());
        assert!(
            !m.effects
                .iter()
                .any(|effect| matches!(effect, Effect::StopGateway))
        );
        // The gateway can now use its existing resume-time leave retransmit.
        m.gateway_status(1, GatewayStatus::Recovery { active: false });
        m.gateway_status(1, GatewayStatus::Ready);
        assert!(!m.rooms["discord-1"].wanted);
        assert!(
            !m.effects
                .iter()
                .any(|effect| matches!(effect, Effect::Gateway(GatewayCommand::Join { .. })))
        );
        m.gateway_status(1, GatewayStatus::Left { key });
        assert_eq!(
            m.advance(false),
            vec![("discord-1".into(), Connection::Disconnected)]
        );
        assert!(
            m.effects
                .iter()
                .any(|effect| matches!(effect, Effect::StopGateway))
        );
    }
    #[test]
    fn unconfirmed_leave_resets_gateway_after_original_deadline_but_not_with_a_live_peer() {
        for with_peer in [false, true] {
            let mut m = Manager::default();
            let (key, generation) = active(&mut m, "discord-1", "1");
            if with_peer {
                active(&mut m, "discord-2", "2");
            }
            m.gateway_status(1, GatewayStatus::Recovery { active: true });
            m.stop("discord-1", None);
            m.gateway_status(1, GatewayStatus::CleanupIncomplete { key });
            m.voice_exit("discord-1", generation, true);
            m.rooms.get_mut("discord-1").unwrap().since = Instant::now() - Duration::from_secs(11);
            m.effects.clear();
            m.advance(false);
            assert!(
                !m.effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::StopGateway))
            );
            m.rooms.get_mut("discord-1").unwrap().since = Instant::now() - Duration::from_secs(12);
            m.stop("discord-1", None);
            m.advance(false);
            assert_eq!(
                m.effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::StopGateway)),
                !with_peer
            );
            if with_peer {
                assert!(m.rooms["discord-2"].wanted);
                assert_eq!(m.rooms["discord-2"].connection, Connection::Connected);
            } else {
                m.gateway_exit(1, true);
                m.advance(false);
                m.effects.clear();
                m.gateway_status(1, GatewayStatus::Ready);
                assert!(m.rooms.is_empty());
                assert!(m.effects.is_empty());
            }
        }
    }
    #[test]
    fn private_credentials_never_enter_public_snapshot_or_control_key() {
        let mut m = Manager::default();
        active(&mut m, "discord-1", "1");
        let mut snapshot = ControlSnapshot::default();
        snapshot.destinations.push(destination("discord-1", "1"));
        m.publish(&mut snapshot);
        let serialized = serde_json::to_string(&snapshot).unwrap();
        let debug = format!("{snapshot:?}");
        for canary in [
            "private-token-canary",
            "private-session-canary",
            "private-endpoint-canary",
        ] {
            assert!(!serialized.contains(canary));
            assert!(!debug.contains(canary));
        }
    }
    #[test]
    fn repeated_credential_churn_cannot_extend_one_recovery_episode() {
        let mut m = Manager::default();
        let (key, first) = active(&mut m, "discord-1", "1");
        let (_, peer) = active(&mut m, "discord-2", "2");
        m.gateway_status(1, GatewayStatus::Reconfiguring { key: key.clone() });
        let since = Instant::now() - Duration::from_secs(46);
        m.rooms.get_mut("discord-1").unwrap().since = since;
        m.gateway_status(
            1,
            GatewayStatus::Credentials {
                key: key.clone(),
                revision: 2,
                secret: secret("next-token"),
            },
        );
        m.gateway_status(1, GatewayStatus::Reconfiguring { key });
        m.voice_exit("discord-1", first, true);
        assert_eq!(m.rooms["discord-1"].since, since);
        m.advance(false);
        assert!(!m.rooms["discord-1"].wanted);
        assert_eq!(m.rooms["discord-2"].voice, Some(peer));
        assert!(m.rooms["discord-2"].wanted);
    }
    #[test]
    fn unexpected_exit_during_permission_recovery_is_immediately_terminal() {
        let mut m = Manager::default();
        let (key, generation) = active(&mut m, "discord-1", "1");
        m.gateway_status(
            1,
            GatewayStatus::Health {
                key,
                recovering: true,
            },
        );
        m.voice_exit("discord-1", generation, true);
        assert!(!m.rooms["discord-1"].wanted);
    }
    #[test]
    fn transport_refresh_keeps_identity_and_waits_for_confirmed_worker_exit() {
        let mut m = Manager::default();
        let (key, old) = active(&mut m, "discord-1", "1");
        let (_, peer) = active(&mut m, "discord-2", "2");
        m.voice_status("discord-1", old, VoiceStatus::RefreshRequired);
        m.voice_status("discord-1", old, VoiceStatus::RefreshRequired);
        assert_eq!(m.effects.len(), 2);
        assert!(
            matches!(&m.effects[0], Effect::StopVoice { generation, .. } if *generation == old)
        );
        assert!(
            matches!(&m.effects[1], Effect::Gateway(GatewayCommand::Refresh { key: requested }) if requested == &key)
        );
        assert!(
            !m.effects
                .iter()
                .any(|effect| matches!(effect, Effect::Gateway(GatewayCommand::Join { .. })))
        );
        m.voice_exit("discord-1", old, false);
        m.gateway_status(
            1,
            GatewayStatus::Credentials {
                key: key.clone(),
                revision: 2,
                secret: secret("private-token-canary"),
            },
        );
        assert!(matches!(
            m.rooms["discord-1"].connection,
            Connection::CleanupIncomplete { .. }
        ));
        m.effects.clear();
        m.advance(false);
        assert!(m.effects.is_empty());
        assert_eq!(m.rooms["discord-1"].voice, Some(old));
        m.voice_exit("discord-1", old, true);
        m.advance(false);
        let new = m.rooms["discord-1"].voice.unwrap();
        assert_ne!(old, new);
        assert_eq!(m.rooms["discord-1"].key, key);
        assert!(
            matches!(&m.effects[0], Effect::LaunchVoice { revision: 2, generation, key: requested, .. } if *generation == new && requested == &key)
        );
        assert_eq!(m.rooms["discord-2"].voice, Some(peer));
        assert_eq!(m.rooms["discord-2"].connection, Connection::Connected);
        m.effects.clear();
        m.voice_status("discord-1", old, VoiceStatus::RefreshRequired);
        assert!(m.effects.is_empty());
        m.stop("discord-1", None);
        m.effects.clear();
        m.voice_status("discord-1", new, VoiceStatus::RefreshRequired);
        m.gateway_status(
            1,
            GatewayStatus::Credentials {
                key,
                revision: 3,
                secret: secret("private-token-canary"),
            },
        );
        assert!(!m.rooms["discord-1"].wanted);
        assert!(m.effects.is_empty());
    }
    #[test]
    fn refresh_budget_survives_worker_replacement_and_ends_only_the_failing_room() {
        let mut m = Manager::default();
        let (key, mut generation) = active(&mut m, "discord-1", "1");
        active(&mut m, "discord-2", "2");
        for revision in 2..=3 {
            m.voice_status("discord-1", generation, VoiceStatus::RefreshRequired);
            m.gateway_status(
                1,
                GatewayStatus::Credentials {
                    key: key.clone(),
                    revision,
                    secret: secret("private-token-canary"),
                },
            );
            m.voice_exit("discord-1", generation, true);
            m.advance(false);
            generation = m.rooms["discord-1"].voice.unwrap();
            m.voice_status("discord-1", generation, VoiceStatus::Connected);
            assert!(m.rooms["discord-1"].wanted);
        }
        m.effects.clear();
        m.voice_status("discord-1", generation, VoiceStatus::RefreshRequired);
        assert!(!m.rooms["discord-1"].wanted);
        assert!(
            !m.effects
                .iter()
                .any(|effect| matches!(effect, Effect::Gateway(GatewayCommand::Refresh { .. })))
        );
        assert_eq!(
            m.rooms["discord-1"].failure.as_ref().unwrap().code,
            ErrorCode::ConnectionFailed
        );
        assert!(m.rooms["discord-2"].wanted);
        assert_eq!(m.rooms["discord-2"].connection, Connection::Connected);
        m.gateway_status(1, GatewayStatus::Left { key: key.clone() });
        m.voice_exit("discord-1", generation, true);
        m.advance(false);
        let (new_key, new_voice) = active(&mut m, "discord-1", "1");
        assert_ne!(new_key.intent, key.intent);
        m.voice_status("discord-1", new_voice, VoiceStatus::RefreshRequired);
        assert!(m.rooms["discord-1"].wanted);
        assert!(
            m.effects
                .iter()
                .any(|effect| matches!(effect, Effect::Gateway(GatewayCommand::Refresh { .. })))
        );
    }
}
