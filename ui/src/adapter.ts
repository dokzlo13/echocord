import type {
  AppError,
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
}

export type RouteOperation = "stopping" | "selecting" | "starting";
export interface LevelObservation {
  at: number;
  dbfs: number;
}

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
  levelHistory: readonly LevelObservation[];
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
    levelHistory: [],
  };
  private readonly listeners = new Set<() => void>();
  private unlisten: (() => void) | undefined;
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

  private publish(patch: Partial<ViewState>) {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }

  private receive(snapshot: ControlSnapshot, generation: number) {
    if (!this.active || generation !== this.generation) return;
    if (
      this.state.snapshot &&
      snapshot.revision <= this.state.snapshot.revision
    )
      return;
    const confirmed = this.state.volumeDraft === snapshot.output.volumePercent;
    const previous = this.state.snapshot;
    const level = snapshot.capture.levelDbfs;
    const running =
      snapshot.capture.phase === "running" &&
      level !== null &&
      Number.isFinite(level);
    const sameInput =
      previous?.sessionId === snapshot.sessionId &&
      previous.selection.sourceId === snapshot.selection.sourceId;
    let history = sameInput && running ? this.state.levelHistory : [];
    if (running && history.at(-1)?.dbfs !== level) {
      const at = Date.now();
      history = [
        ...history.filter((item) => item.at >= at - 15_000),
        { at, dbfs: level },
      ].slice(-96);
    }
    this.publish({
      snapshot,
      phase: "ready",
      error: null,
      volumeDraft: confirmed ? null : this.state.volumeDraft,
      levelHistory: history,
    });
  }

  /** Listener-first avoids losing a transition between cold start and subscription. */
  start = async (): Promise<void> => {
    this.dispose();
    this.active = true;
    const generation = this.generation;
    this.publish({
      phase: "loading",
      error: null,
      commandError: null,
      pending: [],
      volumeDraft: null,
      configuringBot: false,
      configurationError: null,
      routeOperation: null,
      levelHistory: [],
    });
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
  };

  dispose = () => {
    this.active = false;
    this.generation++;
    this.unlisten?.();
    this.unlisten = undefined;
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
    const snapshot = this.state.snapshot;
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
              "Could not save this bot token. Check the token and try again. Your previous configuration is preserved.",
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
        const snapshot = this.state.snapshot;
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
    const initial = this.state.snapshot;
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
      if (this.state.snapshot?.selection.sourceId !== selection.sourceId) {
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
      const current = this.state.snapshot?.selection;
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
