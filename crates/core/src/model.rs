use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    #[default]
    Recording,
    Application,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSource {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: SourceKind,
    pub available: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct VoiceChannel {
    pub id: String,
    pub name: String,
    pub category_name: Option<String>,
    pub position: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    pub name: String,
    pub icon_url: Option<String>,
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
#[serde(rename_all = "camelCase")]
pub struct BotProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub avatar_url: Option<String>,
    pub status: BotStatus,
    pub name: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum Discovery {
    #[default]
    Idle,
    Loading,
    Ready,
    Error {
        message: String,
    },
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
    EngineStopped,
    ConnectionFailed,
    CleanupIncomplete,
    Internal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

/// Observed work currently in progress, never a percentage or estimated timer.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectStage {
    Checking,
    Preflight,
    Gateway,
    Join,
    Capture,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum Connection {
    Disconnected,
    Connecting { stage: ConnectStage },
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
            Self::Connecting { .. }
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
    // Pre-gain capture callback peak; unavailable before capture starts.
    pub level_dbfs: Option<f32>,
    // Post-gain/mute stereo peak at the native PCM reader handoff.
    // Silence is -96 dBFS; unavailable observations are null.
    pub output_level_dbfs: Option<f32>,
    pub message: Option<String>,
    pub dropped_frames: u32,
    pub underrun_frames: u32,
    pub queued_frames: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub volume_linear: Option<f32>,
    pub volume_percent: u8,
    pub muted: bool,
}

/// One fixed native mix supports at most eight explicitly selected inputs.
pub const MAX_INPUTS: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InputPhase {
    Stopped,
    Opening,
    Capturing,
    Fault,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct InputStrip {
    pub strip_id: String,
    pub source_id: String,
    pub kind: SourceKind,
    pub display_name: String,
    pub detail: Option<String>,
    pub phase: InputPhase,
    pub fault: Option<AppError>,
    /// null means Off; otherwise -60..=0 dB, with fractional precision.
    pub gain_db: Option<f32>,
    pub muted: bool,
    pub colour_slot: u8,
    pub restored: bool,
}

pub const MAX_PLAYBACK_DESTINATIONS: usize = 3;
pub const MAX_DESTINATIONS: usize = 4;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DestinationKind {
    #[default]
    Discord,
    Playback,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackDevice {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct DestinationTelemetry {
    pub session_id: u32,
    pub destination_id: String,
    pub level_dbfs: Option<f32>,
    pub queued_frames: u32,
    pub dropped_frames: u32,
    pub underrun_frames: u32,
    pub gateway_ping_ms: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct DestinationStrip {
    #[serde(default)]
    pub session_id: u32,
    #[serde(default)]
    pub kind: DestinationKind,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    pub destination_id: String,
    pub server_id: String,
    pub channel_id: String,
    pub connection: Connection,
    pub volume_db: Option<f32>,
    pub muted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    pub restore_setup: bool,
    pub keep_in_tray: bool,
    pub meter_style: MeterStyle,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MeterStyle {
    Simple,
    History,
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            restore_setup: true,
            keep_in_tray: true,
            meter_style: MeterStyle::Simple,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct InputTelemetry {
    pub strip_id: String,
    pub level_dbfs: Option<f32>,
    pub pre_clip: bool,
    pub dropped_frames: u32,
    pub underrun_frames: u32,
    pub queued_frames: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct MixTelemetry {
    #[serde(default)]
    pub destinations: Vec<DestinationTelemetry>,
    #[serde(default)]
    pub mix_pre_clip: bool,
    pub inputs: Vec<InputTelemetry>,
    pub output_level_dbfs: Option<f32>,
    pub queued_frames: u32,
    pub dropped_frames: u32,
    pub underrun_frames: u32,
    /// Unavailable until measured; never a simulated voice-network statistic.
    pub gateway_ping_ms: Option<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayPhase {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Recovering,
    Disconnecting,
    CleanupIncomplete,
    Error,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySnapshot {
    pub phase: GatewayPhase,
    /// Application-owned gateway lifetime, never Discord's private session ID.
    pub generation: u32,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ReservationPhase {
    Selected,
    Joining,
    Connected,
    Leaving,
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerReservation {
    pub server_id: String,
    pub channel_id: String,
    pub destination_id: String,
    pub session_id: u32,
    pub phase: ReservationPhase,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct ControlSnapshot {
    #[serde(default)]
    pub gateway: GatewaySnapshot,
    #[serde(default)]
    pub server_reservations: Vec<ServerReservation>,
    #[serde(default)]
    pub playback_devices: Vec<PlaybackDevice>,
    #[serde(default)]
    pub playback_discovery: Discovery,
    #[serde(default)]
    pub inputs: Vec<InputStrip>,
    #[serde(default)]
    pub destinations: Vec<DestinationStrip>,
    #[serde(default)]
    pub settings: AppSettings,
    /// Live controls were applied, but the current setup has not been saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub preferences_warning: Option<AppError>,
    /// A bounded background writer is saving the latest live setup.
    #[serde(default)]
    pub preferences_saving: bool,
    #[serde(default)]
    pub telemetry: MixTelemetry,
    pub revision: u32,
    pub session_id: u32,
    pub sources: Vec<RecordingSource>,
    pub servers: Vec<Server>,
    pub selection: Selection,
    pub bot: BotProfile,
    pub discovery: Discovery,
    #[serde(default)]
    pub source_discovery: Discovery,
    pub connection: Connection,
    pub capture: Capture,
    pub output: Output,
}

/// Borrowed control-event comparison state. Exhaustive destructuring below
/// requires new snapshot/capture fields to be classified explicitly.
#[derive(PartialEq)]
struct ControlEventKey<'a> {
    gateway: &'a GatewaySnapshot,
    server_reservations: &'a [ServerReservation],
    playback_devices: &'a [PlaybackDevice],
    playback_discovery: &'a Discovery,
    session_id: u32,
    inputs: &'a [InputStrip],
    destinations: &'a [DestinationStrip],
    settings: &'a AppSettings,
    preferences_warning: &'a Option<AppError>,
    preferences_saving: bool,
    sources: &'a [RecordingSource],
    servers: &'a [Server],
    selection: &'a Selection,
    bot: &'a BotProfile,
    discovery: &'a Discovery,
    source_discovery: &'a Discovery,
    connection: &'a Connection,
    capture_phase: &'a CapturePhase,
    capture_message: &'a Option<String>,
    output: &'a Output,
}

impl ControlSnapshot {
    fn control_event_key(&self) -> ControlEventKey<'_> {
        let Self {
            gateway,
            server_reservations,
            playback_devices,
            playback_discovery,
            revision: _,
            telemetry: _,
            session_id,
            inputs,
            destinations,
            settings,
            preferences_warning,
            preferences_saving,
            sources,
            servers,
            selection,
            bot,
            discovery,
            source_discovery,
            connection,
            output,
            capture:
                Capture {
                    phase,
                    message,
                    level_dbfs: _,
                    output_level_dbfs: _,
                    dropped_frames: _,
                    underrun_frames: _,
                    queued_frames: _,
                },
        } = self;
        ControlEventKey {
            gateway,
            server_reservations,
            playback_devices,
            playback_discovery,
            session_id: *session_id,
            inputs,
            destinations,
            settings,
            preferences_warning,
            preferences_saving: *preferences_saving,
            sources,
            servers,
            selection,
            bot,
            discovery,
            source_discovery,
            connection,
            capture_phase: phase,
            capture_message: message,
            output,
        }
    }

    /// Meter-only observations use the telemetry event. This comparison borrows
    /// the full snapshots and never alters the measurements in an emitted payload.
    pub fn same_control_state(&self, other: &Self) -> bool {
        self.control_event_key() == other.control_event_key()
    }
}

impl Default for ControlSnapshot {
    fn default() -> Self {
        Self {
            gateway: GatewaySnapshot::default(),
            server_reservations: Vec::new(),
            playback_devices: Vec::new(),
            playback_discovery: Discovery::Idle,
            inputs: Vec::new(),
            destinations: Vec::new(),
            settings: AppSettings::default(),
            preferences_warning: None,
            preferences_saving: false,
            telemetry: MixTelemetry::default(),
            revision: 0,
            session_id: 0,
            sources: Vec::new(),
            servers: Vec::new(),
            selection: Selection::default(),
            bot: BotProfile {
                id: None,
                avatar_url: None,
                status: BotStatus::Unconfigured,
                name: None,
                message: None,
            },
            discovery: Discovery::Idle,
            source_discovery: Discovery::Idle,
            connection: Connection::Disconnected,
            capture: Capture {
                phase: CapturePhase::Stopped,
                level_dbfs: None,
                output_level_dbfs: None,
                message: None,
                dropped_frames: 0,
                underrun_frames: 0,
                queued_frames: 0,
            },
            output: Output {
                volume_linear: None,
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
    RetryPreferences,
    RefreshPlaybackDevices,
    AddPlaybackDestination {
        device_id: String,
    },
    ChangePlaybackDestination {
        destination_id: String,
        device_id: String,
    },
    SetAllDestinationsMuted {
        muted: bool,
    },
    DisconnectAllDestinations,
    AddSource {
        source_id: String,
    },
    RemoveSource {
        strip_id: String,
    },
    ReplaceSource {
        strip_id: String,
        source_id: String,
    },
    StartCapture {
        strip_id: String,
    },
    StopCapture {
        strip_id: String,
    },
    SetSourceMuted {
        strip_id: String,
        muted: bool,
    },
    SetSourceGain {
        strip_id: String,
        gain_db: Option<f32>,
    },
    AddDestination {
        server_id: String,
        channel_id: String,
    },
    RemoveDestination {
        destination_id: String,
    },
    ChangeDestination {
        destination_id: String,
        server_id: String,
        channel_id: String,
    },
    ConnectDestination {
        destination_id: String,
    },
    DisconnectDestination {
        destination_id: String,
    },
    SetDestinationVolume {
        destination_id: String,
        volume_db: Option<f32>,
    },
    SetDestinationMuted {
        destination_id: String,
        muted: bool,
    },
    SetSettings {
        settings: AppSettings,
    },
    RefreshSources,
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
        InputPhase::decl(&config),
        InputStrip::decl(&config),
        DestinationKind::decl(&config),
        PlaybackDevice::decl(&config),
        DestinationTelemetry::decl(&config),
        DestinationStrip::decl(&config),
        AppSettings::decl(&config),
        MeterStyle::decl(&config),
        InputTelemetry::decl(&config),
        MixTelemetry::decl(&config),
        SourceKind::decl(&config),
        RecordingSource::decl(&config),
        VoiceChannel::decl(&config),
        Server::decl(&config),
        Selection::decl(&config),
        BotStatus::decl(&config),
        BotProfile::decl(&config),
        Discovery::decl(&config),
        ErrorCode::decl(&config),
        AppError::decl(&config),
        ConnectStage::decl(&config),
        Connection::decl(&config),
        CapturePhase::decl(&config),
        Capture::decl(&config),
        Output::decl(&config),
        GatewayPhase::decl(&config),
        GatewaySnapshot::decl(&config),
        ReservationPhase::decl(&config),
        ServerReservation::decl(&config),
        ControlSnapshot::decl(&config),
        ControlCommand::decl(&config),
    ];
    let mut result = String::from("// Generated from Rust model types. Do not edit by hand.\n");
    for declaration in declarations {
        result.push_str("export ");
        result.push_str(
            &declaration
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_fields_use_defaults_without_discarding_existing_choices() {
        let settings: AppSettings = serde_json::from_str(r#"{"keepInTray":false}"#).unwrap();
        assert!(!settings.keep_in_tray);
        assert!(settings.restore_setup);
        assert_eq!(settings.meter_style, MeterStyle::Simple);
        assert_eq!(
            serde_json::from_str::<AppSettings>("{}").unwrap(),
            AppSettings::default()
        );
        assert!(serde_json::from_str::<AppSettings>(r#"{"meterStyle":"unknown"}"#).is_err());
    }

    #[test]
    fn meter_bursts_do_not_change_control_state_or_erase_snapshot_measurements() {
        let first = ControlSnapshot::default();
        for index in 1..=1000 {
            let mut measured = first.clone();
            measured.revision = index;
            measured.capture.level_dbfs = Some(-12.0);
            measured.capture.output_level_dbfs = Some(-18.0);
            measured.capture.dropped_frames = index;
            measured.capture.underrun_frames = index * 2;
            measured.capture.queued_frames = index % 1920;
            measured.telemetry = MixTelemetry {
                destinations: Vec::new(),
                mix_pre_clip: true,
                inputs: vec![InputTelemetry {
                    strip_id: "input-1".into(),
                    level_dbfs: Some(-6.0),
                    pre_clip: true,
                    dropped_frames: index,
                    underrun_frames: index,
                    queued_frames: 240,
                }],
                output_level_dbfs: Some(-18.0),
                queued_frames: 480,
                dropped_frames: index,
                underrun_frames: index,
                gateway_ping_ms: Some(12.5),
            };
            let payload = serde_json::to_value(&measured).unwrap();
            assert!(first.same_control_state(&measured));
            assert_eq!(serde_json::to_value(&measured).unwrap(), payload);
            assert_eq!(measured.telemetry.output_level_dbfs, Some(-18.0));
        }
    }

    #[test]
    fn control_state_keeps_lifecycle_identity_settings_and_catalog_changes() {
        let first = ControlSnapshot::default();
        let changes: [fn(&mut ControlSnapshot); 15] = [
            |s| s.session_id = 1,
            |s| s.capture.phase = CapturePhase::Opening,
            |s| s.capture.message = Some("Capture stopped.".into()),
            |s| s.connection = Connection::Connected,
            |s| s.settings.meter_style = MeterStyle::History,
            |s| s.output.muted = true,
            |s| s.bot.id = Some("1234567890123456789".into()),
            |s| s.bot.avatar_url = Some("https://cdn.discordapp.com/avatars/1/avatar.png".into()),
            |s| s.selection.source_id = Some("recording".into()),
            |s| s.source_discovery = Discovery::Loading,
            |s| s.discovery = Discovery::Loading,
            |s| {
                s.servers.push(Server {
                    id: "1".into(),
                    name: "Server".into(),
                    icon_url: None,
                    channels: Vec::new(),
                })
            },
            |s| {
                s.sources.push(RecordingSource {
                    id: "recording".into(),
                    name: "Input".into(),
                    kind: SourceKind::Recording,
                    available: true,
                    detail: None,
                })
            },
            |s| {
                s.inputs.push(InputStrip {
                    strip_id: "input-1".into(),
                    source_id: "recording".into(),
                    kind: SourceKind::Recording,
                    display_name: "Input".into(),
                    detail: None,
                    phase: InputPhase::Capturing,
                    fault: None,
                    gain_db: Some(-6.0),
                    muted: true,
                    colour_slot: 0,
                    restored: false,
                })
            },
            |s| {
                s.destinations.push(DestinationStrip {
                    session_id: 0,
                    kind: DestinationKind::Discord,
                    device_id: None,
                    display_name: None,
                    destination_id: "discord-1".into(),
                    server_id: "1".into(),
                    channel_id: "2".into(),
                    connection: Connection::Disconnected,
                    volume_db: Some(-6.0),
                    muted: true,
                })
            },
        ];
        for change in changes {
            let mut changed = first.clone();
            change(&mut changed);
            assert!(!first.same_control_state(&changed));
        }
    }

    #[test]
    fn legacy_source_descriptors_default_to_recording() {
        let source: RecordingSource = serde_json::from_str(
            r#"{"id":"recording","name":"Recording","available":true,"detail":null}"#,
        )
        .unwrap();
        assert_eq!(source.kind, SourceKind::Recording);
        assert_eq!(
            serde_json::to_value(ControlCommand::RefreshSources).unwrap()["type"],
            "refresh-sources"
        );
    }

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

    #[test]
    fn connection_progress_and_rest_catalog_fields_have_explicit_wire_shapes() {
        for (stage, name) in [
            (ConnectStage::Checking, "checking"),
            (ConnectStage::Preflight, "preflight"),
            (ConnectStage::Gateway, "gateway"),
            (ConnectStage::Join, "join"),
            (ConnectStage::Capture, "capture"),
        ] {
            let connection = Connection::Connecting { stage };
            assert!(connection.locks_route());
            assert_eq!(
                serde_json::to_value(connection).unwrap(),
                serde_json::json!({"phase":"connecting", "stage":name})
            );
        }
        let server = Server {
            id: "1".into(),
            name: "Server".into(),
            icon_url: None,
            channels: vec![VoiceChannel {
                id: "2".into(),
                name: "Voice".into(),
                category_name: Some("Games".into()),
                position: 3,
            }],
        };
        let value = serde_json::to_value(server).unwrap();
        assert!(value["iconUrl"].is_null());
        assert_eq!(value["channels"][0]["categoryName"], "Games");
        assert_eq!(value["channels"][0]["position"], 3);
    }
}
