use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSource {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
pub struct VoiceChannel {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub channels: Vec<VoiceChannel>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub source_id: Option<String>,
    pub server_id: Option<String>,
    pub channel_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum BotStatus {
    Unconfigured,
    Checking,
    Ready,
    Invalid,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
pub struct BotProfile {
    pub status: BotStatus,
    pub name: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum Discovery {
    Idle,
    Loading,
    Ready,
    Error { message: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    InvalidSelection,
    InvalidVolume,
    Busy,
    Configuration,
    Authentication,
    PermissionDenied,
    CaptureFailed,
    ConnectionFailed,
    CleanupIncomplete,
    Internal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum Connection {
    Disconnected,
    Connecting,
    Connected,
    Recovering { attempt: u8, message: String },
    Disconnecting,
    CleanupIncomplete { message: String },
    Error { code: ErrorCode, message: String },
}

impl Connection {
    pub fn locks_route(&self) -> bool {
        matches!(
            self,
            Self::Connecting
                | Self::Connected
                | Self::Recovering { .. }
                | Self::Disconnecting
                | Self::CleanupIncomplete { .. }
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CapturePhase {
    Stopped,
    Opening,
    Running,
    Missing,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    pub phase: CapturePhase,
    pub level_dbfs: Option<f32>,
    pub message: Option<String>,
    pub dropped_frames: u32,
    pub underrun_frames: u32,
    pub queued_frames: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub volume_percent: u8,
    pub muted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct ControlSnapshot {
    pub revision: u32,
    pub session_id: u32,
    pub sources: Vec<RecordingSource>,
    pub servers: Vec<Server>,
    pub selection: Selection,
    pub bot: BotProfile,
    pub discovery: Discovery,
    pub connection: Connection,
    pub capture: Capture,
    pub output: Output,
}

impl Default for ControlSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            session_id: 0,
            sources: Vec::new(),
            servers: Vec::new(),
            selection: Selection::default(),
            bot: BotProfile {
                status: BotStatus::Unconfigured,
                name: None,
                message: None,
            },
            discovery: Discovery::Idle,
            connection: Connection::Disconnected,
            capture: Capture {
                phase: CapturePhase::Stopped,
                level_dbfs: None,
                message: None,
                dropped_frames: 0,
                underrun_frames: 0,
                queued_frames: 0,
            },
            output: Output {
                volume_percent: 100,
                muted: false,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ControlCommand {
    RefreshCatalog,
    RecheckConfiguration,
    SelectSource {
        source_id: String,
    },
    SelectDestination {
        server_id: String,
        channel_id: Option<String>,
    },
    Connect,
    Disconnect,
    SetVolume {
        percent: u8,
    },
    SetMuted {
        muted: bool,
    },
}

pub fn typescript_contract() -> String {
    let config = ts_rs::Config::default();
    let declarations = [
        RecordingSource::decl(&config),
        VoiceChannel::decl(&config),
        Server::decl(&config),
        Selection::decl(&config),
        BotStatus::decl(&config),
        BotProfile::decl(&config),
        Discovery::decl(&config),
        ErrorCode::decl(&config),
        AppError::decl(&config),
        Connection::decl(&config),
        CapturePhase::decl(&config),
        Capture::decl(&config),
        Output::decl(&config),
        ControlSnapshot::decl(&config),
        ControlCommand::decl(&config),
    ];
    let mut result = String::from("// Generated from Rust model types. Do not edit by hand.\n");
    for declaration in declarations {
        result.push_str("export ");
        result.push_str(&declaration);
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_wire_format_preserves_large_discord_ids_as_strings() {
        let json = r#"{"type":"select-destination","serverId":"1234567890123456789","channelId":"9876543210987654321"}"#;
        let command: ControlCommand = serde_json::from_str(json).unwrap();
        let value = serde_json::to_value(command).unwrap();
        assert_eq!(value["serverId"], "1234567890123456789");
        assert_eq!(value["channelId"], "9876543210987654321");
    }

    #[test]
    fn startup_is_unselected_disconnected_and_not_capturing() {
        let value = serde_json::to_value(ControlSnapshot::default()).unwrap();
        assert_eq!(value["connection"]["phase"], "disconnected");
        assert_eq!(value["capture"]["phase"], "stopped");
        assert!(value["capture"]["levelDbfs"].is_null());
        assert!(value["selection"]["sourceId"].is_null());
    }

    #[test]
    fn incomplete_cleanup_keeps_route_locked() {
        assert!(
            Connection::CleanupIncomplete {
                message: "cleanup pending".into()
            }
            .locks_route()
        );
        assert!(Connection::Disconnecting.locks_route());
        assert!(!Connection::Disconnected.locks_route());
    }
}
