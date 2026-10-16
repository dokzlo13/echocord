import { createContext, useContext } from "react";
import type {
  DestinationStrip,
  DestinationTelemetry,
  InputStrip,
  InputTelemetry,
  MixTelemetry,
  TimingObservation,
  TimingPhase,
  TimingResetReason,
} from "./generated";

export const AUDIO_FRESH_MS = 1000;
export const GATEWAY_FRESH_MS = 2500;
export const LIMITER_WINDOW_MS = 10_000;
const RECENT_MS = 60_000;
/** Native u32 counters saturate here; a saturated counter can no longer show new events. */
export const U32_MAX = 4_294_967_295;
/**
 * The correction controller is bounded to ±0.001 of nominal rate; ppm derived from the
 * applied ratio in floating point can land a hair outside that bound.
 */
const MAX_ADJUSTMENT_PPM = 1000.001;
/** Ten seconds of 48 kHz frames bounds any sane stage capacity. */
const MAX_STAGE_FRAMES = 480_000;
/** Opaque lifetime/mode tuple: printable ASCII, compared only for equality. */
const EPOCH_FORMAT = /^[\x20-\x7e]{1,128}$/;
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
  /** A counter at its native maximum can no longer report increases. */
  dropsSaturated = false;
  gapsSaturated = false;
  reset() {
    this.times.fill(-Infinity);
    this.previous = null;
    this.ready = false;
    this.incompleteUntil = -Infinity;
    this.dropsSaturated = this.gapsSaturated = false;
  }
  observe(drops: number, gaps: number, time: number) {
    if (
      ![drops, gaps].every(
        (value) =>
          Number.isSafeInteger(value) && value >= 0 && value <= U32_MAX,
      )
    ) {
      this.reset();
      return;
    }
    if (drops === U32_MAX) this.dropsSaturated = true;
    if (gaps === U32_MAX) this.gapsSaturated = true;
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
    return {
      drops: this.dropsSaturated ? null : drops,
      gaps: this.gapsSaturated ? null : gaps,
    };
  }
}

/** Recent correction events from observed increases within one epoch. */
export type RecentTiming =
  | { restarts: number; gapMs: number; droppedMs: number }
  | "limit";
/**
 * Native publishes at most ten observations a second, so a persistent problem yields 600
 * events per 60 s window; 1024 slots keep that window complete. Faster reporting overflows
 * and is shown as incomplete rather than as a smaller total.
 */
const RECENT_EVENTS = 1024;
class RecentTimingWindow {
  private readonly times = new Float64Array(RECENT_EVENTS).fill(-Infinity);
  private readonly restarts = new Float64Array(RECENT_EVENTS);
  private readonly gaps = new Float64Array(RECENT_EVENTS);
  private readonly drops = new Float64Array(RECENT_EVENTS);
  private next = 0;
  private previous: {
    restarts: number;
    gaps: number;
    drops: number;
    time: number;
  } | null = null;
  private ready = false;
  private saturated = false;
  private incompleteUntil = -Infinity;
  reset() {
    this.times.fill(-Infinity);
    this.previous = null;
    this.ready = false;
    this.saturated = false;
    this.incompleteUntil = -Infinity;
  }
  observe(value: TimingObservation, time: number) {
    const current = {
      restarts: value.resyncCount,
      gaps: value.gapOutputFrames,
      drops: value.droppedInputFrames,
      time,
    };
    // Native clamps u64 counters to the safe-integer limit; increases past it are invisible.
    if (
      [current.restarts, current.gaps, current.drops].some(
        (count) => count === Number.MAX_SAFE_INTEGER,
      )
    )
      this.saturated = true;
    const previous = this.previous;
    if (
      previous &&
      (current.restarts < previous.restarts ||
        current.gaps < previous.gaps ||
        current.drops < previous.drops ||
        time - previous.time > AUDIO_FRESH_MS)
    ) {
      const saturated = this.saturated;
      this.reset();
      this.saturated =
        saturated &&
        [current.restarts, current.gaps, current.drops].some(
          (count) => count === Number.MAX_SAFE_INTEGER,
        );
    } else if (previous) {
      this.ready = true;
      const restarts = current.restarts - previous.restarts;
      const gaps = current.gaps - previous.gaps;
      const drops = current.drops - previous.drops;
      if (restarts || gaps || drops) {
        this.incompleteUntil = Math.max(
          this.incompleteUntil,
          (this.times[this.next] ?? -Infinity) + RECENT_MS,
        );
        this.times[this.next] = time;
        this.restarts[this.next] = restarts;
        this.gaps[this.next] = gaps;
        this.drops[this.next] = drops;
        this.next = (this.next + 1) % this.times.length;
      }
    }
    this.previous = current;
  }
  read(now: number): RecentTiming | null {
    if (this.saturated) return "limit";
    if (!this.ready || now < this.incompleteUntil) return null;
    let restarts = 0;
    let gaps = 0;
    let drops = 0;
    for (let index = 0; index < this.times.length; index++)
      if ((this.times[index] ?? -Infinity) > now - RECENT_MS) {
        restarts += this.restarts[index] ?? 0;
        gaps += this.gaps[index] ?? 0;
        drops += this.drops[index] ?? 0;
      }
    return { restarts, gapMs: gaps / 48, droppedMs: drops / 48 };
  }
}

