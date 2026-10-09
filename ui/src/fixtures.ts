import type { Transport } from "./adapter";
import type { ControlCommand, ControlSnapshot } from "./generated";

export const fixtureNames = [
  "disconnected",
  "first-run",
  "loading",
  "empty-devices",
  "empty-servers",
  "no-channels",
  "unavailable-route",
  "invalid-config",
  "discovery-error",
  "connecting",
  "connected",
  "muted",
  "recovering",
  "missing-device",
  "capture-error",
  "connection-error",
  "disconnecting",
  "cleanup-incomplete",
  "rejected-command",
  "backend-unavailable",
] as const;
export type FixtureName = (typeof fixtureNames)[number];

export function fixtureSnapshot(
  name: FixtureName = "disconnected",
): ControlSnapshot {
  const snapshot: ControlSnapshot = {
    revision: 1,
    sessionId: 0,
    sources: [
      {
        id: "cable",
        name: "CABLE Output (VB-Audio Virtual Cable)",
        available: true,
        detail: "48 kHz recording device",
      },
      {
        id: "cable-b",
        name: "CABLE-B Output",
        available: true,
        detail: "48 kHz recording device",
      },
    ],
    servers: [
      {
        id: "1234567890123456789",
        name: "The Sunday Table",
        iconUrl: null,
        channels: [
          {
            id: "9876543210987654321",
            name: "Session room",
            categoryName: "Game night",
            position: 0,
          },
          {
            id: "9876543210987654322",
            name: "Lobby",
            categoryName: "General",
            position: 1,
          },
        ],
      },
      {
        id: "1234567890123456790",
        name: "One-shot Club",
        iconUrl: null,
        channels: [
          {
            id: "9876543210987654323",
            name: "The tavern",
            categoryName: null,
            position: 0,
          },
        ],
      },
    ],
    selection: {
      sourceId: "cable",
      serverId: "1234567890123456789",
      channelId: "9876543210987654321",
    },
    bot: { status: "ready", name: "Table Audio", message: null },
    discovery: { phase: "ready" },
    connection: { phase: "disconnected" },
    capture: {
      phase: "stopped",
      levelDbfs: null,
      message: null,
      droppedFrames: 0,
      underrunFrames: 0,
      queuedFrames: 0,
    },
    output: { volumePercent: 80, muted: false },
  };
  if (
    [
      "connecting",
      "connected",
      "muted",
      "recovering",
      "missing-device",
      "capture-error",
      "disconnecting",
      "cleanup-incomplete",
    ].includes(name)
  )
    snapshot.sessionId = 1;
  if (
    [
      "connected",
      "muted",
      "recovering",
      "missing-device",
      "capture-error",
    ].includes(name)
  ) {
    snapshot.connection = { phase: "connected" };
    snapshot.capture = {
      ...snapshot.capture,
      phase: "running",
      levelDbfs: -21,
    };
  }
  switch (name) {
    case "first-run":
      snapshot.sources = [];
      snapshot.servers = [];
      snapshot.selection = { sourceId: null, serverId: null, channelId: null };
      snapshot.bot = {
        status: "unconfigured",
        name: null,
        message: "Bot configuration has not been supplied to the app.",
      };
      snapshot.discovery = { phase: "idle" };
      break;
    case "loading":
      snapshot.discovery = { phase: "loading" };
      snapshot.bot.status = "checking";
      break;
    case "empty-devices":
      snapshot.sources = [];
      snapshot.selection.sourceId = null;
      break;
    case "empty-servers":
      snapshot.servers = [];
      snapshot.selection.serverId = null;
      snapshot.selection.channelId = null;
      break;
    case "no-channels":
      snapshot.servers = snapshot.servers.map((server, index) =>
        index === 0 ? { ...server, channels: [] } : server,
      );
      snapshot.selection.channelId = null;
      break;
    case "unavailable-route":
      snapshot.selection = {
        sourceId: "saved-device",
        serverId: "saved-server",
        channelId: "saved-channel",
      };
      break;
    case "invalid-config":
      snapshot.bot = {
        status: "invalid",
        name: null,
        message: "Bot configuration is invalid. Check the token, then recheck.",
      };
      break;
    case "discovery-error":
      snapshot.discovery = {
        phase: "error",
        message: "Could not load Discord servers. Refresh to try again.",
      };
      break;
    case "connecting":
      snapshot.connection = { phase: "connecting", stage: "gateway" };
      snapshot.capture.phase = "opening";
      break;
    case "muted":
      snapshot.output.muted = true;
      break;
    case "recovering":
      snapshot.connection = {
        phase: "recovering",
        attempt: 1,
        message:
          "Restoring the same voice channel. Disconnect cancels recovery.",
      };
      snapshot.output.muted = true;
      break;
    case "missing-device":
      snapshot.capture = {
        ...snapshot.capture,
        phase: "missing",
        levelDbfs: null,
        message:
          "The selected device is missing. Restore that device or disconnect to choose another.",
      };
      snapshot.sources = snapshot.sources.map((source, index) =>
        index === 0 ? { ...source, available: false } : source,
      );
      break;
    case "capture-error":
      snapshot.capture = {
        ...snapshot.capture,
        phase: "error",
        levelDbfs: null,
        message:
          "Could not open the selected input at 48 kHz. Check its Windows sound settings.",
      };
      break;
    case "connection-error":
      snapshot.connection = {
        phase: "error",
        code: "permission-denied",
        message:
          "The bot cannot join this channel. Check its Connect and Speak permissions.",
      };
      break;
    case "disconnecting":
      snapshot.connection = { phase: "disconnecting" };
      break;
    case "cleanup-incomplete":
      snapshot.connection = {
        phase: "cleanup-incomplete",
        message:
          "The session has not exited. Retry disconnect or quit from the tray before reconnecting.",
      };
      break;
  }
  return snapshot;
}

