// Generated from Rust model types. Do not edit by hand.
export type InputPhase = "stopped" | "opening" | "capturing" | "fault";
export type InputStrip = { stripId: string, sourceId: string, kind: SourceKind, displayName: string, detail: string | null, phase: InputPhase, fault: AppError | null, 
/**
 * null means Off; otherwise -60..=0 dB, with fractional precision.
 */
gainDb: number | null, muted: boolean, colourSlot: number, restored: boolean, };
export type DestinationStrip = { destinationId: string, serverId: string, channelId: string, connection: Connection, volumeDb: number | null, muted: boolean, };
export type AppSettings = { restoreSetup: boolean, keepInTray: boolean, meterStyle: MeterStyle, };
export type MeterStyle = "simple" | "history";
export type InputTelemetry = { stripId: string, levelDbfs: number | null, preClip: boolean, droppedFrames: number, underrunFrames: number, queuedFrames: number, };
export type MixTelemetry = { inputs: Array<InputTelemetry>, outputLevelDbfs: number | null, queuedFrames: number, droppedFrames: number, underrunFrames: number, 
/**
 * Unavailable until measured; never a simulated voice-network statistic.
 */
gatewayPingMs: number | null, };
export type SourceKind = "recording" | "application";
export type RecordingSource = { id: string, name: string, kind: SourceKind, available: boolean, detail: string | null, };
export type VoiceChannel = { id: string, name: string, categoryName: string | null, position: number, };
export type Server = { id: string, name: string, iconUrl: string | null, channels: Array<VoiceChannel>, };
export type Selection = { sourceId: string | null, serverId: string | null, channelId: string | null, };
export type BotStatus = "unconfigured" | "checking" | "ready" | "invalid" | "error";
export type BotProfile = { status: BotStatus, name: string | null, message: string | null, };
export type Discovery = { "phase": "idle" } | { "phase": "loading" } | { "phase": "ready" } | { "phase": "error", message: string, };
export type ErrorCode = "invalid-selection" | "invalid-volume" | "busy" | "configuration" | "authentication" | "permission-denied" | "capture-failed" | "connection-failed" | "cleanup-incomplete" | "internal";
export type AppError = { code: ErrorCode, message: string, };
export type ConnectStage = "checking" | "preflight" | "gateway" | "join" | "capture";
export type Connection = { "phase": "disconnected" } | { "phase": "connecting", stage: ConnectStage, } | { "phase": "connected" } | { "phase": "recovering", attempt: number, message: string, } | { "phase": "disconnecting" } | { "phase": "cleanup-incomplete", message: string, } | { "phase": "error", code: ErrorCode, message: string, };
export type CapturePhase = "stopped" | "opening" | "running" | "missing" | "error";
export type Capture = { phase: CapturePhase, levelDbfs: number | null, outputLevelDbfs: number | null, message: string | null, droppedFrames: number, underrunFrames: number, queuedFrames: number, };
export type Output = { volumePercent: number, muted: boolean, };
export type ControlSnapshot = { inputs: Array<InputStrip>, destinations: Array<DestinationStrip>, settings: AppSettings, telemetry: MixTelemetry, revision: number, sessionId: number, sources: Array<RecordingSource>, servers: Array<Server>, selection: Selection, bot: BotProfile, discovery: Discovery, sourceDiscovery: Discovery, connection: Connection, capture: Capture, output: Output, };
export type ControlCommand = { "type": "add-source", sourceId: string, } | { "type": "remove-source", stripId: string, } | { "type": "replace-source", stripId: string, sourceId: string, } | { "type": "start-capture", stripId: string, } | { "type": "stop-capture", stripId: string, } | { "type": "set-source-muted", stripId: string, muted: boolean, } | { "type": "set-source-gain", stripId: string, gainDb: number | null, } | { "type": "add-destination", serverId: string, channelId: string, } | { "type": "remove-destination", destinationId: string, } | { "type": "change-destination", destinationId: string, serverId: string, channelId: string, } | { "type": "connect-destination", destinationId: string, } | { "type": "disconnect-destination", destinationId: string, } | { "type": "set-destination-volume", destinationId: string, volumeDb: number | null, } | { "type": "set-destination-muted", destinationId: string, muted: boolean, } | { "type": "set-settings", settings: AppSettings, } | { "type": "refresh-sources" } | { "type": "refresh-catalog" } | { "type": "recheck-configuration" } | { "type": "select-source", sourceId: string, } | { "type": "select-destination", serverId: string, channelId: string | null, } | { "type": "connect" } | { "type": "disconnect" } | { "type": "set-volume", percent: number, } | { "type": "set-muted", muted: boolean, };
