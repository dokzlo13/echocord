// Generated from Rust model types. Do not edit by hand.
export type RecordingSource = { id: string, name: string, available: boolean, detail: string | null, };
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
export type Capture = { phase: CapturePhase, levelDbfs: number | null, message: string | null, droppedFrames: number, underrunFrames: number, queuedFrames: number, };
export type Output = { volumePercent: number, muted: boolean, };
export type ControlSnapshot = { revision: number, sessionId: number, sources: Array<RecordingSource>, servers: Array<Server>, selection: Selection, bot: BotProfile, discovery: Discovery, connection: Connection, capture: Capture, output: Output, };
export type ControlCommand = { "type": "refresh-catalog" } | { "type": "recheck-configuration" } | { "type": "select-source", sourceId: string, } | { "type": "select-destination", serverId: string, channelId: string | null, } | { "type": "connect" } | { "type": "disconnect" } | { "type": "set-volume", percent: number, } | { "type": "set-muted", muted: boolean, };
