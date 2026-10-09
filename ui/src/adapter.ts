import type {
  AppError,
  Capture,
  ControlCommand,
  ControlSnapshot,
  Selection,
} from "./generated";
import { locksRoute } from "./presentation";

/** UI lifecycle only; all wire values come directly from Rust-generated types. */
export interface Transport {
  listen: (receive: (snapshot: ControlSnapshot) => void) => Promise<() => void>;
  getSnapshot: () => Promise<ControlSnapshot>;
  dispatch: (command: ControlCommand) => Promise<void>;
  configureBot: (token: string) => Promise<void>;
  getVisible?: () => Promise<boolean>;
  listenVisibility?: (
    receive: (visible: boolean) => void,
  ) => Promise<() => void>;
}

export interface TelemetrySnapshot {
  sessionId: number;
  sourceId: string | null;
  capture: Capture | null;
  visible: boolean;
}

export type RouteOperation = "stopping" | "selecting" | "starting";

export interface ViewState {
  phase: "loading" | "ready" | "error";
  snapshot: ControlSnapshot | null;
  error: AppError | null;
  commandError: AppError | null;
  pending: readonly ControlCommand["type"][];
  volumeDraft: number | null;
  configuringBot: boolean;
  configurationError: AppError | null;
  routeOperation: RouteOperation | null;
}

export function appError(error: unknown): AppError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    const codes: AppError["code"][] = [
      "invalid-selection",
      "invalid-volume",
      "busy",
      "configuration",
      "authentication",
      "permission-denied",
      "capture-failed",
      "connection-failed",
      "cleanup-incomplete",
      "internal",
    ];
    if (codes.some((code) => code === error.code)) return error as AppError;
  }
  // Never expose arbitrary IPC exceptions, private paths, or parser excerpts.
  return {
    code: "internal",
    message:
      "Could not reach Echocord. Reopen the app or retry loading its status.",
  };
}

/** IPC snapshots are immutable JSON values; catalog clones are not changes. */
function sameValue(previous: unknown, next: unknown): boolean {
  if (Object.is(previous, next)) return true;
  if (
    typeof previous !== "object" ||
    previous === null ||
    typeof next !== "object" ||
    next === null
  )
    return false;
  if (Array.isArray(previous) || Array.isArray(next))
    return (
      Array.isArray(previous) &&
      Array.isArray(next) &&
      previous.length === next.length &&
      previous.every((value, index) => sameValue(value, next[index]))
    );
  const before = previous as Record<string, unknown>;
  const after = next as Record<string, unknown>;
  const keys = Object.keys(before);
  return (
    keys.length === Object.keys(after).length &&
    keys.every(
      (key) => Object.hasOwn(after, key) && sameValue(before[key], after[key]),
    )
  );
}

function sameControls(
  previous: ControlSnapshot | null,
  next: ControlSnapshot,
): boolean {
  if (!previous) return false;
  const {
    revision: _beforeRevision,
    capture: before,
    ...beforeControls
  } = previous;
  const { revision: _nextRevision, capture: after, ...afterControls } = next;
  return (
    before.phase === after.phase &&
    before.message === after.message &&
    sameValue(beforeControls, afterControls)
  );
}

export class ControlAdapter {
  private state: ViewState = {
    phase: "loading",
    snapshot: null,
    error: null,
    commandError: null,
    pending: [],
    volumeDraft: null,
    configuringBot: false,
    configurationError: null,
    routeOperation: null,
  };
  private readonly listeners = new Set<() => void>();
  private readonly telemetryListeners = new Set<() => void>();
  private latestSnapshot: ControlSnapshot | null = null;
  private telemetry: TelemetrySnapshot = {
    sessionId: 0,
    sourceId: null,
    capture: null,
    visible: true,
  };
  private unlisten: (() => void) | undefined;
  private unlistenVisibility: (() => void) | undefined;
  private generation = 0;
  private active = false;
  private volumeTimer: ReturnType<typeof setTimeout> | undefined;
  private volumeQueued: number | null = null;
  private volumeInFlight = false;
  private routeEpoch = 0;

