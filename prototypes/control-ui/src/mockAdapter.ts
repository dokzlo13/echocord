import {
  type CommandResult,
  type ControlAdapter,
  type ControlCommand,
  type ControlSnapshot,
  sessionActive,
} from "./contract";

export const previews = [
  "disconnected",
  "connecting",
  "streaming",
  "muted",
  "recovering",
  "missing-device",
  "error",
] as const;
export type Preview = (typeof previews)[number];
export type DemoBotProfile = "ready" | "unconfigured" | "permission-denied";

const initialSnapshot = (): ControlSnapshot => ({
  revision: 0,
  sources: [
    {
      id: "cable",
      name: "CABLE Output (VB-Audio Virtual Cable)",
      available: true,
    },
    {
      id: "cable-b",
      name: "CABLE-B Output (demo recording device)",
      available: true,
    },
  ],
  servers: [
    {
      id: "table",
      name: "The Sunday Table",
      channels: [
        { id: "session", name: "Session room" },
        { id: "lobby", name: "Lobby" },
      ],
    },
    {
      id: "oneshot",
      name: "One-shot Club",
      channels: [{ id: "tavern", name: "The tavern" }],
    },
  ],
  selection: { sourceId: "cable", serverId: "table", channelId: "session" },
  bot: { status: "ready", name: "Echocord Demo" },
  connection: { phase: "disconnected" },
  capture: { health: "ready", levelDbfs: -22 },
  output: { volumePercent: 80, muted: false },
});

/** Entirely in-memory fixtures. No audio, Discord, token, or storage APIs. */
export class MockControlAdapter implements ControlAdapter {
  private snapshot = initialSnapshot();
  private readonly listeners = new Set<() => void>();
  private pending: ReturnType<typeof setTimeout> | undefined;

  getSnapshot = (): ControlSnapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private update(patch: Partial<Omit<ControlSnapshot, "revision">>) {
    this.snapshot = {
      ...this.snapshot,
      ...patch,
      revision: this.snapshot.revision + 1,
    };
    for (const listener of this.listeners) listener();
  }

  private cancelPending() {
    clearTimeout(this.pending);
    this.pending = undefined;
  }

  private finishConnection(delay: number) {
    this.cancelPending();
    this.pending = setTimeout(() => {
      this.pending = undefined;
      if (
        sessionActive(this.snapshot.connection) &&
        this.snapshot.capture.health === "ready"
      ) {
        this.update({ connection: { phase: "connected" } });
      }
    }, delay);
  }

  dispatch = async (command: ControlCommand): Promise<CommandResult> => {
    const state = this.snapshot;
    switch (command.type) {
      case "select-source": {
        if (sessionActive(state.connection))
          return {
            ok: false,
            message: "Disconnect before changing the source.",
          };
        const source = state.sources.find(
          (item) => item.id === command.sourceId,
        );
        if (!source?.available)
          return {
            ok: false,
            message: "Choose an available recording device.",
          };
        this.update({
          selection: { ...state.selection, sourceId: source.id },
          capture: { health: "ready", levelDbfs: -22 },
        });
        break;
      }
      case "select-destination": {
        if (sessionActive(state.connection))
          return {
            ok: false,
            message: "Disconnect before changing the destination.",
          };
        const server = state.servers.find(
          (item) => item.id === command.serverId,
        );
        if (
          !server ||
          (command.channelId !== null &&
            !server.channels.some((item) => item.id === command.channelId))
        ) {
          return {
            ok: false,
            message: "Choose a channel in the selected server.",
          };
        }
        this.update({
          selection: {
            ...state.selection,
            serverId: server.id,
            channelId: command.channelId,
          },
        });
        break;
      }
      case "connect":
        if (sessionActive(state.connection))
          return { ok: false, message: "A session is already active." };
        if (state.bot.status !== "ready")
          return {
            ok: false,
            message: "Configure the demo bot before connecting.",
          };
        if (
          state.capture.health !== "ready" ||
          !state.sources.some(
            (item) => item.id === state.selection.sourceId && item.available,
          )
        ) {
          return {
            ok: false,
            message: "The selected recording device is unavailable.",
          };
        }
        if (
          !state.servers.some(
            (server) =>
              server.id === state.selection.serverId &&
              server.channels.some(
                (channel) => channel.id === state.selection.channelId,
              ),
          )
        ) {
          return { ok: false, message: "Select a server and voice channel." };
        }
        this.update({ connection: { phase: "connecting" } });
        this.finishConnection(1200);
        break;
      case "disconnect":
        this.cancelPending();
        this.update({ connection: { phase: "disconnected" } });
        break;
      case "set-volume":
        if (
          !Number.isFinite(command.percent) ||
          command.percent < 0 ||
          command.percent > 100
        ) {
          return { ok: false, message: "Volume must be between 0 and 100." };
        }
        this.update({
          output: {
            ...state.output,
            volumePercent: Math.round(command.percent),
          },
        });
        break;
      case "set-muted":
        this.update({ output: { ...state.output, muted: command.muted } });
        break;
    }
    return { ok: true };
  };

