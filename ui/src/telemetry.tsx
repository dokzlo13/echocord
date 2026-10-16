import { createContext, useContext } from "react";
import type {
  DestinationStrip,
  DestinationTelemetry,
  InputStrip,
  InputTelemetry,
  MixTelemetry,
} from "./generated";

export const AUDIO_FRESH_MS = 1000;
export const GATEWAY_FRESH_MS = 2500;
const RECENT_MS = 60_000;
export const fraction = (db: number | null) =>
  db === null ? 0 : Math.max(0, Math.min(1, (db + 60) / 60));

export class Ring {
  readonly buf: Float32Array;
  private readonly times: Float64Array;
  private next = 0;
  constructor(size: number) {
    this.buf = new Float32Array(size).fill(Number.NaN);
    this.times = new Float64Array(size).fill(Number.NaN);
  }
  push(value: number, time = performance.now()) {
    this.buf[this.next] = value;
    this.times[this.next] = time;
    this.next = (this.next + 1) % this.buf.length;
  }
  at(age: number) {
    return (
      this.buf[(this.next - 1 - age + this.buf.length) % this.buf.length] ??
      Number.NaN
    );
  }
  timeAt(age: number) {
    return (
      this.times[(this.next - 1 - age + this.buf.length) % this.buf.length] ??
      Number.NaN
    );
  }
  near(time: number, tolerance: number) {
    let result = Number.NaN;
    let distance = tolerance;
    for (let index = 0; index < this.buf.length; index++) {
      const delta = Math.abs(this.timeAt(index) - time);
      if (delta <= distance) {
        distance = delta;
        result = this.at(index);
      }
    }
    return result;
  }
}

class Observation {
  stamp = -1;
  valid = false;
  private received = 0;
  private initialAge = 0;
  age(now: number) {
    return this.stamp < 0
      ? null
      : this.initialAge + Math.max(0, now - this.received);
  }
  fresh(now: number, limit: number) {
    return this.valid && (this.age(now) ?? Infinity) <= limit;
  }
  update(stamp: number | null, clock: number, now: number) {
    if (
      stamp === null ||
      !Number.isFinite(stamp) ||
      stamp < 0 ||
      stamp > clock
    ) {
      this.valid = false;
      return false;
    }
    if (stamp < this.stamp) return false;
    if (stamp === this.stamp) {
      this.initialAge = Math.max(this.age(now) ?? 0, clock - stamp);
      this.received = now;
      return false; // A repeated origin cannot revive a null/stale observation.
    }
    this.stamp = stamp;
    this.valid = true;
    this.initialAge = clock - stamp;
    this.received = now;
    return true;
  }
}

class RecentCounters {
  private readonly times = new Float64Array(1024).fill(-Infinity);
  private readonly drops = new Float64Array(1024);
  private readonly gaps = new Float64Array(1024);
  private next = 0;
  private previous: { drops: number; gaps: number; time: number } | null = null;
  private ready = false;
  private incompleteUntil = -Infinity;
  reset() {
    this.times.fill(-Infinity);
    this.previous = null;
    this.ready = false;
    this.incompleteUntil = -Infinity;
  }
  observe(drops: number, gaps: number, time: number) {
    if (
      ![drops, gaps].every((value) => Number.isSafeInteger(value) && value >= 0)
    ) {
      this.reset();
      return;
    }
    const previous = this.previous;
    if (
      previous &&
      (drops < previous.drops ||
        gaps < previous.gaps ||
        time - previous.time > AUDIO_FRESH_MS)
    )
      this.reset();
    else if (previous) {
      this.ready = true;
      const lost = drops - previous.drops;
      const missing = gaps - previous.gaps;
      if (lost || missing) {
        this.incompleteUntil = Math.max(
          this.incompleteUntil,
          (this.times[this.next] ?? -Infinity) + RECENT_MS,
        );
        this.times[this.next] = time;
        this.drops[this.next] = lost;
        this.gaps[this.next] = missing;
        this.next = (this.next + 1) % this.times.length;
      }
    }
    this.previous = { drops, gaps, time };
  }
  read(now: number) {
    if (!this.ready || now < this.incompleteUntil) return null;
    let drops = 0;
    let gaps = 0;
    for (let index = 0; index < this.times.length; index++) {
      if ((this.times[index] ?? -Infinity) > now - RECENT_MS) {
        drops += this.drops[index] ?? 0;
        gaps += this.gaps[index] ?? 0;
      }
    }
    return { drops, gaps };
  }
}