  constructor(
    private readonly transport: Transport,
    private readonly volumeDelay = 120,
  ) {}

  getSnapshot = (): ViewState => this.state;
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  getTelemetrySnapshot = (): TelemetrySnapshot => this.telemetry;
  subscribeTelemetry = (listener: () => void): (() => void) => {
    this.telemetryListeners.add(listener);
    return () => this.telemetryListeners.delete(listener);
  };

  private publish(patch: Partial<ViewState>) {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }

  private receive(snapshot: ControlSnapshot, generation: number) {
    if (!this.active || generation !== this.generation) return;
    if (
      this.latestSnapshot &&
      snapshot.revision <= this.latestSnapshot.revision
    )
      return;
    // Ordering and command confirmation always use the latest full observation.
    this.latestSnapshot = snapshot;
    const telemetryChanged =
      this.telemetry.sessionId !== snapshot.sessionId ||
      this.telemetry.sourceId !== snapshot.selection.sourceId ||
      !sameValue(this.telemetry.capture, snapshot.capture);
    if (telemetryChanged)
      this.telemetry = {
        sessionId: snapshot.sessionId,
        sourceId: snapshot.selection.sourceId,
        capture: snapshot.capture,
        visible: this.telemetry.visible,
      };
    const confirmed =
      this.state.volumeDraft !== null &&
      this.state.volumeDraft === snapshot.output.volumePercent;
    if (
      !sameControls(this.state.snapshot, snapshot) ||
      this.state.phase !== "ready" ||
      this.state.error !== null ||
      confirmed
    )
      this.publish({
        snapshot,
        phase: "ready",
        error: null,
        volumeDraft: confirmed ? null : this.state.volumeDraft,
      });
    // Keep one fresh observation while hidden; visibility return notifies once.
    if (
      this.active &&
      generation === this.generation &&
      telemetryChanged &&
      this.telemetry.visible
    )
      this.notifyTelemetry();
  }

  private notifyTelemetry() {
    for (const listener of this.telemetryListeners) listener();
  }

  private setVisible(visible: boolean) {
    if (this.telemetry.visible === visible) return;
    this.telemetry = { ...this.telemetry, visible };
    this.notifyTelemetry();
  }

  private async startVisibility(generation: number): Promise<void> {
    const current = () => this.active && generation === this.generation;
    let receivedEvent = false;
    try {
      if (this.transport.listenVisibility) {
        const unlisten = await this.transport.listenVisibility((visible) => {
          if (!current()) return;
          receivedEvent = true;
          this.setVisible(visible);
        });
        if (!current()) {
          unlisten();
          return;
        }
        this.unlistenVisibility = unlisten;
      }
      if (!current()) return;
      if (this.transport.getVisible) {
        const visible = await this.transport.getVisible();
        // A boolean read has no revision; any subscribed event outranks it.
        if (current() && !receivedEvent) this.setVisible(visible);
      }
    } catch {
      // Visibility failures cannot block commands or override a known hide.
    }
  }

  /** Listener-first avoids losing a transition between cold start and subscription. */
  start = async (): Promise<void> => {
    this.dispose();
    this.active = true;
    const generation = this.generation;
    // Native presentation waits for visibility; fixtures default to visible.
    this.setVisible(!this.transport.getVisible);
    this.publish({
      phase: "loading",
      error: null,
      commandError: null,
      pending: [],
      volumeDraft: null,
      configuringBot: false,
      configurationError: null,
      routeOperation: null,
    });
    const visibility = this.startVisibility(generation);
    try {
      const unlisten = await this.transport.listen((snapshot) =>
        this.receive(snapshot, generation),
      );
      if (!this.active || generation !== this.generation) {
        unlisten();
        return;
      }
      this.unlisten = unlisten;
      const snapshot = await this.transport.getSnapshot();
      this.receive(snapshot, generation);
      // A retained snapshot with the same revision still completes a retry.
      if (
        this.active &&
        generation === this.generation &&
        this.state.phase === "loading"
      )
        this.publish({ phase: "ready" });
    } catch (error) {
      if (this.active && generation === this.generation)
        this.publish({ phase: "error", error: appError(error) });
    }
    await visibility;
  };

