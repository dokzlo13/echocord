/** Development/test control plane only. This module is absent from release builds. */
import type { Transport } from "./adapter";
import type {
  ControlCommand,
  ControlSnapshot,
  MixTelemetry,
} from "./generated";
export const fixtureNames = [
  "first-run",
  "disconnected",
  "connected",
  "muted",
  "many",
  "restored",
  "faults",
  "connecting",
  "cleanup",
] as const;
export type FixtureName = (typeof fixtureNames)[number];
export function fixtureSnapshot(
  name: FixtureName = "disconnected",
): ControlSnapshot {
  const sources: ControlSnapshot["sources"] = [
    {
      id: "app:player:1",
      kind: "application",
      name: "Preview player",
      detail: "Example track · selected process tree",
      available: true,
    },
    {
      id: "app:browser:2",
      kind: "application",
      name: "Preview browser",
      detail: "Two windows · selected process tree",
      available: true,
    },
    {
      id: "rec:cable",
      kind: "recording",
      name: "Preview recording input",
      detail: "48 kHz stereo recording",
      available: true,
    },
    {
      id: "rec:unsupported",
      kind: "recording",
      name: "Unsupported input",
      detail:
        "44.1 kHz input is unsupported. Switch this input to 48 kHz in Windows sound settings.",
      available: false,
    },
  ];
  const player = sources[0];
  const browser = sources[1];
  if (!player || !browser) throw Error("Missing owned test catalog");
  const servers: ControlSnapshot["servers"] = [
    {
      id: "guild:1",
      name: "Studio group",
      iconUrl: null,
      channels: [
        {
          id: "room:1",
          name: "Session room",
          categoryName: "Game night",
          position: 0,
        },
        {
          id: "room:2",
          name: "Lounge",
          categoryName: "Game night",
          position: 1,
        },
      ],
    },
    {
      id: "guild:2",
      name: "Вечерняя группа",
      iconUrl: null,
      channels: [
        {
          id: "room:3",
          name: "Сессия",
          categoryName: "Голосовые каналы",
          position: 0,
        },
      ],
    },
  ];
  const inputs: ControlSnapshot["inputs"] =
    name === "first-run"
      ? []
      : [
          {
            stripId: "strip:1",
            sourceId: player.id,
            kind: "application",
            displayName: player.name,
            detail: player.detail,
            phase: "capturing",
            gainDb: -3,
            muted: false,
            colourSlot: 0,
            restored: false,
            fault: null,
          },
        ];
  if (name === "many" || name === "restored" || name === "faults") {
    inputs.push({
      stripId: "strip:2",
      sourceId: browser.id,
      kind: "application",
      displayName: browser.name,
      detail: browser.detail,
      phase: name === "restored" ? "stopped" : "capturing",
      gainDb: 0,
      muted: name === "many",
      colourSlot: 1,
      restored: name === "restored",
      fault: null,
    });
    inputs.push({
      stripId: "strip:3",
      sourceId: "rec:cable",
      kind: "recording",
      displayName: "Preview recording input",
      detail: "48 kHz stereo",
      phase: "stopped",
      gainDb: 0,
      muted: false,
      colourSlot: 2,
      restored: false,
      fault: null,
    });
  }
  let connection: ControlSnapshot["connection"] =
    name === "connected" || name === "muted" || name === "many"
      ? { phase: "connected" }
      : name === "connecting"
        ? { phase: "connecting", stage: "join" }
        : name === "cleanup"
          ? {
              phase: "cleanup-incomplete",
              message: "Audio cleanup has not finished. Retry disconnect.",
            }
          : { phase: "disconnected" };
  if (name === "faults" && inputs[0]) {
    inputs[0].phase = "fault";
    inputs[0].fault = {
      code: "capture-failed",
      message:
        "The selected application closed. Pick its current process again.",
    };
    connection = {
      phase: "error",
      code: "permission-denied",
      message: "The bot cannot speak in this room. Allow Connect and Speak.",
    };
  }
  const telemetry: MixTelemetry = {
    mixPreClip: false,
    inputs: inputs.map((s) => ({
      stripId: s.stripId,
      levelDbfs: s.phase === "capturing" ? -18 : null,
      preClip: false,
      queuedFrames: 0,
      droppedFrames: 0,
      underrunFrames: 0,
    })),
    outputLevelDbfs:
      connection.phase === "connected" && name !== "muted" ? -24 : null,
    queuedFrames: connection.phase === "connected" ? 480 : 0,
    droppedFrames: 0,
    underrunFrames: 0,
    gatewayPingMs: null,
  };
  return {
    revision: 1,
    sessionId: 1,
    sources,
    servers,
    inputs,
    destinations:
      name === "first-run"
        ? []
        : [
            {
              destinationId: "dest:1",
              serverId: "guild:1",
              channelId: "room:1",
              connection,
              volumeDb: -6,
              muted: name === "muted",
            },
          ],
    settings: { meterStyle: "simple", restoreSetup: true, keepInTray: true },
    telemetry,
    selection: {
      sourceId: inputs[0]?.sourceId ?? null,
      serverId: name === "first-run" ? null : "guild:1",
      channelId: name === "first-run" ? null : "room:1",
    },
    bot: {
      status: name === "first-run" ? "unconfigured" : "ready",
      name: name === "first-run" ? null : "Preview bot",
      message: null,
    },
    discovery: { phase: "ready" },
    sourceDiscovery: { phase: "ready" },
    connection,
    capture: {
      phase: "running",
      levelDbfs: -18,
      outputLevelDbfs: telemetry.outputLevelDbfs,
      message: null,
      droppedFrames: 0,
      underrunFrames: 0,
      queuedFrames: telemetry.queuedFrames,
    },
    output: { volumePercent: 50, muted: name === "muted" },
  };
}
export class FixtureTransport implements Transport {
  current: ControlSnapshot;
  commands: ControlCommand[] = [];
  reject: unknown = null;
  rejectSave = false;
  holdConnect = false;
  holdDisconnect = false;
  minimized = 0;
  closed = 0;
  dragged = 0;
  private listeners = new Set<(s: ControlSnapshot) => void>();
  private visibility = new Set<(v: boolean) => void>();
  constructor(name: FixtureName = "disconnected") {
    this.current = fixtureSnapshot(name);
  }
  listen = async (f: (s: ControlSnapshot) => void) => {
    this.listeners.add(f);
    return () => this.listeners.delete(f);
  };
  getSnapshot = async () => structuredClone(this.current);
  getVisible = async () => true;
  listenVisibility = async (f: (v: boolean) => void) => {
    this.visibility.add(f);
    return () => this.visibility.delete(f);
  };
  visible(v: boolean) {
    for (const f of this.visibility) f(v);
  }
  emit(patch: Partial<ControlSnapshot>) {
    this.current = {
      ...this.current,
      ...patch,
      revision: this.current.revision + 1,
    };
    for (const f of this.listeners) f(structuredClone(this.current));
  }
  configureBot = async (_token: string) => {
    if (this.rejectSave) throw Error("Fixture credential failure");
    this.emit({
      bot: { status: "ready", name: "Configured bot", message: null },
    });
  };
  minimize = async () => {
    this.minimized++;
  };
  close = async () => {
    this.closed++;
  };
  startDragging = async () => {
    this.dragged++;
  };
  dispatch = async (c: ControlCommand) => {
    this.commands.push(c);
    if (this.reject) throw this.reject;
    const input = (id: string) =>
      this.current.inputs.find((s) => s.stripId === id);
    const dest = (id: string) =>
      this.current.destinations.find((d) => d.destinationId === id);
    switch (c.type) {
      case "add-source": {
        const s = this.current.sources.find((s) => s.id === c.sourceId);
        if (s) {
          const colour =
            Math.max(-1, ...this.current.inputs.map((s) => s.colourSlot)) + 1;
          this.current.inputs.push({
            stripId: `strip:${Date.now()}`,
            sourceId: s.id,
            kind: s.kind,
            displayName: s.name,
            detail: s.detail,
            phase: "capturing",
            fault: null,
            gainDb: 0,
            muted: false,
            colourSlot: Math.min(5, colour),
            restored: false,
          });
        }
        break;
      }
      case "remove-source":
        this.current.inputs = this.current.inputs.filter(
          (s) => s.stripId !== c.stripId,
        );
        break;
      case "replace-source": {
        const s = input(c.stripId);
        const next = this.current.sources.find((s) => s.id === c.sourceId);
        if (s && next)
          Object.assign(s, {
            sourceId: next.id,
            kind: next.kind,
            displayName: next.name,
            detail: next.detail,
            phase: "capturing",
            fault: null,
            restored: false,
          });
        break;
      }
      case "start-capture": {
        const s = input(c.stripId);
        if (s) {
          s.phase = "capturing";
          s.restored = false;
        }
        break;
      }
      case "stop-capture": {
        const s = input(c.stripId);
        if (s) s.phase = "stopped";
        break;
      }
      case "set-source-muted": {
        const s = input(c.stripId);
        if (s) s.muted = c.muted;
        break;
      }
      case "set-source-gain": {
        const s = input(c.stripId);
        if (s) s.gainDb = c.gainDb;
        break;
      }
      case "add-destination":
        this.current.destinations = [
          {
            destinationId: "dest:1",
            serverId: c.serverId,
            channelId: c.channelId,
            connection: { phase: "disconnected" },
            volumeDb: 0,
            muted: false,
          },
        ];
        break;
      case "remove-destination":
        this.current.destinations = [];
        break;
      case "change-destination": {
        const d = dest(c.destinationId);
        if (d) {
          d.serverId = c.serverId;
          d.channelId = c.channelId;
        }
        break;
      }
      case "connect-destination": {
        const d = dest(c.destinationId);
        if (d)
          d.connection = this.holdConnect
            ? { phase: "connecting", stage: "gateway" }
            : { phase: "connected" };
        break;
      }
      case "disconnect-destination": {
        const d = dest(c.destinationId);
        if (d)
          d.connection = this.holdDisconnect
            ? {
                phase: "cleanup-incomplete",
                message: "Audio cleanup has not finished.",
              }
            : { phase: "disconnected" };
        break;
      }
      case "set-destination-volume": {
        const d = dest(c.destinationId);
        if (d) d.volumeDb = c.volumeDb;
        break;
      }
      case "set-destination-muted": {
        const d = dest(c.destinationId);
        if (d) d.muted = c.muted;
        break;
      }
      case "set-settings":
        this.current.settings = c.settings;
        break;
      case "refresh-sources":
        this.current.sourceDiscovery = { phase: "ready" };
        break;
      case "refresh-catalog":
        this.current.discovery = { phase: "ready" };
        break;
      case "recheck-configuration":
        this.current.bot = { ...this.current.bot, status: "ready" };
        break;
    }
    this.emit({
      inputs: this.current.inputs,
      destinations: this.current.destinations,
      settings: this.current.settings,
    });
  };
}