export interface Network {
  ping: number | null;
  buffer: number | null;
  drops: number | null;
  underruns: number | null;
  recentDrops: number | null;
  recentUnderruns: number | null;
  ageMs: number | null;
  stale: boolean;
  pingStale: boolean;
  ping2m: Ring;
  buffer2m: Ring;
}
const emptyNetwork = (): Network => ({
  ping: null,
  buffer: null,
  drops: null,
  underruns: null,
  recentDrops: null,
  recentUnderruns: null,
  ageMs: null,
  stale: true,
  pingStale: false,
  ping2m: new Ring(240),
  buffer2m: new Ring(240),
});
interface InputState {
  active: boolean;
  identity: string;
  value: InputTelemetry | null;
  observed: Observation;
  lastLevel: number;
}
interface DestinationState {
  active: boolean;
  identity: string;
  sessionId: number;
  value: DestinationTelemetry | null;
  observed: Observation;
  gateway: Observation;
  ping: number | null;
  counters: RecentCounters;
  network: Network;
  lastLevel: number;
  lastBuffer: number;
  lastPing: number;
}
export class TelemetryStore {
  visible = true;
  private lastClock = -Infinity;
  private mix = new Observation();
  private mixClip = false;
  private inputs = new Map<string, InputState>();
  private outputs = new Map<string, DestinationState>();
  private histories = new Map<string, Ring>();
  private peaks = new Map<string, { db: number; until: number }>();
  private unknown = emptyNetwork();
  private visibleListeners = new Set<() => void>();
  constructor(private now: () => number = () => performance.now()) {}
  setInputs(inputs: InputStrip[]) {
    const valid = new Set(inputs.map((input) => input.stripId));
    for (const input of inputs) {
      const previous = this.inputs.get(input.stripId);
      if (!previous || previous.identity !== input.sourceId) {
        const observed = new Observation();
        if (previous) observed.stamp = previous.observed.stamp;
        this.histories.delete(`src:${input.stripId}`);
        this.peaks.delete(`src:${input.stripId}`);
        this.inputs.set(input.stripId, {
          active: input.phase === "capturing",
          identity: input.sourceId,
          value: null,
          observed,
          lastLevel: -Infinity,
        });
      }
      const state = this.inputs.get(input.stripId);
      if (state) {
        state.active = input.phase === "capturing";
        if (!state.active) {
          state.observed.valid = false;
          state.value = null;
          this.peaks.delete(`src:${input.stripId}`);
        }
      }
    }
    for (const id of this.inputs.keys())
      if (!valid.has(id)) {
        this.inputs.delete(id);
        this.histories.delete(`src:${id}`);
        this.peaks.delete(`src:${id}`);
      }
  }
  setDestinations(destinations: DestinationStrip[]) {
    const valid = new Set(
      destinations.map((destination) => destination.destinationId),
    );
    for (const destination of destinations) {
      const identity = `${destination.kind}:${destination.deviceId ?? `${destination.serverId}:${destination.channelId}`}`;
      const current = this.outputs.get(destination.destinationId);
      if (
        !current ||
        current.identity !== identity ||
        destination.sessionId > current.sessionId
      ) {
        this.histories.delete(`dst:${destination.destinationId}`);
        this.peaks.delete(`dst:${destination.destinationId}`);
        this.outputs.set(destination.destinationId, {
          active:
            destination.connection.phase === "connected" ||
            destination.connection.phase === "recovering",
          identity,
          sessionId: destination.sessionId,
          value: null,
          observed: new Observation(),
          gateway: new Observation(),
          ping: null,
          counters: new RecentCounters(),
          network: emptyNetwork(),
          lastLevel: -Infinity,
          lastBuffer: -Infinity,
          lastPing: -Infinity,
        });
      }
      const state = this.outputs.get(destination.destinationId);
      if (state) {
        state.active =
          destination.connection.phase === "connected" ||
          destination.connection.phase === "recovering";
        if (!state.active) {
          state.observed.valid = false;
          state.value = null;
          state.counters.reset();
          this.peaks.delete(`dst:${destination.destinationId}`);
          state.gateway.valid = false;
        }
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
    for (const input of this.inputs.values()) input.lastLevel = -Infinity;
    for (const output of this.outputs.values()) {
      output.network.ping2m = new Ring(240);
      output.network.buffer2m = new Ring(240);
      output.lastLevel = output.lastBuffer = output.lastPing = -Infinity;
      output.counters.reset();
    }
  };
  setVisible(visible: boolean) {
    if (this.visible === visible) return;
    if (visible) this.resetHistory();
    this.visible = visible;
    for (const listener of this.visibleListeners) listener();
  }
  subscribeVisible = (listener: () => void) => {
    this.visibleListeners.add(listener);
    return () => this.visibleListeners.delete(listener);
  };
  private level(
    key: string,
    db: number | null,
    time: number,
    now: number,
    previous: number,
  ) {
    if (db === null || !Number.isFinite(db)) {
      this.peaks.delete(key);
      return previous;
    }
    const peak = this.peaks.get(key);
    if (!peak || db > peak.db || now >= peak.until)
      this.peaks.set(key, { db, until: now + 1200 });
    if (
      !this.visible ||
      (typeof document !== "undefined" && document.hidden) ||
      time - previous < 95
    )
      return previous;
    let history = this.histories.get(key);
    if (!history) {
      history = new Ring(600);
      this.histories.set(key, history);
    }
    history.push(fraction(db), time);
    return time;
  }
  ingest(value: MixTelemetry) {
    if (
      !Number.isFinite(value.clockMs) ||
      value.clockMs < 0 ||
      value.clockMs < this.lastClock
    )
      return;
    const newerEnvelope = value.clockMs > this.lastClock;
    this.lastClock = value.clockMs;
    const now = this.now();
    const visible =
      this.visible && !(typeof document !== "undefined" && document.hidden);
    if (this.mix.update(value.mixObservedAtMs, value.clockMs, now))
      this.mixClip = value.mixPreClip;
    const inputIds = new Set(value.inputs.map((v) => v.stripId));
    const outputIds = new Set(value.destinations.map((v) => v.destinationId));
    for (const [id, input] of this.inputs)
      if (newerEnvelope && !inputIds.has(id)) {
        input.observed.valid = false;
        this.peaks.delete(`src:${id}`);
      }
    for (const [id, output] of this.outputs)
      if (newerEnvelope && !outputIds.has(id)) {
        output.observed.valid = output.gateway.valid = false;
        output.counters.reset();
        this.peaks.delete(`dst:${id}`);
      }
    for (const measurement of value.inputs) {
      const input = this.inputs.get(measurement.stripId);
      if (!input) continue;
      const changed = input.observed.update(
        measurement.observedAtMs,
        value.clockMs,
        now,
      );
      if (!input.active) input.observed.valid = false;
      if (!input.observed.valid)
        this.peaks.delete(`src:${measurement.stripId}`);
      if (changed && input.active) {
        input.value = measurement;
        if (input.observed.fresh(now, AUDIO_FRESH_MS))
          input.lastLevel = this.level(
            `src:${measurement.stripId}`,
            measurement.levelDbfs,
            now - (input.observed.age(now) ?? 0),
            now,
            input.lastLevel,
          );
      }
    }
    for (const measurement of value.destinations) {
      const output = this.outputs.get(measurement.destinationId);
      if (!output || measurement.sessionId !== output.sessionId) continue;
      const changed = output.observed.update(
        measurement.observedAtMs,
        value.clockMs,
        now,
      );
      if (!output.active) output.observed.valid = false;
      if (!output.observed.valid) {
        this.peaks.delete(`dst:${measurement.destinationId}`);
        output.counters.reset();
      }
      if (changed && output.active) {
        output.value = measurement;
        if (output.observed.fresh(now, AUDIO_FRESH_MS)) {
          const time = now - (output.observed.age(now) ?? 0);
          output.lastLevel = this.level(
            `dst:${measurement.destinationId}`,
            measurement.levelDbfs,
            time,
            now,
            output.lastLevel,
          );
          output.counters.observe(
            measurement.droppedFrames,
            measurement.underrunFrames,
            time,
          );
          if (visible && time - output.lastBuffer >= 490) {
            output.network.buffer2m.push(measurement.queuedFrames / 48, time);
            output.lastBuffer = time;
          }
        }
      } else if (measurement.observedAtMs === null) output.counters.reset();
      if (
        output.gateway.update(
          measurement.gatewayObservedAtMs,
          value.clockMs,
          now,
        )
      ) {
        output.ping = measurement.gatewayPingMs;
        const time = now - (output.gateway.age(now) ?? 0);
        if (
          visible &&
          output.gateway.fresh(now, GATEWAY_FRESH_MS) &&
          time - output.lastPing >= 490
        ) {
          output.network.ping2m.push(
            measurement.gatewayPingMs ?? Number.NaN,
            time,
          );
          output.lastPing = time;
        }
      }
    }
  }
  history = (key: string) => this.histories.get(key);
  read = (key: string) => {
    const source = key.startsWith("src:")
      ? this.inputs.get(key.slice(4))
      : null;
    const output = key.startsWith("dst:")
      ? this.outputs.get(key.slice(4))
      : null;
    const state = source ?? output;
    const now = this.now();
    const fresh = state?.observed.fresh(now, AUDIO_FRESH_MS) ?? false;
    const measured = state?.value?.levelDbfs;
    const db =
      fresh &&
      measured !== undefined &&
      measured !== null &&
      Number.isFinite(measured)
        ? measured
        : null;
    const peak = this.peaks.get(key);
    return {
      db,
      peak: db !== null && peak && now < peak.until ? peak.db : null,
      stale: !fresh || db === null,
      preClip:
        fresh &&
        (source
          ? Boolean(source.value?.preClip)
          : this.mix.fresh(now, AUDIO_FRESH_MS) && this.mixClip),
    };
  };
  net = (id?: string): Network => {
    const output = id
      ? this.outputs.get(id)
      : this.outputs.values().next().value;
    if (!output) return this.unknown;
    const now = this.now();
    const fresh = output.observed.fresh(now, AUDIO_FRESH_MS);
    const recent = fresh ? output.counters.read(now) : null;
    Object.assign(output.network, {
      ping: output.gateway.fresh(now, GATEWAY_FRESH_MS) ? output.ping : null,
      pingStale:
        output.gateway.stamp >= 0 &&
        !output.gateway.fresh(now, GATEWAY_FRESH_MS),
      buffer: fresh && output.value ? output.value.queuedFrames / 48 : null,
      drops: output.value?.droppedFrames ?? null,
      underruns: output.value?.underrunFrames ?? null,
      recentDrops: recent?.drops ?? null,
      recentUnderruns: recent?.gaps ?? null,
      ageMs: output.observed.age(now),
      stale: !fresh,
    });
    return output.network;
  };
  clear() {
    this.lastClock = -Infinity;
    this.mix = new Observation();
    this.mixClip = false;
    this.inputs.clear();
    this.outputs.clear();
    this.histories.clear();
    this.peaks.clear();
    this.unknown = emptyNetwork();
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
