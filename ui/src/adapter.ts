import type {
  AppError,
  AppSettings,
  ControlCommand,
  ControlSnapshot,
  MixTelemetry,
} from "./generated";
import { TelemetryStore } from "./telemetry";

export interface Transport {
  listen: (receive: (snapshot: ControlSnapshot) => void) => Promise<() => void>;
  listenTelemetry?: (
    receive: (value: MixTelemetry) => void,
  ) => Promise<() => void>;
  getSnapshot: () => Promise<ControlSnapshot>;
  dispatch: (command: ControlCommand) => Promise<void>;
  configureBot: (token: string) => Promise<void>;
  getVisible?: () => Promise<boolean>;
  listenVisibility?: (
    receive: (visible: boolean) => void,
  ) => Promise<() => void>;
  minimize?: () => Promise<void>;
  close?: () => Promise<void>;
  startDragging?: () => Promise<void>;
}
export interface ViewState {
  phase: "loading" | "ready" | "error";
  snapshot: ControlSnapshot | null;
  error: AppError | null;
  commandError: AppError | null;
  commandErrorCommand: ControlCommand | null;
  targetErrors: Readonly<
    Record<string, { error: AppError; command: ControlCommand }>
  >;
  pending: readonly string[];
  configuringBot: boolean;
  configurationError: AppError | null;
}
export function appError(value: unknown): AppError {
  const codes: AppError["code"][] = [
    "invalid-selection",
    "invalid-volume",
    "busy",
    "configuration",
    "authentication",
    "permission-denied",
    "capture-failed",
    "engine-stopped",
    "connection-failed",
    "cleanup-incomplete",
    "internal",
  ];
  if (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value &&
    typeof value.message === "string" &&
    codes.some((c) => c === value.code)
  )
    return value as AppError;
  return {
    code: "internal",
    message:
      "Could not reach Echocord. Reopen the app or retry loading its status.",
  };
}
function commandTarget(command: ControlCommand | null) {
  if (command && "stripId" in command) return `src:${command.stripId}`;
  if (command && "destinationId" in command)
    return `dst:${command.destinationId}`;
  return null;
}

function targetExists(snapshot: ControlSnapshot | null, target: string) {
  return Boolean(
    snapshot?.inputs.some((input) => target === `src:${input.stripId}`) ||
      snapshot?.destinations.some(
        (destination) => target === `dst:${destination.destinationId}`,
      ),
  );
}

function configurationError(value: unknown): AppError {
  // Configuration failures may contain token-bearing native exception text.
  // Select fixed copy by known code only; never forward the supplied message.
  const code =
    typeof value === "object" && value !== null && "code" in value
      ? value.code
      : null;
  switch (code) {
    case "authentication":
      return {
        code,
        message:
          "Discord rejected this bot token. Check it in the Developer Portal and try again.",
      };
    case "busy":
      return {
        code,
        message:
          "Disconnect Discord outputs and finish pending cleanup before replacing the token.",
      };
    case "connection-failed":
      return {
        code,
        message:
          "Could not reach Discord. Check your connection and try again.",
      };
    case "permission-denied":
      return {
        code,
        message:
          "Permission was denied while saving this bot. Check access and try again.",
      };
    default:
      return {
        code: "configuration",
        message:
          "Could not confirm this bot token was saved. Check bot status and try again.",
      };
  }
}

