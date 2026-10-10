import type { GatewayPhase } from "./generated";
import type { DestState, Stage } from "./model";

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
      return "standby";
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
  "cleanup-incomplete": "Cleanup incomplete",
  error: "Connection error",
};
