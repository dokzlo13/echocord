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
    this.telemetry.setOutput(snapshot.destinations[0]?.destinationId ?? null);
    this.telemetry.ingest(snapshot.telemetry);
    if (
      !this.state.snapshot ||
      controls(this.state.snapshot) !== controls(snapshot) ||
      this.state.phase !== "ready"
    )
      this.publish({ snapshot, phase: "ready", error: null });
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
      pending: [],
      configuringBot: false,
      configurationError: null,
    });
    this.telemetry.setVisible(!this.transport.getVisible);
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
  dismissError = () => this.publish({ commandError: null });
  dispatch = async (command: ControlCommand): Promise<boolean> => {
    if (!this.active || this.state.phase !== "ready") return false;
    const g = this.generation;
    const key = JSON.stringify(command);
    this.publish({ pending: [...this.state.pending, key], commandError: null });
    try {
      await this.transport.dispatch(command);
      return this.active && g === this.generation;
    } catch (e) {
      if (this.active && g === this.generation)
        this.publish({ commandError: appError(e) });
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
    } catch {
      if (this.active && g === this.generation)
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