interface FaderWork {
  command: ControlCommand | null;
  timer: ReturnType<typeof setTimeout> | null;
  inFlight: boolean;
  waiters: Array<(accepted: boolean) => void>;
}
function controls(value: ControlSnapshot) {
  const {
    revision: _revision,
    telemetry: _telemetry,
    capture: _capture,
    ...rest
  } = value;
  return JSON.stringify(rest);
}
export class ControlAdapter {
  readonly telemetry = new TelemetryStore();
  private state: ViewState = {
    phase: "loading",
    snapshot: null,
    error: null,
    commandError: null,
    commandErrorCommand: null,
    targetErrors: {},
    pending: [],
    configuringBot: false,
    configurationError: null,
  };
  private listeners = new Set<() => void>();
  private generation = 0;
  private active = false;
  private revision = -1;
  private cleanup: Array<() => void> = [];
  private faders = new Map<string, FaderWork>();
  constructor(private transport: Transport) {}
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  private publish(patch: Partial<ViewState>) {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }
  private receive = (snapshot: ControlSnapshot, g: number) => {
    if (
      !this.active ||
      g !== this.generation ||
      snapshot.revision <= this.revision
    )
      return;
    this.revision = snapshot.revision;
    this.telemetry.setDestinations(snapshot.destinations);
    this.telemetry.ingest(snapshot.telemetry);
    this.telemetry.setInputs(snapshot.inputs);
    if (
      !this.state.snapshot ||
      controls(this.state.snapshot) !== controls(snapshot) ||
      this.state.phase !== "ready"
    )
      this.publish({
        snapshot,
        phase: "ready",
        error: null,
        targetErrors: Object.fromEntries(
          Object.entries(this.state.targetErrors).filter(([target]) =>
            targetExists(snapshot, target),
          ),
        ),
      });
  };
  start = async () => {
    this.dispose();
    this.active = true;
    this.revision = -1;
    const g = this.generation;
    this.publish({
      phase: "loading",
      error: null,
      commandError: null,
      commandErrorCommand: null,
      targetErrors: {},
      pending: [],
      configuringBot: false,
      configurationError: null,
    });
    this.telemetry.setVisible(!this.transport.getVisible);
    if (typeof document !== "undefined") {
      const onVisibility = () => {
        if (!document.hidden) this.telemetry.resetHistory();
      };
      document.addEventListener("visibilitychange", onVisibility);
      this.cleanup.push(() =>
        document.removeEventListener("visibilitychange", onVisibility),
      );
    }
    const own = async (p: Promise<() => void>) => {
      const un = await p;
      if (!this.active || g !== this.generation) un();
      else this.cleanup.push(un);
    };
    let visibilityEvent = false;
    try {
      await own(this.transport.listen((s) => this.receive(s, g)));
      if (!this.active || g !== this.generation) return;
      if (this.transport.listenTelemetry)
        await own(
          this.transport.listenTelemetry((t) => {
            if (this.active && g === this.generation) this.telemetry.ingest(t);
          }),
        );
      if (this.transport.listenVisibility)
        await own(
          this.transport.listenVisibility((v) => {
            if (this.active && g === this.generation) {
              visibilityEvent = true;
              this.telemetry.setVisible(v);
            }
          }),
        );
      const s = await this.transport.getSnapshot();
      this.receive(s, g);
      if (this.transport.getVisible) {
        const visible = await this.transport.getVisible();
        if (this.active && g === this.generation && !visibilityEvent)
          this.telemetry.setVisible(visible);
      }
    } catch (e) {
      if (this.active && g === this.generation)
        this.publish({ phase: "error", error: appError(e) });
    }
  };
  dispose = () => {
    this.active = false;
    this.generation++;
    for (const un of this.cleanup) un();
    this.cleanup = [];
    for (const f of this.faders.values()) {
      if (f.timer) clearTimeout(f.timer);
      for (const resolve of f.waiters) resolve(false);
    }
    this.faders.clear();
    this.telemetry.setVisible(false);
    this.telemetry.clear();
  };
  dismissError = (target?: string) => {
    const currentTarget = commandTarget(this.state.commandErrorCommand);
    const targetErrors = { ...this.state.targetErrors };
    if (target) delete targetErrors[target];
    this.publish({
      targetErrors,
      ...(target === undefined || target === currentTarget
        ? { commandError: null, commandErrorCommand: null }
        : {}),
    });
  };
  dispatch = async (command: ControlCommand): Promise<boolean> => {
    if (!this.active || this.state.phase !== "ready") return false;
    const g = this.generation;
    const key = JSON.stringify(command);
    const target = commandTarget(command);
    const targetErrors = { ...this.state.targetErrors };
    if (target) delete targetErrors[target];
    this.publish({
      pending: [...this.state.pending, key],
      targetErrors,
      ...(target === commandTarget(this.state.commandErrorCommand)
        ? { commandError: null, commandErrorCommand: null }
        : {}),
    });
    try {
      await this.transport.dispatch(command);
      return this.active && g === this.generation;
    } catch (e) {
      if (this.active && g === this.generation)
        this.publish({
          commandError: appError(e),
          commandErrorCommand: command,
          ...(target && targetExists(this.state.snapshot, target)
            ? {
                targetErrors: {
                  ...this.state.targetErrors,
                  [target]: { error: appError(e), command },
                },
              }
            : {}),
        });
      return false;
    } finally {
      if (this.active && g === this.generation) {
        const pending = [...this.state.pending];
        const i = pending.indexOf(key);
        if (i >= 0) pending.splice(i, 1);
        this.publish({ pending });
      }
    }
  };
  configureBot = async (token: string) => {
    if (!this.active || this.state.configuringBot) return false;
    const g = this.generation;
    this.publish({ configuringBot: true, configurationError: null });
    try {
      const request = this.transport.configureBot(token);
      token = "";
      await request;
      return this.active && g === this.generation;
    } catch (error) {
      if (this.active && g === this.generation)
        this.publish({ configurationError: configurationError(error) });
      return false;
    } finally {
      token = "";
      if (this.active && g === this.generation)
        this.publish({ configuringBot: false });
    }
  };
  setFader = (key: string, command: ControlCommand): Promise<boolean> => {
    if (!this.active || this.state.phase !== "ready")
      return Promise.resolve(false);
    let work = this.faders.get(key);
    if (!work) {
      work = { command: null, timer: null, inFlight: false, waiters: [] };
      this.faders.set(key, work);
    }
    for (const resolve of work.waiters) resolve(false);
    work.waiters = [];
    work.command = command;
    const pending = new Promise<boolean>((resolve) =>
      work.waiters.push(resolve),
    );
    if (!work.timer && !work.inFlight)
      work.timer = setTimeout(() => void this.flushFader(key), 80);
    return pending;
  };
  flushFader = async (key: string): Promise<void> => {
    const work = this.faders.get(key);
    if (!work || work.inFlight || !work.command) return;
    if (work.timer) clearTimeout(work.timer);
    work.timer = null;
    work.inFlight = true;
    const command = work.command;
    work.command = null;
    const waiters = work.waiters;
    work.waiters = [];
    const g = this.generation;
    const accepted = await this.dispatch(command);
    for (const resolve of waiters) resolve(accepted);
    if (g !== this.generation) return;
    work.inFlight = false;
    if (work.command)
      work.timer = setTimeout(() => void this.flushFader(key), 80);
    else this.faders.delete(key);
  };
  setSettings = (settings: AppSettings) =>
    this.dispatch({ type: "set-settings", settings });
  minimize = () => this.transport.minimize?.();
  close = () => this.transport.close?.();
  startDragging = () => this.transport.startDragging?.();
}