  dispose = () => {
    this.active = false;
    this.generation++;
    this.unlisten?.();
    this.unlisten = undefined;
    this.unlistenVisibility?.();
    this.unlistenVisibility = undefined;
    this.setVisible(false);
    clearTimeout(this.volumeTimer);
    this.volumeTimer = undefined;
    this.volumeQueued = null;
    this.volumeInFlight = false;
    this.routeEpoch++;
    for (const listener of this.listeners) listener();
  };

  dismissError = () => this.publish({ commandError: null });
  dismissConfigurationError = () => this.publish({ configurationError: null });

  configureBot = async (token: string): Promise<boolean> => {
    const snapshot = this.latestSnapshot;
    if (
      !this.active ||
      this.state.phase !== "ready" ||
      !snapshot ||
      locksRoute(snapshot) ||
      snapshot.bot.status === "checking" ||
      this.state.configuringBot ||
      this.state.routeOperation
    )
      return false;
    const generation = this.generation;
    this.publish({ configuringBot: true, configurationError: null });
    try {
      const request = this.transport.configureBot(token);
      token = "";
      await request;
      return this.active && generation === this.generation;
    } catch {
      // A secret-bearing command never echoes arbitrary exception or REST text.
      if (this.active && generation === this.generation)
        this.publish({
          configurationError: {
            code: "configuration",
            message:
              "Could not confirm this bot token was saved. Check bot status and try again.",
          },
        });
      return false;
    } finally {
      token = "";
      if (this.active && generation === this.generation)
        this.publish({ configuringBot: false });
    }
  };

  private waitForSnapshot(
    predicate: (snapshot: ControlSnapshot) => boolean,
    epoch: number,
  ): Promise<void> {
    return new Promise((resolve, reject) => {
      let unsubscribe = () => {};
      const finish = (error?: AppError) => {
        clearTimeout(timer);
        unsubscribe();
        error ? reject(error) : resolve();
      };
      const inspect = () => {
        if (!this.active || epoch !== this.routeEpoch)
          return finish({ code: "busy", message: "Route change cancelled." });
        const snapshot = this.latestSnapshot;
        if (!snapshot) return;
        if (snapshot.connection.phase === "cleanup-incomplete")
          return finish({
            code: "cleanup-incomplete",
            message:
              "Disconnect has not finished. Retry disconnect before applying another route.",
          });
        if (predicate(snapshot)) finish();
      };
      const timer = setTimeout(
        () =>
          finish({
            code: "busy",
            message:
              "Waiting for the app to confirm the route change. Stop the session and try again.",
          }),
        20_000,
      );
      unsubscribe = this.subscribe(inspect);
      inspect();
    });
  }

  stop = async (): Promise<boolean> => {
    return this.dispatch({ type: "disconnect" });
  };

