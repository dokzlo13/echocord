import type {
  AppSettings,
  BotStatus,
  Connection,
  InputStrip,
  Server as NativeServer,
  RecordingSource,
} from "./generated";

export type MeterMode = "simple" | "history";
export type Glyph = "application" | "cable";
export interface CatalogSource extends RecordingSource {
  glyph: Glyph;
  tint: string;
  note?: string | undefined;
  disabledReason?: string | undefined;
}
export interface Channel {
  id: string;
  name: string;
  category: string;
}
export interface Server {
  id: string;
  name: string;
  initials: string;
  tint: string;
  iconUrl: string | null;
  channels: Channel[];
}
export type SourceState = InputStrip["phase"];
export interface Fault {
  title: string;
  actionLabel: string;
}
export interface SourceCard {
  uid: string;
  sourceId: string;
  identity: CatalogSource;
  colour: number;
  state: SourceState;
  muted: boolean;
  gainDb: number | null;
  fault?: Fault | undefined;
  restored: boolean;
}
export type DestState =
  | "idle"
  | "connecting"
  | "live"
  | "recovering"
  | "disconnecting"
  | "fault"
  | "cleanup-incomplete";
export type Stage = "checking" | "gateway" | "joining" | "starting";
export interface DestCard {
  uid: string;
  serverId: string;
  channelId: string;
  server: Server;
  channel: Channel;
  state: DestState;
  connection: Connection;
  stage?: Stage | undefined;
  attempt?: number | undefined;
  volumeDb: number | null;
  muted: boolean;
  fault?: Fault | undefined;
  confirmRemove: boolean;
  op: number;
  liveAt?: number | undefined;
}
export type BotState = BotStatus;
export type Sheet =
  | { kind: "source"; replaceUid?: string | undefined }
  | { kind: "room"; replaceUid?: string | undefined }
  | { kind: "bot"; next?: "room" }
  | { kind: "settings" }
  | { kind: "bot-info" }
  | { kind: "net"; uid: string };
export function catalogSource(s: RecordingSource): CatalogSource {
  return {
    ...s,
    glyph: s.kind === "application" ? "application" : "cable",
    tint: "#6b7f8e",
    disabledReason: s.available
      ? undefined
      : (s.detail ?? "This source is unavailable."),
    note:
      s.kind === "application"
        ? "Captures this application and its child processes. Browser capture includes its tabs and windows; keep your call in another app."
        : undefined,
  };
}
export function serverView(s: NativeServer): Server {
  return {
    ...s,
    initials: s.name
      .trim()
      .split(/\s+/)
      .slice(0, 2)
      .map((x) => x[0])
      .join("")
      .toUpperCase(),
    tint: "#4f6f8f",
    channels: s.channels.map((c) => ({
      id: c.id,
      name: c.name,
      category: c.categoryName ?? "Voice rooms",
    })),
  };
}
export type Settings = AppSettings;