  configureDemoBot(profile: DemoBotProfile) {
    if (sessionActive(this.snapshot.connection)) return;
    this.cancelPending();
    this.update({
      bot: {
        status: profile,
        name: profile === "unconfigured" ? null : "Echocord Demo",
      },
      connection: { phase: "disconnected" },
    });
  }

  setDemoSignal(silent: boolean) {
    if (this.snapshot.capture.health === "ready") {
      this.update({
        capture: { health: "ready", levelDbfs: silent ? -60 : -22 },
      });
    }
  }

  simulateNetworkLoss() {
    if (this.snapshot.connection.phase !== "connected") return;
    this.update({
      connection: { phase: "recovering", reason: "network", attempt: 1 },
    });
    this.finishConnection(1800);
  }

  removeDevice() {
    this.cancelPending();
    const state = this.snapshot;
    this.update({
      sources: state.sources.map((source) =>
        source.id === state.selection.sourceId
          ? { ...source, available: false }
          : source,
      ),
      capture: { health: "missing-device", levelDbfs: null },
      connection: sessionActive(state.connection)
        ? { phase: "recovering", reason: "device-missing", attempt: 0 }
        : state.connection,
    });
  }

  returnDevice() {
    const state = this.snapshot;
    this.update({
      sources: state.sources.map((source) =>
        source.id === state.selection.sourceId
          ? { ...source, available: true }
          : source,
      ),
      capture: { health: "ready", levelDbfs: -22 },
    });
    if (
      state.connection.phase === "recovering" &&
      state.connection.reason === "device-missing"
    ) {
      this.finishConnection(1200);
    }
  }

  /** Stable screenshot fixtures; only explicit user actions resume transitions. */
  preview(preview: Preview) {
    this.cancelPending();
    const base = initialSnapshot();
    this.update({ ...base, output: this.snapshot.output });
    switch (preview) {
      case "disconnected":
        break;
      case "connecting":
        this.update({ connection: { phase: "connecting" } });
        break;
      case "streaming":
        this.update({
          connection: { phase: "connected" },
          output: { ...this.snapshot.output, muted: false },
        });
        break;
      case "muted":
        this.update({
          connection: { phase: "connected" },
          output: { ...this.snapshot.output, muted: true },
        });
        break;
      case "recovering":
        this.update({
          connection: { phase: "recovering", reason: "network", attempt: 1 },
        });
        break;
      case "missing-device":
        this.update({ connection: { phase: "connected" } });
        this.removeDevice();
        break;
      case "error":
        this.update({
          connection: {
            phase: "error",
            code: "permission-denied",
            message: "The demo bot was denied permission to join this channel.",
          },
          bot: { status: "permission-denied", name: "Echocord Demo" },
        });
        break;
    }
  }
}
