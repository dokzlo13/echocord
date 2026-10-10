import { createContext, useContext } from "react";
import type { MixTelemetry } from "./generated";

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
export class TelemetryStore {
  visible = true;
  private latest: MixTelemetry | null = null;
  private histories = new Map<string, Ring>();
  private network: Network = {
    ping: null,
    buffer: null,
    drops: null,
    underruns: null,
    ping2m: new Ring(240),
    buffer2m: new Ring(240),
  };
  private lastLevel = Number.NEGATIVE_INFINITY;
  private lastNet = Number.NEGATIVE_INFINITY;
  private sessionId: number | null = null;
  private outputId: string | null = null;
  private peaks = new Map<string, { db: number; until: number }>();
  private visibleListeners = new Set<() => void>();
  setOutput(id: string | null) {
    if (this.outputId !== id) {
      this.outputId = id;
      this.histories.delete(`dst:${id}`);
      this.network = {
        ping: null,
        buffer: null,
        drops: null,
        underruns: null,
        ping2m: new Ring(240),
        buffer2m: new Ring(240),
      };
    }
  }
  resetHistory = () => {
    this.histories.clear();
    this.peaks.clear();
    this.network.ping2m = new Ring(240);
    this.network.buffer2m = new Ring(240);
    this.lastLevel = Number.NEGATIVE_INFINITY;
    this.lastNet = Number.NEGATIVE_INFINITY;
  };
  setSession(id: number) {
    if (this.sessionId !== id) {
      this.sessionId = id;
      for (const key of this.histories.keys())
        if (key.startsWith("dst:")) this.histories.delete(key);
      for (const key of this.peaks.keys())
        if (key.startsWith("dst:")) this.peaks.delete(key);
      this.network.ping2m = new Ring(240);
      this.network.buffer2m = new Ring(240);
      this.lastNet = Number.NEGATIVE_INFINITY;
    }
  }
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
    this.network.ping = value.gatewayPingMs;
    this.network.buffer = value.queuedFrames / 48;
    this.network.drops = value.droppedFrames;
    this.network.underruns = value.underrunFrames;
    if (!this.visible || (typeof document !== "undefined" && document.hidden))
      return;
    const now = performance.now();
    if (Number.isFinite(this.lastLevel) && now - this.lastLevel > 250)
      this.resetHistory();
    if (now - this.lastLevel >= 95) {
      this.lastLevel = now;
      const valid = new Set<string>();
      for (const s of value.inputs) {
        const key = `src:${s.stripId}`;
        valid.add(key);
        this.ring(key).push(fraction(s.levelDbfs));
      }
      if (this.outputId) {
        const key = `dst:${this.outputId}`;
        valid.add(key);
        this.ring(key).push(fraction(value.outputLevelDbfs));
      }
      for (const key of this.histories.keys())
        if (!valid.has(key)) this.histories.delete(key);
      for (const key of this.peaks.keys())
        if (!valid.has(key)) this.peaks.delete(key);
    }
    if (now - this.lastNet >= 490) {
      this.lastNet = now;
      this.network.ping2m.push(value.gatewayPingMs ?? Number.NaN);
      this.network.buffer2m.push(value.queuedFrames / 48);
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
    const db = input
      ? input.levelDbfs
      : key.startsWith("dst:")
        ? (this.latest?.outputLevelDbfs ?? null)
        : null;
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
        input?.preClip ??
        (key.startsWith("dst:") ? (this.latest?.mixPreClip ?? false) : false),
    };
  };
  net = () => this.network;
  clear() {
    this.latest = null;
    this.sessionId = null;
    this.histories.clear();
    this.peaks.clear();
    this.network = {
      ping: null,
      buffer: null,
      drops: null,
      underruns: null,
      ping2m: new Ring(240),
      buffer2m: new Ring(240),
    };
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