  /** An explicit apply never chooses a fallback or reconnects before confirmed cleanup. */
  applyRoute = async (
    selection: Selection,
    connectAfter = false,
  ): Promise<boolean> => {
    const initial = this.latestSnapshot;
    if (
      !this.active ||
      this.state.phase !== "ready" ||
      !initial ||
      this.state.routeOperation ||
      this.state.configuringBot
    )
      return false;
    const source = initial.sources.find(
      (item) => item.id === selection.sourceId && item.available,
    );
    const server = initial.servers.find(
      (item) => item.id === selection.serverId,
    );
    if (
      !source ||
      !server?.channels.some((item) => item.id === selection.channelId)
    ) {
      this.publish({
        commandError: {
          code: "invalid-selection",
          message:
            "Choose an available recording device and voice room before applying.",
        },
      });
      return false;
    }
    if (
      ["disconnecting", "cleanup-incomplete"].includes(initial.connection.phase)
    )
      return false;
    const epoch = ++this.routeEpoch;
    const active = () => this.active && epoch === this.routeEpoch;
    this.publish({
      routeOperation: locksRoute(initial) ? "stopping" : "selecting",
      commandError: null,
    });
    try {
      if (locksRoute(initial)) {
        if (!(await this.sendCommand({ type: "disconnect" }))) return false;
        await this.waitForSnapshot(
          (snapshot) => snapshot.connection.phase === "disconnected",
          epoch,
        );
      }
      if (!active()) return false;
      this.publish({ routeOperation: "selecting" });
      if (this.latestSnapshot?.selection.sourceId !== selection.sourceId) {
        if (
          !(await this.sendCommand({
            type: "select-source",
            sourceId: source.id,
          }))
        )
          return false;
        await this.waitForSnapshot(
          (snapshot) => snapshot.selection.sourceId === source.id,
          epoch,
        );
      }
      if (!active()) return false;
      const current = this.latestSnapshot?.selection;
      if (
        current?.serverId !== selection.serverId ||
        current.channelId !== selection.channelId
      ) {
        if (
          !(await this.sendCommand({
            type: "select-destination",
            serverId: server.id,
            channelId: selection.channelId,
          }))
        )
          return false;
        await this.waitForSnapshot(
          (snapshot) =>
            snapshot.selection.serverId === selection.serverId &&
            snapshot.selection.channelId === selection.channelId,
          epoch,
        );
      }
      if (!active()) return false;
      if (connectAfter) {
        this.publish({ routeOperation: "starting" });
        const accepted = await this.sendCommand({ type: "connect" });
        return active() && accepted;
      }
      return true;
    } catch (error) {
      if (active()) this.publish({ commandError: appError(error) });
      return false;
    } finally {
      if (active()) this.publish({ routeOperation: null });
    }
  };

  dispatch = async (command: ControlCommand): Promise<boolean> => {
    if (command.type === "disconnect") {
      this.routeEpoch++;
      this.publish({ routeOperation: null });
    } else if (
      (this.state.routeOperation || this.state.configuringBot) &&
      !["set-muted", "set-volume"].includes(command.type)
    ) {
      return false;
    }
    return this.sendCommand(command);
  };

  private sendCommand = async (command: ControlCommand): Promise<boolean> => {
    if (!this.active || this.state.phase !== "ready") return false;
    const generation = this.generation;
    if (command.type === "disconnect") {
      // Cancel unsent slider work so it cannot modify a later session.
      clearTimeout(this.volumeTimer);
      this.volumeQueued = null;
      this.publish({ volumeDraft: null });
    }
    this.publish({
      pending: [...this.state.pending, command.type],
      commandError: null,
    });
    try {
      await this.transport.dispatch(command);
      return true;
    } catch (error) {
      if (this.active && generation === this.generation)
        this.publish({ commandError: appError(error) });
      return false;
    } finally {
      if (this.active && generation === this.generation) {
        const pending = [...this.state.pending];
        const index = pending.indexOf(command.type);
        if (index >= 0) pending.splice(index, 1);
        this.publish({ pending });
      }
    }
  };

  setVolume = (percent: number) => {
    if (!this.active || this.state.phase !== "ready") return;
    this.volumeQueued = Math.max(0, Math.min(100, Math.round(percent)));
    this.publish({ volumeDraft: this.volumeQueued });
    clearTimeout(this.volumeTimer);
    this.volumeTimer = setTimeout(() => {
      void this.flushVolume();
    }, this.volumeDelay);
  };

  flushVolume = async (): Promise<void> => {
    clearTimeout(this.volumeTimer);
    this.volumeTimer = undefined;
    if (this.volumeInFlight || this.volumeQueued === null || !this.active)
      return;
    const generation = this.generation;
    const percent = this.volumeQueued;
    this.volumeQueued = null;
    this.volumeInFlight = true;
    const accepted = await this.dispatch({ type: "set-volume", percent });
    if (!this.active || generation !== this.generation) return;
    this.volumeInFlight = false;
    if (!accepted && this.state.volumeDraft === percent)
      this.publish({ volumeDraft: null });
    // A meter event must not clear a newer slider intent. Keep at most one queued value.
    if (this.volumeQueued !== null) await this.flushVolume();
  };
}
