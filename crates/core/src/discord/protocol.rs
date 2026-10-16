//! Backend-only room and voice-worker messages. Never expose these to TypeScript.
use crate::{
    model::{Capture, Output},
    protocol::Failure,
};
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct RoomKey {
    pub destination_id: String,
    pub intent: u64,
    pub server_id: String,
    pub channel_id: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VoiceSecret {
    pub endpoint: String,
    pub session_id: String,
    pub token: String,
    pub user_id: String,
}
impl fmt::Debug for VoiceSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VoiceSecret { <redacted> }")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GatewayCommand {
    Init { token_file: PathBuf },
    Join { key: RoomKey },
    Refresh { key: RoomKey },
    Leave { key: RoomKey },
    Forget { key: RoomKey },
    Stop,
}
impl fmt::Debug for GatewayCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GatewayCommand { <private> }")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GatewayStatus {
    Ready,
    Recovery {
        active: bool,
    },
    Allowance {
        remaining: u64,
        reset_after_ms: u64,
    },
    Joining {
        key: RoomKey,
    },
    Credentials {
        key: RoomKey,
        revision: u32,
        secret: VoiceSecret,
    },
    Reconfiguring {
        key: RoomKey,
    },
    Health {
        key: RoomKey,
        recovering: bool,
    },
    Left {
        key: RoomKey,
    },
    RoomFailed {
        key: RoomKey,
        code: Failure,
    },
    CleanupIncomplete {
        key: RoomKey,
    },
    Ping {
        milliseconds: Option<f32>,
    },
    Failed {
        code: Failure,
    },
    Stopped,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)] // bounded private queue, keeping Init contiguous avoids another allocation
pub enum VoiceCommand {
    Init {
        key: RoomKey,
        gateway_epoch: u32,
        credential_revision: u32,
        source_id: String,
        secret: VoiceSecret,
        output: Output,
    },
    Output {
        output: Output,
    },
    Stop,
}
impl fmt::Debug for VoiceCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VoiceCommand { <private> }")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum VoiceStatus {
    Connected,
    RefreshRequired,
    Meter { capture: Capture },
    Failed { code: Failure },
    Stopped,
}

pub fn valid_key(key: &RoomKey) -> bool {
    !key.destination_id.is_empty()
        && key.destination_id.len() <= 64
        && key.intent != 0
        && crate::protocol::discord_id(&key.server_id).is_some()
        && crate::protocol::discord_id(&key.channel_id).is_some()
}
pub fn valid_secret(secret: &VoiceSecret) -> bool {
    !secret.endpoint.is_empty()
        && secret.endpoint.len() <= 512
        && !secret.endpoint.chars().any(char::is_control)
        && !secret.session_id.is_empty()
        && secret.session_id.len() <= 1024
        && !secret.session_id.chars().any(char::is_control)
        && !secret.token.is_empty()
        && secret.token.len() <= 4096
        && !secret.token.chars().any(char::is_control)
        && crate::protocol::discord_id(&secret.user_id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_credentials_cannot_escape_debug_or_public_contract() {
        let secret = VoiceSecret {
            endpoint: "endpoint-canary".into(),
            session_id: "session-canary".into(),
            token: "token-canary".into(),
            user_id: "1".into(),
        };
        let key = RoomKey {
            destination_id: "discord-1".into(),
            intent: 1,
            server_id: "2".into(),
            channel_id: "3".into(),
        };
        let status = GatewayStatus::Credentials {
            key,
            revision: 1,
            secret,
        };
        let text = format!("{status:?}");
        for value in ["endpoint-canary", "session-canary", "token-canary"] {
            assert!(!text.contains(value));
            assert!(!crate::model::typescript_contract().contains(value));
        }
        let public = serde_json::to_string(&crate::model::ControlSnapshot::default()).unwrap();
        assert!(!public.contains("credentialRevision"));
        assert!(!public.contains("token"));
    }
}
