//! Portable ordered gateway correlation. No API call, audio, clock or worker lives here.
use super::protocol::{RoomKey, VoiceSecret, valid_key};
use crate::protocol::Failure;
use std::collections::{BTreeMap, VecDeque};

const RETIRED_LIMIT: usize = 256;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Joining,
    Active,
    Leaving,
    Left,
    Blocked,
}
#[derive(Clone)]
struct State {
    session: String,
    channel: String,
}
#[derive(Clone)]
struct Server {
    endpoint: String,
    token: String,
}
pub struct Slot {
    pub key: RoomKey,
    pub phase: Phase,
    pub counter: u64,
    pub issued: bool,
    pub wanted: bool,
    pub revision: u32,
    pub verifying: Option<u64>,
    state: Option<State>,
    server: Option<Server>,
    released: Option<VoiceSecret>,
    refresh_requested: bool,
    join_state_seen: bool,
    join_server_seen: bool,
    incomplete_cancel: bool,
    leave_floor: Option<u64>,
    last_nonnull: u64,
    null_at: Option<u64>,
}
impl Slot {
    pub fn candidate(&self, user_id: &str) -> Option<VoiceSecret> {
        if !self.wanted || !self.issued || !matches!(self.phase, Phase::Joining | Phase::Active) {
            return None;
        }
        let (Some(state), Some(server)) = (&self.state, &self.server) else {
            return None;
        };
        if state.channel != self.key.channel_id {
            return None;
        }
        let secret = VoiceSecret {
            endpoint: server.endpoint.clone(),
            token: server.token.clone(),
            session_id: state.session.clone(),
            user_id: user_id.into(),
        };
        (self.refresh_requested || self.released.as_ref() != Some(&secret)).then_some(secret)
    }
    pub fn left(&self) -> bool {
        if !self.issued {
            return true;
        }
        self.null_at.is_some_and(|receipt| {
            self.leave_floor.is_some_and(|floor| receipt > floor) && receipt > self.last_nonnull
        }) && (!self.incomplete_cancel || (self.join_state_seen && self.join_server_seen))
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum StateAction {
    Ignore,
    Forward,
    LeaveAgain,
    Failed(Failure),
    Left,
}
#[derive(Debug, PartialEq, Eq)]
pub enum ServerAction {
    Ignore,
    Forward,
    Reconfigure,
    Failed(Failure),
    Left,
}
#[derive(Default)]
pub struct Correlator {
    pub slots: BTreeMap<String, Slot>,
    retired: VecDeque<(String, String)>,
    pub exhausted: bool,
}
impl Correlator {
    pub fn reserve(&mut self, key: RoomKey) -> Result<(), Failure> {
        if !valid_key(&key)
            || self.slots.len() >= 4
            || self.slots.contains_key(&key.server_id)
            || self.exhausted
        {
            return Err(Failure::Protocol);
        }
        self.slots.insert(
            key.server_id.clone(),
            Slot {
                key,
                phase: Phase::Checking,
                counter: 0,
                issued: false,
                wanted: true,
                revision: 0,
                verifying: None,
                state: None,
                server: None,
                released: None,
                refresh_requested: false,
                join_state_seen: false,
                join_server_seen: false,
                incomplete_cancel: false,
                leave_floor: None,
                last_nonnull: 0,
                null_at: None,
            },
        );
        Ok(())
    }
    pub fn slot(&self, key: &RoomKey) -> Option<&Slot> {
        self.slots.get(&key.server_id).filter(|s| &s.key == key)
    }
    pub fn slot_mut(&mut self, key: &RoomKey) -> Option<&mut Slot> {
        self.slots.get_mut(&key.server_id).filter(|s| &s.key == key)
    }
    pub fn preflight(&mut self, key: &RoomKey, counter: u64, absent: bool) -> bool {
        let Some(slot) = self.slot_mut(key) else {
            return false;
        };
        if !slot.wanted || slot.phase != Phase::Checking || slot.counter != counter || !absent {
            return false;
        }
        slot.phase = Phase::Joining;
        true
    }
    pub fn mark_join_sent(&mut self, key: &RoomKey) -> bool {
        let Some(slot) = self.slot_mut(key) else {
            return false;
        };
        if !slot.wanted || slot.phase != Phase::Joining || slot.issued {
            return false;
        }
        slot.issued = true;
        // Only events following this actual send can discharge its handshake debt.
        slot.join_state_seen = false;
        slot.join_server_seen = false;
        slot.state = None;
        slot.server = None;
        slot.null_at = None;
        true
    }
    pub fn cancel(&mut self, key: &RoomKey) -> bool {
        let Some(slot) = self.slot_mut(key) else {
            return false;
        };
        slot.wanted = false;
        slot.verifying = None;
        slot.incomplete_cancel |= slot.issued && !(slot.join_state_seen && slot.join_server_seen);
        slot.phase = if slot.issued {
            Phase::Leaving
        } else {
            Phase::Left
        };
        slot.left()
    }
    pub fn leave_sent(&mut self, key: &RoomKey) {
        if let Some(slot) = self.slot_mut(key) {
            slot.leave_floor = Some(slot.counter);
            slot.null_at = None;
        }
    }
    pub fn state(
        &mut self,
        guild: &str,
        channel: Option<&str>,
        session: &str,
        permitted: bool,
    ) -> StateAction {
        let Some(slot) = self.slots.get_mut(guild) else {
            return StateAction::Ignore;
        };
        slot.counter += 1;
        if let Some(channel) = channel {
            slot.last_nonnull = slot.counter;
            slot.null_at = None;
            if slot.issued && channel == slot.key.channel_id {
                slot.join_state_seen = true;
            }
            if !slot.wanted {
                return StateAction::LeaveAgain;
            }
            if slot.phase == Phase::Checking {
                return StateAction::Ignore;
            }
            if !slot.issued {
                return StateAction::Ignore;
            }
            if channel != slot.key.channel_id || !permitted {
                return StateAction::Failed(Failure::Permission);
            }
            if slot
                .released
                .as_ref()
                .is_some_and(|old| old.session_id != session)
            {
                return StateAction::Failed(Failure::Connection);
            }
            slot.state = Some(State {
                session: session.into(),
                channel: channel.into(),
            });
            StateAction::Forward
        } else {
            slot.null_at = Some(slot.counter);
            slot.state = None;
            if !slot.wanted {
                if slot.left() {
                    slot.phase = Phase::Left;
                    StateAction::Left
                } else {
                    StateAction::Ignore
                }
            } else if slot.issued {
                StateAction::Failed(Failure::Permission)
            } else {
                StateAction::Ignore
            }
        }
    }
    pub fn server(&mut self, guild: &str, endpoint: Option<&str>, token: &str) -> ServerAction {
        if self
            .retired
            .iter()
            .any(|(old_guild, old_token)| old_guild == guild && old_token == token)
        {
            return ServerAction::Ignore;
        }
        let Some(slot) = self.slots.get_mut(guild) else {
            self.retire(guild, token);
            return ServerAction::Ignore;
        };
        slot.counter += 1;
        if !slot.issued {
            self.retire(guild, token);
            return ServerAction::Ignore;
        }
        if endpoint.is_some() {
            slot.join_server_seen = true;
        }
        if !slot.wanted {
            let left = slot.left();
            if left {
                slot.phase = Phase::Left;
            }
            self.retire(guild, token);
            return if left {
                ServerAction::Left
            } else {
                ServerAction::Ignore
            };
        }
        let Some(endpoint) = endpoint else {
            let old = slot.server.take();
            let released = slot.released.clone();
            if let Some(old) = old {
                self.retire(guild, &old.token);
            }
            if let Some(old) = released {
                self.retire(guild, &old.token);
            }
            return ServerAction::Reconfigure;
        };
        if let Some(current) = &slot.server {
            if current.token == token && current.endpoint == endpoint {
                return ServerAction::Ignore;
            }
            if slot.revision == 0 {
                return ServerAction::Failed(Failure::Protocol);
            }
        }
        let old = slot.server.replace(Server {
            endpoint: endpoint.into(),
            token: token.into(),
        });
        let action = if slot.revision > 0 {
            ServerAction::Reconfigure
        } else {
            ServerAction::Forward
        };
        if let Some(old) = old {
            self.retire(guild, &old.token);
        }
        action
    }
    pub fn begin_verification(&mut self, key: &RoomKey, user: &str) -> Option<(u64, VoiceSecret)> {
        let slot = self.slot_mut(key)?;
        if slot.verifying.is_some() {
            return None;
        }
        let secret = slot.candidate(user)?;
        slot.verifying = Some(slot.counter);
        Some((slot.counter, secret))
    }
    /// Renew only the current active credential, with a new REST/counter fence.
    /// This never sends another gateway join or accepts a retired token.
    pub fn request_refresh(&mut self, key: &RoomKey) -> bool {
        let Some(slot) = self.slot(key) else {
            return false;
        };
        let (Some(released), Some(server), Some(state)) =
            (&slot.released, &slot.server, &slot.state)
        else {
            return false;
        };
        if !slot.wanted
            || !slot.issued
            || slot.phase != Phase::Active
            || slot.refresh_requested
            || released.token != server.token
            || released.endpoint != server.endpoint
            || released.session_id != state.session
            || state.channel != key.channel_id
            || self
                .retired
                .iter()
                .any(|(guild, token)| guild == &key.server_id && token == &released.token)
        {
            return false;
        }
        let Some(counter) = slot.counter.checked_add(1) else {
            return false;
        };
        let slot = self.slot_mut(key).expect("matched slot");
        slot.counter = counter;
        slot.refresh_requested = true;
        true
    }
    pub fn verify(
        &mut self,
        key: &RoomKey,
        counter: u64,
        secret: &VoiceSecret,
        current_channel: Option<&str>,
        current_session: Option<&str>,
        permitted: bool,
    ) -> Option<u32> {
        let slot = self.slot_mut(key)?;
        if slot.verifying != Some(counter) {
            return None;
        }
        slot.verifying = None;
        if !slot.wanted
            || slot.counter != counter
            || current_channel != Some(key.channel_id.as_str())
            || current_session != Some(secret.session_id.as_str())
            || !permitted
            || slot.candidate(&secret.user_id).as_ref() != Some(secret)
        {
            return None;
        }
        slot.revision = slot.revision.checked_add(1)?;
        slot.released = Some(secret.clone());
        slot.refresh_requested = false;
        slot.phase = Phase::Active;
        Some(slot.revision)
    }
    pub fn forget(&mut self, key: &RoomKey) -> bool {
        if !self.slot(key).is_some_and(|s| !s.wanted && s.left()) {
            return false;
        }
        let slot = self.slots.remove(&key.server_id).expect("matched slot");
        if let Some(server) = slot.server {
            self.retire(&key.server_id, &server.token);
        }
        if let Some(released) = slot.released {
            self.retire(&key.server_id, &released.token);
        }
        true
    }
    fn retire(&mut self, guild: &str, token: &str) {
        if token.is_empty() || self.retired.iter().any(|(g, t)| g == guild && t == token) {
            return;
        }
        if self.retired.len() == RETIRED_LIMIT {
            self.exhausted = true;
            return;
        }
        self.retired.push_back((guild.into(), token.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(intent: u64, channel: &str) -> RoomKey {
        RoomKey {
            destination_id: "discord-1".into(),
            intent,
            server_id: "1".into(),
            channel_id: channel.into(),
        }
    }
    fn joining() -> (Correlator, RoomKey) {
        let mut c = Correlator::default();
        let k = key(1, "2");
        c.reserve(k.clone()).unwrap();
        assert!(c.preflight(&k, 0, true));
        assert!(c.mark_join_sent(&k));
        (c, k)
    }
    #[test]
    fn both_pair_orders_need_current_membership_and_counter_fence() {
        for server_first in [true, false] {
            let (mut c, k) = joining();
            if server_first {
                c.server("1", Some("host"), "secret");
                c.state("1", Some("2"), "session", true);
            } else {
                c.state("1", Some("2"), "session", true);
                c.server("1", Some("host"), "secret");
            }
            let (counter, secret) = c.begin_verification(&k, "3").unwrap();
            assert_eq!(
                c.verify(&k, counter, &secret, Some("wrong"), Some("session"), true),
                None
            );
            let (counter, secret) = c.begin_verification(&k, "3").unwrap();
            c.state("1", Some("2"), "session", true);
            assert_eq!(
                c.verify(&k, counter, &secret, Some("2"), Some("session"), true),
                None
            );
            let (counter, secret) = c.begin_verification(&k, "3").unwrap();
            assert_eq!(
                c.verify(&k, counter, &secret, Some("2"), Some("session"), true),
                Some(1)
            );
        }
    }
    #[test]
    fn late_join_state_invalidates_an_earlier_leave_receipt() {
        let (mut c, k) = joining();
        assert!(!c.cancel(&k));
        c.leave_sent(&k);
        assert_eq!(c.state("1", None, "", true), StateAction::Ignore);
        assert!(!c.forget(&k));
        assert_eq!(
            c.state("1", Some("2"), "old-session", true),
            StateAction::LeaveAgain
        );
        c.server("1", Some("old-host"), "old-token");
        assert!(!c.forget(&k));
        c.leave_sent(&k);
        assert_eq!(c.state("1", None, "", true), StateAction::Left);
        assert!(c.forget(&k));
        let next = key(2, "4");
        c.reserve(next.clone()).unwrap();
        c.preflight(&next, 0, true);
        c.mark_join_sent(&next);
        assert_eq!(
            c.server("1", Some("old-host"), "old-token"),
            ServerAction::Ignore
        );
        assert!(c.begin_verification(&next, "3").is_none());
    }
    #[test]
    fn cancellation_before_send_and_old_keys_cannot_resurrect_intent() {
        let mut c = Correlator::default();
        let k = key(1, "2");
        c.reserve(k.clone()).unwrap();
        assert!(c.cancel(&k));
        assert!(!c.mark_join_sent(&k));
        assert!(c.forget(&k));
        let next = key(2, "4");
        c.reserve(next.clone()).unwrap();
        assert!(!c.preflight(&k, 0, true));
        assert!(!c.mark_join_sent(&k));
    }
    #[test]
    fn unexpected_move_or_null_never_changes_the_requested_room() {
        let (mut c, k) = joining();
        assert_eq!(
            c.state("1", Some("99"), "session", true),
            StateAction::Failed(Failure::Permission)
        );
        assert_eq!(c.slot(&k).unwrap().key.channel_id, "2");
        assert_eq!(
            c.state("1", None, "", true),
            StateAction::Failed(Failure::Permission)
        );
    }
    #[test]
    fn preflight_observation_cannot_discharge_a_later_join_handshake() {
        let mut c = Correlator::default();
        let k = key(1, "2");
        c.reserve(k.clone()).unwrap();
        c.state("1", Some("2"), "previous-session", true);
        c.state("1", None, "", true);
        assert!(c.preflight(&k, 2, true));
        assert!(c.mark_join_sent(&k));
        assert!(!c.cancel(&k));
        c.leave_sent(&k);
        c.server("1", Some("host"), "new-token");
        assert_eq!(c.state("1", None, "", true), StateAction::Ignore);
        assert!(!c.forget(&k));
        assert_eq!(
            c.state("1", Some("2"), "new-session", true),
            StateAction::LeaveAgain
        );
        c.leave_sent(&k);
        assert_eq!(c.state("1", None, "", true), StateAction::Left);
        assert!(c.forget(&k));
    }
    #[test]
    fn suspended_endpoint_retires_the_previous_credential() {
        let (mut c, k) = joining();
        c.state("1", Some("2"), "session", true);
        c.server("1", Some("host"), "old-token");
        let (counter, secret) = c.begin_verification(&k, "3").unwrap();
        assert_eq!(
            c.verify(&k, counter, &secret, Some("2"), Some("session"), true),
            Some(1)
        );
        assert_eq!(
            c.server("1", None, "suspended-token"),
            ServerAction::Reconfigure
        );
        assert_eq!(
            c.server("1", Some("host"), "old-token"),
            ServerAction::Ignore
        );
        assert!(c.begin_verification(&k, "3").is_none());
        assert_eq!(
            c.server("1", Some("new-host"), "suspended-token"),
            ServerAction::Reconfigure
        );
        assert!(c.begin_verification(&k, "3").is_some());
    }
    #[test]
    fn guild_membership_preflight_cannot_overwrite_newer_state() {
        let mut c = Correlator::default();
        let k = key(1, "2");
        c.reserve(k.clone()).unwrap();
        c.state("1", Some("88"), "other", true);
        assert!(!c.preflight(&k, 0, true));
        assert!(!c.preflight(&k, 1, false));
    }
    #[test]
    fn transport_refresh_rechecks_current_credentials_with_a_new_counter_and_revision() {
        let mut c = Correlator::default();
        let k = key(1, "2");
        c.reserve(k.clone()).unwrap();
        c.preflight(&k, 0, true);
        c.mark_join_sent(&k);
        c.state("1", Some("2"), "session", true);
        c.server("1", Some("host"), "token");
        let (first_counter, secret) = c.begin_verification(&k, "3").unwrap();
        assert_eq!(
            c.verify(&k, first_counter, &secret, Some("2"), Some("session"), true),
            Some(1)
        );
        assert!(c.begin_verification(&k, "3").is_none());
        assert!(c.request_refresh(&k));
        assert!(!c.request_refresh(&k));
        let (counter, repeated) = c.begin_verification(&k, "3").unwrap();
        assert!(counter > first_counter);
        assert_eq!(secret, repeated);
        c.state("1", Some("2"), "session", true);
        assert!(
            c.verify(&k, counter, &repeated, Some("2"), Some("session"), true)
                .is_none()
        );
        let (counter, repeated) = c.begin_verification(&k, "3").unwrap();
        assert_eq!(
            c.verify(&k, counter, &repeated, Some("2"), Some("session"), true),
            Some(2)
        );
        assert!(c.begin_verification(&k, "3").is_none());
        c.cancel(&k);
        assert!(!c.request_refresh(&k));
        assert!(c.begin_verification(&k, "3").is_none());
    }
    #[test]
    fn refresh_never_revives_retired_tokens_or_an_old_room_key() {
        let mut c = Correlator::default();
        let k = key(1, "2");
        c.reserve(k.clone()).unwrap();
        c.preflight(&k, 0, true);
        c.mark_join_sent(&k);
        c.state("1", Some("2"), "session", true);
        c.server("1", Some("host"), "old-token");
        let (counter, secret) = c.begin_verification(&k, "3").unwrap();
        c.verify(&k, counter, &secret, Some("2"), Some("session"), true);
        assert!(!c.request_refresh(&key(2, "2")));
        c.server("1", None, "next-token");
        assert!(!c.request_refresh(&k));
        assert_eq!(
            c.server("1", Some("host"), "old-token"),
            ServerAction::Ignore
        );
        assert!(c.begin_verification(&k, "3").is_none());
    }
}