/** Included only by an explicit development import; never a native failure fallback. */
export class FixtureTransport implements Transport {
  readonly names = fixtureNames;
  private snapshot: ControlSnapshot;
  private readonly listeners = new Set<(snapshot: ControlSnapshot) => void>();
  private timer: ReturnType<typeof setTimeout> | undefined;
  readonly commands: ControlCommand[] = [];

  constructor(public name: FixtureName = "disconnected") {
    this.snapshot = fixtureSnapshot(name);
  }

  listen = async (receive: (snapshot: ControlSnapshot) => void) => {
    if (this.name === "backend-unavailable")
      throw {
        code: "internal",
        message: "Fixture backend unavailable. Retry loading status.",
      };
    this.listeners.add(receive);
    return () => this.listeners.delete(receive);
  };
  getSnapshot = async () => this.snapshot;

  private update(patch: Partial<ControlSnapshot>) {
    this.snapshot = {
      ...this.snapshot,
      ...patch,
      revision: this.snapshot.revision + 1,
    };
    for (const receive of this.listeners) receive(this.snapshot);
  }

  preview = (name: FixtureName) => {
    clearTimeout(this.timer);
    this.name = name;
    this.update(fixtureSnapshot(name));
  };

  configureBot = async (token: string): Promise<void> => {
    if (!token.trim() || token.length > 4096)
      throw { code: "configuration", message: "Token could not be saved." };
    // Do not retain the token or add it to command/snapshot evidence.
    this.update({ bot: { status: "checking", name: null, message: null } });
    await new Promise<void>((resolve) => setTimeout(resolve, 500));
    this.update({
      bot: { status: "ready", name: "Table Audio", message: null },
    });
  };

  dispatch = async (command: ControlCommand) => {
    this.commands.push(command);
    switch (command.type) {
      case "set-volume":
        this.update({
          output: { ...this.snapshot.output, volumePercent: command.percent },
        });
        break;
      case "set-muted":
        this.update({
          output: { ...this.snapshot.output, muted: command.muted },
        });
        break;
      case "select-source":
        this.update({
          selection: { ...this.snapshot.selection, sourceId: command.sourceId },
        });
        break;
      case "select-destination":
        this.update({
          selection: {
            ...this.snapshot.selection,
            serverId: command.serverId,
            channelId: command.channelId,
          },
        });
        break;
      case "connect": {
        if (this.name === "rejected-command")
          throw {
            code: "permission-denied",
            message:
              "The bot was denied permission to connect. Review channel permissions.",
          };
        clearTimeout(this.timer);
        const sessionId = this.snapshot.sessionId + 1;
        this.update({
          sessionId,
          connection: { phase: "connecting", stage: "gateway" },
          capture: {
            ...this.snapshot.capture,
            phase: "opening",
            levelDbfs: null,
          },
        });
        this.timer = setTimeout(() => {
          if (
            this.snapshot.sessionId === sessionId &&
            this.snapshot.connection.phase === "connecting"
          )
            this.update({
              connection: { phase: "connected" },
              capture: {
                ...this.snapshot.capture,
                phase: "running",
                levelDbfs: -21,
              },
            });
        }, 700);
        break;
      }
      case "disconnect":
        clearTimeout(this.timer);
        this.update({
          connection: { phase: "disconnecting" },
          capture: {
            ...this.snapshot.capture,
            phase: "stopped",
            levelDbfs: null,
          },
        });
        this.timer = setTimeout(
          () => this.update({ connection: { phase: "disconnected" } }),
          400,
        );
        break;
      case "recheck-configuration":
        this.update({
          bot: { ...this.snapshot.bot, status: "checking", message: null },
        });
        clearTimeout(this.timer);
        this.timer = setTimeout(
          () =>
            this.update({
              bot: { status: "ready", name: "Table Audio", message: null },
            }),
          500,
        );
        break;
      case "refresh-catalog":
        this.update({ discovery: { phase: "loading" } });
        clearTimeout(this.timer);
        this.timer = setTimeout(() => {
          const catalog = fixtureSnapshot();
          this.update({
            sources: catalog.sources,
            servers: catalog.servers,
            discovery: { phase: "ready" },
          });
        }, 500);
        break;
    }
  };
}
