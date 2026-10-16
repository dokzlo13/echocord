import type { BotStatus, DestinationKind, GatewayPhase } from "./generated";
import type { DestCard, DestState, SourceCard, Stage } from "./model";

/** Destination status words (On Air world, Precision-style badge). */
export const STATUS: Record<"off" | "standby" | "live" | "fault", string> = {
  off: "Off air",
  standby: "Joining",
  live: "On air",
  fault: "Fault",
};

export const statusKey = (
  s: DestState,
): "off" | "standby" | "live" | "fault" => {
  switch (s) {
    case "live":
      return "live";
    case "connecting":
    case "recovering":
    case "disconnecting":
      return "standby";
    case "cleanup-incomplete":
    case "fault":
      return "fault";
    default:
      return "off";
  }
};

export const STAGES: { id: Stage; label: string }[] = [
  { id: "checking", label: "Bot" },
  { id: "gateway", label: "Discord" },
  { id: "joining", label: "Room" },
  { id: "starting", label: "Audio" },
];

export const stageLine = (stage: Stage | undefined, room: string) => {
  switch (stage) {
    case "checking":
      return "Checking the bot…";
    case "gateway":
      return "Reaching Discord…";
    case "joining":
      return `Joining ${room}…`;
    case "starting":
      return "Starting audio…";
    default:
      return "Connecting…";
  }
};

export const GATEWAY_STATUS: Record<GatewayPhase, string> = {
  disconnected: "Disconnected",
  connecting: "Connecting",
  connected: "Connected",
  recovering: "Reconnecting",
  disconnecting: "Disconnecting",
  "cleanup-incomplete": "Cleanup unconfirmed",
  error: "Connection error",
};

export function destinationStatusLabel(
  state: DestState,
  kind: DestinationKind,
) {
  const local = kind === "playback";
  switch (state) {
    case "live":
      return local ? "Playing" : "On air";
    case "connecting":
      return local ? "Opening" : "Joining";
    case "recovering":
      return "Reconnecting";
    case "disconnecting":
      return local ? "Stopping" : "Leaving";
    case "cleanup-incomplete":
      return "Unconfirmed";
    case "fault":
      return "Fault";
    default:
      return local ? "Stopped" : "Off air";
  }
}

export function destinationChipState(card: DestCard) {
  return card.state === "live"
    ? card.kind === "playback"
      ? "playing"
      : "on"
    : card.state === "idle"
      ? "stopped"
      : card.state;
}

export function destinationSummaryText(cards: DestCard[]) {
  if (cards.length <= 1) return undefined;
  if (cards.every((card) => card.state === "idle")) return "All stopped";
  const counts = new Map<string, number>();
  for (const card of cards) {
    const label = destinationStatusLabel(card.state, card.kind).toLowerCase();
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return [
    "fault",
    "unconfirmed",
    "reconnecting",
    "joining",
    "opening",
    "leaving",
    "stopping",
    "on air",
    "playing",
  ]
    .filter((label) => counts.has(label))
    .map((label) => `${counts.get(label)} ${label}`)
    .join(" · ");
}

export function sourceSummaryText(cards: SourceCard[]) {
  const counts = new Map<string, number>();
  for (const card of cards) {
    const label =
      card.state === "capturing"
        ? card.muted
          ? "muted"
          : "on"
        : card.state === "stopped"
          ? "off"
          : card.state;
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return ["fault", "on", "muted", "opening", "off"]
    .filter((label) => counts.has(label))
    .map((label) =>
      cards.length === 1 ? label : `${counts.get(label)} ${label}`,
    )
    .join(" · ");
}

export function botStatusLabel(status: BotStatus, name: string) {
  switch (status) {
    case "ready":
      return name;
    case "checking":
      return "Checking bot";
    case "invalid":
      return "Token rejected";
    case "error":
      return "Bot check failed";
    default:
      return "Set up bot";
  }
}