/** State facts and audio facts carry separate origins; neither renews the other. */
interface TimingState {
  epoch: string;
  stateValue: TimingObservation | null;
  audioValue: TimingObservation | null;
  state: Observation;
  audio: Observation;
  reset: Observation;
  recent: RecentTimingWindow;
}
export type SyncState = "running" | "priming" | "restarting" | "off" | "stale";
export interface TimingView {
  state: SyncState;
  adjustmentPpm: number | null;
  queuedMs: number | null;
  targetMs: number | null;
  recent: RecentTiming | null;
  lastReset: { reason: TimingResetReason; ageMs: number } | null;
  ageMs: number | null;
}
export interface LimiterView {
  state: "none" | "stale" | "idle" | "reducing";
  maxDb: number | null;
}
const PHASES: ReadonlySet<TimingPhase> = new Set(["off", "priming", "running"]);
const REASONS: ReadonlySet<TimingResetReason> = new Set([
  "discontinuity",
  "stalled-input",
  "paused-output",
  "starved",
  "overrun",
]);
const count = (value: unknown) =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const frames = (value: unknown, max: number) =>
  value === null || (count(value) && (value as number) <= max);
const stamp = (value: unknown) =>
  value === null || (typeof value === "number" && Number.isFinite(value));
export const validEpoch = (value: unknown) =>
  typeof value === "string" && EPOCH_FORMAT.test(value) && value.trim() !== "";
/** Rejects malformed observations instead of displaying any part of them. */
export function validTiming(value: TimingObservation) {
  const capacity = value.capacityFrames;
  const ppm = value.appliedAdjustmentPpm;
  return (
    validEpoch(value.epoch) &&
    PHASES.has(value.phase) &&
    count(capacity) &&
    capacity > 0 &&
    capacity <= MAX_STAGE_FRAMES &&
    frames(value.queuedFrames, capacity) &&
    frames(value.targetFrames, capacity) &&
    (ppm === null ||
      (typeof ppm === "number" &&
        Number.isFinite(ppm) &&
        Math.abs(ppm) <= MAX_ADJUSTMENT_PPM)) &&
    (value.phase !== "off" || ppm === null) &&
    count(value.gapOutputFrames) &&
    count(value.droppedInputFrames) &&
    count(value.resyncCount) &&
    (value.lastResetReason === null || REASONS.has(value.lastResetReason)) &&
    (value.lastResetReason === null) === (value.lastResetAtMs === null) &&
    stamp(value.lastResetAtMs) &&
    stamp(value.stateObservedAtMs) &&
    stamp(value.audioObservedAtMs)
  );
}
/** Origins at or before the fence belong to a retired stage and can never be accepted. */
function fenced(fence: number) {
  const observation = new Observation();
  observation.stamp = fence;
  return observation;
}
const newTiming = (epoch: string, fence: number): TimingState => ({
  epoch,
  stateValue: null,
  audioValue: null,
  state: fenced(fence),
  audio: fenced(fence),
  reset: new Observation(),
  recent: new RecentTimingWindow(),
});
interface TimingHolder {
  timing: TimingState | null;
  /** Latest origin seen for this identity before its stage was retired. */
  timingFence: number;
}
const latestOrigin = (holder: TimingHolder) =>
  Math.max(
    holder.timingFence,
    holder.timing?.state.stamp ?? -1,
    holder.timing?.audio.stamp ?? -1,
  );
function retireTiming(holder: TimingHolder) {
  holder.timingFence = latestOrigin(holder);
  holder.timing = null;
}

