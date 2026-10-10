import { createContext, useContext } from "react";
import type {
  DestinationStrip,
  DestinationTelemetry,
  MixTelemetry,
} from "./generated";

export const fraction = (db: number | null) =>
  db === null ? 0 : Math.max(0, Math.min(1, (db + 60) / 60));
export class Ring {
  readonly buf: Float32Array;
  private next = 0;
  constructor(size: number) {
    this.buf = new Float32Array(size);
    this.buf.fill(Number.NaN);
  }
  push(value: number) {
    this.buf[this.next] = value;
    this.next = (this.next + 1) % this.buf.length;
  }
  at(age: number) {
    return (
      this.buf[(this.next - 1 - age + this.buf.length) % this.buf.length] ??
      Number.NaN
    );
  }
}
export interface Network {
  ping: number | null;
  buffer: number | null;
  drops: number | null;
  underruns: number | null;
  ping2m: Ring;
  buffer2m: Ring;
}
const emptyNetwork = (): Network => ({
  ping: null,
  buffer: null,
  drops: null,
  underruns: null,
  ping2m: new Ring(240),
  buffer2m: new Ring(240),
});
interface DestinationState {
  sessionId: number;
  identity: string;
  value: DestinationTelemetry | null;
  network: Network;
  lastNet: number;
}
export class TelemetryStore {
  visible = true;
  private latest: MixTelemetry | null = null;
  private histories = new Map<string, Ring>();
  private outputs = new Map<string, DestinationState>();
  private unknown = emptyNetwork();
  private lastLevel = Number.NEGATIVE_INFINITY;
  private peaks = new Map<string, { db: number; until: number }>();
  private visibleListeners = new Set<() => void>();
  setDestinations(destinations: DestinationStrip[]) {
    const valid = new Set(destinations.map((d) => d.destinationId));
    for (const d of destinations) {
      const identity = `${d.kind}:${d.deviceId ?? `${d.serverId}:${d.channelId}`}`;
      const current = this.outputs.get(d.destinationId);
      if (
        !current ||
        identity !== current.identity ||
        d.sessionId > current.sessionId
      ) {
        this.histories.delete(`dst:${d.destinationId}`);
        this.peaks.delete(`dst:${d.destinationId}`);
        this.outputs.set(d.destinationId, {
          sessionId: d.sessionId,
          identity,
          value: null,
          network: emptyNetwork(),
          lastNet: Number.NEGATIVE_INFINITY,
        });
      }
    }
    for (const id of this.outputs.keys())
      if (!valid.has(id)) {
        this.outputs.delete(id);
        this.histories.delete(`dst:${id}`);
        this.peaks.delete(`dst:${id}`);
      }
  }
  resetHistory = () => {
    this.histories.clear();
    this.peaks.clear();
    this.lastLevel = Number.NEGATIVE_INFINITY;
    for (const d of this.outputs.values()) {
      d.network.ping2m = new Ring(240);
      d.network.buffer2m = new Ring(240);
      d.lastNet = Number.NEGATIVE_INFINITY;
    }
  };
  setVisible(visible: boolean) {
    if (this.visible === visible) return;
    if (visible) this.resetHistory();
    this.visible = visible;
    for (const f of this.visibleListeners) f();
  }
  subscribeVisible = (f: () => void) => {
    this.visibleListeners.add(f);
    return () => this.visibleListeners.delete(f);
  };
  ingest(value: MixTelemetry) {
    this.latest = value;
    const now = performance.now();
    const visible =
      this.visible && !(typeof document !== "undefined" && document.hidden);
    if (
      visible &&
      Number.isFinite(this.lastLevel) &&
      now - this.lastLevel > 250
    )
      this.resetHistory();
    for (const v of value.destinations) {
      const d = this.outputs.get(v.destinationId);
      if (!d || v.sessionId < d.sessionId) continue;
      if (v.sessionId > d.sessionId) {
        d.sessionId = v.sessionId;
        d.network = emptyNetwork();
        d.lastNet = Number.NEGATIVE_INFINITY;
        this.histories.delete(`dst:${v.destinationId}`);
        this.peaks.delete(`dst:${v.destinationId}`);
      }
      d.value = v;
      d.network.ping = v.gatewayPingMs;
      d.network.buffer = v.queuedFrames / 48;
      d.network.drops = v.droppedFrames;
      d.network.underruns = v.underrunFrames;
      if (visible && now - d.lastNet >= 490) {
        d.lastNet = now;
        d.network.ping2m.push(v.gatewayPingMs ?? Number.NaN);
        d.network.buffer2m.push(v.queuedFrames / 48);
      }
    }
    if (!visible) return;
    if (now - this.lastLevel >= 95) {
      this.lastLevel = now;
      const valid = new Set<string>();
      for (const input of value.inputs) {
        const key = `src:${input.stripId}`;
        valid.add(key);
        this.ring(key).push(
          input.levelDbfs === null ? Number.NaN : fraction(input.levelDbfs),
        );
      }
      for (const [id, d] of this.outputs) {
        const key = `dst:${id}`;
        valid.add(key);
        this.ring(key).push(
          d.value?.levelDbfs == null ? Number.NaN : fraction(d.value.levelDbfs),
        );
      }
      for (const key of this.histories.keys())
        if (!valid.has(key)) this.histories.delete(key);
      for (const key of this.peaks.keys())
        if (!valid.has(key)) this.peaks.delete(key);
    }
  }
  private ring(key: string) {
    let r = this.histories.get(key);
    if (!r) {
      r = new Ring(600);
      this.histories.set(key, r);
    }
    return r;
  }
  history = (key: string) => this.histories.get(key);
  read = (key: string) => {
    const input = key.startsWith("src:")
      ? this.latest?.inputs.find((s) => s.stripId === key.slice(4))
      : null;
    const output = key.startsWith("dst:")
      ? this.outputs.get(key.slice(4))
      : null;
    const db = input?.levelDbfs ?? output?.value?.levelDbfs ?? null;
    const now = performance.now();
    let held = this.peaks.get(key);
    if (db === null) {
      this.peaks.delete(key);
      held = undefined;
    } else if (!held || db > held.db || now >= held.until) {
      held = { db, until: now + 1200 };
      this.peaks.set(key, held);
    }
    return {
      db,
      peak: held?.db ?? null,
      preClip:
        input?.preClip ?? (output ? (this.latest?.mixPreClip ?? false) : false),
    };
  };
  net = (id?: string) =>
    id
      ? (this.outputs.get(id)?.network ?? this.unknown)
      : (this.outputs.values().next().value?.network ?? this.unknown);
  clear() {
    this.latest = null;
    this.outputs.clear();
    this.histories.clear();
    this.peaks.clear();
    this.unknown = emptyNetwork();
    this.lastLevel = Number.NEGATIVE_INFINITY;
  }
}
const Context = createContext<TelemetryStore | null>(null);
export const TelemetryProvider = Context.Provider;
export function useTelemetryStore() {
  const store = useContext(Context);
  if (!store) throw Error("Missing telemetry store");
  return store;
}
/** Pause all animation work while the document or native window is hidden. */
export function paint(
  store: TelemetryStore,
  draw: (now: number) => void,
  hz = 20,
) {
  let raf = 0;
  let last = Number.NEGATIVE_INFINITY;
  let disposed = false;
  const loop = (now: number) => {
    raf = 0;
    if (
      disposed ||
      !store.visible ||
      (typeof document !== "undefined" && document.hidden)
    )
      return;
    if (now - last >= 1000 / hz) {
      last = now;
      draw(now);
    }
    raf = requestAnimationFrame(loop);
  };
  const resume = () => {
    cancelAnimationFrame(raf);
    raf = 0;
    if (
      !disposed &&
      store.visible &&
      !(typeof document !== "undefined" && document.hidden)
    )
      raf = requestAnimationFrame(loop);
  };
  const un = store.subscribeVisible(resume);
  document.addEventListener("visibilitychange", resume);
  resume();
  return () => {
    disposed = true;
    cancelAnimationFrame(raf);
    un();
    document.removeEventListener("visibilitychange", resume);
  };
}
