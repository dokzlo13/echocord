import type { ConnectStage, ControlSnapshot } from "./generated";

export const connectStageLabels: Record<ConnectStage, string> = {
  checking: "Checking bot configuration…",
  preflight: "Checking the selected route…",
  gateway: "Connecting to Discord…",
  join: "Joining the voice channel…",
  capture: "Opening audio source…",
};

export function locksRoute(snapshot: ControlSnapshot): boolean {
  return [
    "connecting",
    "connected",
    "recovering",
    "disconnecting",
    "cleanup-incomplete",
  ].includes(snapshot.connection.phase);
}

export function readiness(snapshot: ControlSnapshot): string | null {
  if (snapshot.connection.phase === "cleanup-incomplete")
    return "Cleanup must finish before another connection.";
  if (snapshot.connection.phase === "disconnecting")
    return "Waiting for the session to stop.";
  if (locksRoute(snapshot)) return null;
  if (snapshot.bot.status !== "ready")
    return snapshot.bot.status === "checking"
      ? "Checking bot configuration…"
      : "Review bot configuration to connect.";
  if (snapshot.discovery.phase === "loading")
    return "Loading Discord servers and rooms…";
  if (snapshot.discovery.phase !== "ready")
    return "Refresh Discord rooms to choose a destination.";
  if (!snapshot.selection.sourceId) return "Select an audio source.";
  const source = snapshot.sources.find(
    (item) => item.id === snapshot.selection.sourceId,
  );
  if (!source?.available)
    return source?.kind === "application"
      ? "The selected application is unavailable. Refresh sources and select it again."
      : "The selected audio source is unavailable.";
  const server = snapshot.servers.find(
    (item) => item.id === snapshot.selection.serverId,
  );
  if (!server) return "Select a server.";
  if (
    !server.channels.some(
      (channel) => channel.id === snapshot.selection.channelId,
    )
  )
    return "Select a voice channel.";
  return null;
}

export function connectionLabel(snapshot: ControlSnapshot): string {
  const labels = {
    disconnected: "Disconnected",
    connecting: "Connecting",
    connected: "Connected",
    recovering: "Recovering",
    disconnecting: "Disconnecting",
    "cleanup-incomplete": "Cleanup incomplete",
    error: "Connection failed",
  };
  return labels[snapshot.connection.phase];
}

export function captureLabel(snapshot: ControlSnapshot): string {
  const labels = {
    stopped: "Capture stopped",
    opening: "Opening input",
    running: "Input running",
    missing:
      snapshot.sources.find(
        (source) => source.id === snapshot.selection.sourceId,
      )?.kind === "application"
        ? "Application unavailable"
        : "Device missing",
    error: "Input failed",
  };
  return labels[snapshot.capture.phase];
}

export function outputLabel(snapshot: ControlSnapshot): string {
  if (snapshot.output.muted) return "Muted";
  if (snapshot.output.volumePercent === 0) return "Volume is zero";
  return "Output enabled";
}
