/** EC-004 proposal only. No generated Rust bindings or audio samples. */
export interface RecordingSource {
  readonly id: string;
  readonly name: string;
  readonly available: boolean;
}

export interface Server {
  readonly id: string;
  readonly name: string;
  readonly channels: readonly { readonly id: string; readonly name: string }[];
}

export type Connection =
  | { readonly phase: "disconnected" }
  | { readonly phase: "connecting" }
  | { readonly phase: "connected" }
  | {
      readonly phase: "recovering";
      readonly reason: "network" | "device-missing";
      readonly attempt: number;
    }
  | {
      readonly phase: "error";
      readonly code: "permission-denied" | "connection-failed";
      readonly message: string;
    };

export interface ControlSnapshot {
  readonly revision: number;
  readonly sources: readonly RecordingSource[];
  readonly servers: readonly Server[];
  readonly selection: {
    readonly sourceId: string | null;
    readonly serverId: string | null;
    readonly channelId: string | null;
  };
  readonly bot: {
    readonly status: "unconfigured" | "ready" | "permission-denied";
    readonly name: string | null;
  };
  readonly connection: Connection;
  readonly capture: {
    readonly health: "ready" | "missing-device";
    /** Pre-volume, pre-mute input; null = unavailable, not silence. */
    readonly levelDbfs: number | null;
  };
  readonly output: { readonly volumePercent: number; readonly muted: boolean };
}

export type ControlCommand =
  | { readonly type: "select-source"; readonly sourceId: string }
  | {
      readonly type: "select-destination";
      readonly serverId: string;
      readonly channelId: string | null;
    }
  | { readonly type: "connect" }
  | { readonly type: "disconnect" }
  | { readonly type: "set-volume"; readonly percent: number }
  | { readonly type: "set-muted"; readonly muted: boolean };

/** Acceptance is not connection completion; snapshots report that separately. */
export type CommandResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly message: string };

export interface ControlAdapter {
  getSnapshot: () => ControlSnapshot;
  subscribe: (listener: () => void) => () => void;
  dispatch: (command: ControlCommand) => Promise<CommandResult>;
}

export function sessionActive(connection: Connection): boolean {
  return ["connecting", "connected", "recovering"].includes(connection.phase);
}

/** Delivery describes control state, never proof that a listener heard audio. */
export function deliveryLabel(snapshot: ControlSnapshot): string {
  if (snapshot.connection.phase !== "connected") return "Not sending";
  if (snapshot.capture.health !== "ready") return "Input unavailable";
  if (snapshot.output.muted) return "Muted";
  if (snapshot.output.volumePercent === 0) return "Volume is zero";
  if ((snapshot.capture.levelDbfs ?? -60) <= -60) return "Silent input";
  return "Sending · simulated";
}
