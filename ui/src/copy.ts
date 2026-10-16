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

/* --------------------------------------------------------- audio processing
 * Plain language for a wide audience. Settings shows no ms, dB or ppm; numbers
 * appear only as secondary detail in Sync details. Copy states what Echocord
 * applied or observed, never physical synchronization or audible results.
 */
export const AUDIO_PROCESSING = {
  group: "Advanced",
  section: "Audio processing",
  sync: {
    name: "Sync correction",
    sub: "Adjusts tiny speed differences between devices to reduce clicks and growing delay. Uses a short audio buffer; turn off to compare delay or troubleshoot.",
  },
  limiter: {
    name: "Limit loud peaks",
    sub: "When the combined sound gets too loud, briefly turns it down to help avoid harsh distortion. May also lower loud audio without an overload warning. Adds a very small delay.",
  },
  footnote: "Changing these may briefly interrupt the sound.",
  details: "Sync details",
} as const;

export const audioProcessingSummary = (sync: boolean, limiter: boolean) =>
  `${AUDIO_PROCESSING.sync.name} ${sync ? "on" : "off"} · ${AUDIO_PROCESSING.limiter.name} ${limiter ? "on" : "off"}`;

/** Stage state words. Running means correction is applied, not that devices are proven in sync. */
export const SYNC_STATE = {
  running: "Running",
  priming: "Starting…",
  restarting: "Restarting…",
  off: "Off",
  stale: "No recent data",
} as const;
export type SyncStateKey = keyof typeof SYNC_STATE;

/** Why a stage restarted, in everyday words. User actions never appear here. */
export const RESTART_REASON = {
  discontinuity: "the device reported a break",
  "stalled-input": "the sound stopped arriving",
  "paused-output": "the device paused",
  starved: "sound arrived too late",
  overrun: "too much sound piled up",
} as const;

const MINUS = "−";
export const signedPpm = (ppm: number) =>
  `${ppm < 0 ? MINUS : "+"}${Math.abs(ppm).toFixed(1)} ppm`;

/** Positive means Echocord consumes this stage's input faster. It is not a pitch or clock measurement. */
export function adjustmentText(ppm: number) {
  if (Math.abs(ppm) < 0.05) return "No adjustment (0.0 ppm)";
  return `Reads input slightly ${ppm > 0 ? "faster" : "slower"} (${signedPpm(ppm)})`;
}

export const bufferedText = (queuedMs: number, targetMs: number | null) =>
  targetMs === null
    ? `${Math.round(queuedMs)} ms`
    : `${Math.round(queuedMs)} ms (aiming for ${Math.round(targetMs)} ms)`;

export const COUNT_LIMIT = "Unavailable (count limit reached)";

export function lastMinuteText(recent: {
  restarts: number;
  gapMs: number;
  droppedMs: number;
}) {
  const parts = [
    recent.restarts > 0 &&
      `${recent.restarts} restart${recent.restarts === 1 ? "" : "s"}`,
    recent.gapMs > 0 && `${Math.ceil(recent.gapMs)} ms of silence`,
    recent.droppedMs > 0 && `${Math.ceil(recent.droppedMs)} ms skipped`,
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : "No dropouts";
}

export function agoText(ms: number) {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s ago`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} min ago` : `${Math.floor(m / 60)} h ago`;
}

/** "Just now" only inside the one-second audio freshness window. */
export const updatedText = (ms: number) =>
  ms < 1000 ? "Just now" : agoText(ms);

/** Status under the Limit loud peaks switch; derived only from reported reductions. */
export const LIMITER_LINE = {
  reducing: "Lowered loud peaks in the last 10 seconds",
  idle: "No peaks needed lowering recently",
  stale: "No recent data",
} as const;

/** Output-meter overload hint. The indicator reads the sum before any limiting. */
export const mixOverloadTitle = (limiter: boolean) =>
  limiter
    ? `The combined sound is too loud. Lower a source. ${AUDIO_PROCESSING.limiter.name} is on.`
    : "The combined sound is too loud. Lower a source.";

export const SYNC_COPY = {
  intro:
    "Devices can run at slightly different speeds. Sync correction makes small rate adjustments where sound passes between them.",
  honesty:
    "Adjustments show what Echocord applied, not how accurate your devices are.",
  off: "Sync correction is off, so Echocord makes no rate adjustments. Different device clocks can then cause clicks or a slowly growing delay. Format conversion still applies.",
  empty:
    "Nothing is playing. Details appear here while a source is capturing or a playback device is playing.",
  buffered:
    "Buffered sound is Echocord's own short wait, not the full delay your listeners hear.",
  discord:
    "Discord rooms receive the combined sound directly, so they have no sync correction stage.",
  sources: "Sources",
  outputs: "Playback devices",
  limiterOff: "Off",
  limiterOn: "On · adds a 3.7 ms delay",
} as const;