export interface Network {
  ping: number | null;
  buffer: number | null;
  drops: number | null;
  underruns: number | null;
  recentDrops: number | null;
  recentUnderruns: number | null;
  dropsSaturated: boolean;
  underrunsSaturated: boolean;
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
  dropsSaturated: false,
  underrunsSaturated: false,
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
  timing: TimingState | null;
  timingFence: number;
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
  timing: TimingState | null;
  timingFence: number;
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
  private stageList: readonly string[] = [];
  private stageListeners = new Set<() => void>();
  private limiterRing = new Ring(160);
  private limiterLatest: number | null = null;
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
          timing: null,
          // A repeated envelope must not attach the previous identity's stage.
          timingFence: previous
            ? Math.max(latestOrigin(previous), previous.observed.stamp)
            : -1,
        });
      }
      const state = this.inputs.get(input.stripId);
      if (state) {
        // Source mute is mix-only and keeps its timing; only leaving capture ends the stage.
        state.active = input.phase === "capturing";
        if (!state.active) {
          state.observed.valid = false;
          state.value = null;
          retireTiming(state);
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
    this.publishStages();
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
          timing: null,
          timingFence: current
            ? Math.max(latestOrigin(current), current.observed.stamp)
            : -1,
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
          retireTiming(state);
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
    this.publishStages();
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
      output.timing?.recent.reset();
    }
    for (const input of this.inputs.values()) input.timing?.recent.reset();
    this.resetLimiter();
  };
  /** Forgets reductions from an earlier limiter mode; call when the setting changes. */
  resetLimiter = () => {
    this.limiterRing = new Ring(160);
    this.limiterLatest = null;
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
    if (this.mix.update(value.mixObservedAtMs, value.clockMs, now)) {
      this.mixClip = value.mixPreClip;
      const reduction = value.limiterReductionDb;
      const next =
        typeof reduction === "number" &&
        Number.isFinite(reduction) &&
        reduction >= 0 &&
        reduction <= 96
          ? reduction
          : null;
      // A new processing period must not show reductions from before it began.
      if (next !== null && this.limiterLatest === null)
        this.limiterRing = new Ring(160);
      this.limiterLatest = next;
      if (next !== null)
        this.limiterRing.push(next, now - (this.mix.age(now) ?? 0));
    }
    const inputIds = new Set(value.inputs.map((v) => v.stripId));
    const outputIds = new Set(value.destinations.map((v) => v.destinationId));
    for (const [id, input] of this.inputs)
      if (newerEnvelope && !inputIds.has(id)) {
        input.observed.valid = false;
        retireTiming(input);
        this.peaks.delete(`src:${id}`);
      }
    for (const [id, output] of this.outputs)
      if (newerEnvelope && !outputIds.has(id)) {
        output.observed.valid = output.gateway.valid = false;
        output.counters.reset();
        retireTiming(output);
        this.peaks.delete(`dst:${id}`);
      }
    for (const measurement of value.inputs) {
      const input = this.inputs.get(measurement.stripId);
      if (!input) continue;
      this.ingestTiming(
        input,
        input.active ? measurement.timing : undefined,
        value.clockMs,
        now,
        newerEnvelope,
      );
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
      if (!output) continue;
      if (measurement.sessionId !== output.sessionId) {
        // Another session's record says nothing current about this session's stage.
        if (newerEnvelope) retireTiming(output);
        continue;
      }
      this.ingestTiming(
        output,
        output.active ? measurement.timing : undefined,
        value.clockMs,
        now,
        newerEnvelope,
      );
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
    this.publishStages();
  }
  /**
   * Absent timing means no correction stage exists here; it is never shown as Off.
   * Fields are accepted only together with their own origin, so a fresh state
   * origin cannot renew stale audio facts and an older origin cannot replace newer facts.
   */
  private ingestTiming(
    holder: TimingHolder,
    timing: TimingObservation | undefined,
    clock: number,
    now: number,
    newerEnvelope: boolean,
  ) {
    if (!timing) {
      if (newerEnvelope) retireTiming(holder);
      return;
    }
    const fence = latestOrigin(holder);
    // Future origins are invalid and must never become a fence themselves.
    const origin = (value: unknown) =>
      typeof value === "number" && Number.isFinite(value) && value <= clock
        ? value
        : -1;
    const newest = Math.max(
      origin(timing.stateObservedAtMs),
      origin(timing.audioObservedAtMs),
    );
    const epoch = validEpoch(timing.epoch) ? timing.epoch : "";
    const current = holder.timing?.epoch === epoch ? holder.timing : null;
    // A record no newer than everything already seen belongs to a retired stage or lifetime.
    if (!current && newest <= fence) return;
    if (!epoch || !validTiming(timing)) {
      // Newer malformed data is listed as No recent data and cannot be revived by a
      // repeat; an older malformed record cannot blank a valid current stage.
      if (newest > fence) holder.timing = newTiming(epoch, newest);
      return;
    }
    const state = current ?? newTiming(epoch, fence);
    holder.timing = state;
    const accepted = (observation: Observation, origin: number | null) => {
      observation.update(origin, clock, now);
      return (
        origin !== null && observation.valid && observation.stamp === origin
      );
    };
    if (accepted(state.state, timing.stateObservedAtMs)) {
      state.stateValue = timing;
      if (timing.lastResetAtMs === null) state.reset = new Observation();
      else state.reset.update(timing.lastResetAtMs, clock, now);
    }
    const previousAudio = state.audio.stamp;
    if (accepted(state.audio, timing.audioObservedAtMs)) {
      state.audioValue = timing;
      if (
        state.audio.stamp !== previousAudio &&
        state.audio.fresh(now, AUDIO_FRESH_MS)
      )
        state.recent.observe(timing, now - (state.audio.age(now) ?? 0));
    }
  }
  private publishStages() {
    const next: string[] = [];
    for (const [id, input] of this.inputs)
      if (input.timing) next.push(`src:${id}`);
    for (const [id, output] of this.outputs)
      if (output.timing) next.push(`dst:${id}`);
    if (
      next.length === this.stageList.length &&
      next.every((key, index) => key === this.stageList[index])
    )
      return;
    this.stageList = next;
    for (const listener of this.stageListeners) listener();
  }
  /** Changes only when a correction stage appears or disappears. */
  subscribeStages = (listener: () => void) => {
    this.stageListeners.add(listener);
    return () => this.stageListeners.delete(listener);
  };
  stages = () => this.stageList;
  timing = (key: string): TimingView | null => {
    const holder = key.startsWith("src:")
      ? this.inputs.get(key.slice(4))
      : key.startsWith("dst:")
        ? this.outputs.get(key.slice(4))
        : undefined;
    const state = holder?.timing;
    if (!state) return null;
    const now = this.now();
    const facts = state.stateValue;
    const audio = state.audioValue;
    const stateFresh = !!facts && state.state.fresh(now, AUDIO_FRESH_MS);
    const audioFresh =
      stateFresh && !!audio && state.audio.fresh(now, AUDIO_FRESH_MS);
    const phase = facts?.phase;
    const sync: SyncState = !stateFresh
      ? "stale"
      : phase === "off"
        ? "off"
        : phase === "running"
          ? "running"
          : facts?.lastResetReason
            ? "restarting"
            : "priming";
    const correcting = sync !== "stale" && sync !== "off";
    return {
      state: sync,
      adjustmentPpm:
        audioFresh && correcting ? (audio?.appliedAdjustmentPpm ?? null) : null,
      queuedMs:
        audioFresh && audio?.queuedFrames != null
          ? audio.queuedFrames / 48
          : null,
      targetMs:
        correcting && facts?.targetFrames != null
          ? facts.targetFrames / 48
          : null,
      recent: audioFresh ? state.recent.read(now) : null,
      lastReset:
        stateFresh &&
        facts?.lastResetReason &&
        state.reset.valid &&
        state.reset.stamp === facts.lastResetAtMs
          ? {
              reason: facts.lastResetReason,
              ageMs: state.reset.age(now) ?? 0,
            }
          : null,
      // Age describes what the row shows: the state while it is stale, otherwise the
      // audio facts. Only accepted facts have an origin; a fenced placeholder has none.
      ageMs: !facts
        ? null
        : stateFresh && audio
          ? state.audio.age(now)
          : state.state.age(now),
    };
  };
  limiter = (): LimiterView => {
    const now = this.now();
    if (this.limiterLatest === null) return { state: "none", maxDb: null };
    if (!this.mix.fresh(now, AUDIO_FRESH_MS))
      return { state: "stale", maxDb: null };
    let max = 0;
    for (let age = 0; age < this.limiterRing.buf.length; age++) {
      const time = this.limiterRing.timeAt(age);
      if (!Number.isFinite(time) || now - time > LIMITER_WINDOW_MS) continue;
      const reduction = this.limiterRing.at(age);
      if (Number.isFinite(reduction)) max = Math.max(max, reduction);
    }
    return max >= 0.1
      ? { state: "reducing", maxDb: max }
      : { state: "idle", maxDb: 0 };
  };
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
      dropsSaturated:
        output.counters.dropsSaturated ||
        output.value?.droppedFrames === U32_MAX,
      underrunsSaturated:
        output.counters.gapsSaturated ||
        output.value?.underrunFrames === U32_MAX,
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
    this.resetLimiter();
    this.publishStages();
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
