import { expect, test } from "@playwright/test";
import { fixtureSnapshot, freshTelemetry } from "../src/fixtures";
import type { MixTelemetry } from "../src/generated";
import { TelemetryStore } from "../src/telemetry";

function setup() {
  let now = 1000;
  const fixture = fixtureSnapshot("connected");
  const store = new TelemetryStore(() => now);
  store.setInputs(fixture.inputs);
  store.setDestinations(fixture.destinations);
  const sample = (patch: Partial<MixTelemetry> = {}) =>
    freshTelemetry({ ...fixture.telemetry, ...patch }, now);
  const send = (patch: Partial<MixTelemetry> = {}) => {
    const value = sample(patch);
    store.ingest(value);
    return value;
  };
  return {
    fixture,
    store,
    sample,
    send,
    advance: (ms: number) => {
      now += ms;
    },
    time: () => now,
  };
}

test("control envelopes and repeated origins cannot refresh frozen audio or held peaks", () => {
  const t = setup();
  const value = t.send();
  expect(t.store.read("src:strip:1").db).toBe(-18);
  const history = t.store.history("src:strip:1");
  t.advance(1001);
  t.store.ingest({ ...value, clockMs: t.time() });
  expect(t.store.read("src:strip:1")).toMatchObject({
    db: null,
    peak: null,
    stale: true,
  });
  expect(history?.timeAt(0)).toBe(1000);
  expect(Number.isNaN(history?.at(1))).toBe(true);
  t.advance(1);
  t.send();
  expect(t.store.read("src:strip:1").db).toBe(-18);
  expect(history?.timeAt(0)).toBe(2002);
});

test("transport delay counts immediately and out-of-order envelopes cannot restore older audio", () => {
  const t = setup();
  const value = t.sample();
  t.advance(1200);
  t.store.ingest({ ...value, clockMs: t.time() });
  expect(t.store.read("src:strip:1").db).toBeNull();
  t.send();
  t.store.ingest(value);
  expect(t.store.history("src:strip:1")?.timeAt(0)).toBe(2200);
  expect(t.store.read("src:strip:1").db).toBe(-18);
});

test("null origin invalidates immediately without history insertion or same-origin revival", () => {
  const t = setup();
  const value = t.send();
  const history = t.store.history("src:strip:1");
  t.advance(100);
  t.store.ingest({
    ...value,
    clockMs: t.time(),
    inputs: value.inputs.map((v) => ({ ...v, observedAtMs: null })),
  });
  expect(t.store.read("src:strip:1").db).toBeNull();
  t.store.ingest({ ...value, clockMs: t.time() });
  expect(t.store.read("src:strip:1").db).toBeNull();
  expect(history?.timeAt(0)).toBe(1000);
  t.send();
  expect(t.store.read("src:strip:1").db).toBe(-18);
});

test("gateway and audio age independently with their own reporting cadence", () => {
  const t = setup();
  const value = t.send({
    destinations: t.fixture.telemetry.destinations.map((v) => ({
      ...v,
      gatewayPingMs: 42,
    })),
  });
  t.advance(1100);
  expect(t.store.net("dest:1")).toMatchObject({
    buffer: null,
    ping: 42,
    stale: true,
    pingStale: false,
  });
  t.store.ingest({
    ...t.sample(),
    destinations: t.sample().destinations.map((v) => ({
      ...v,
      gatewayPingMs: 42,
      gatewayObservedAtMs: value.clockMs,
    })),
  });
  expect(t.store.net("dest:1")).toMatchObject({ buffer: 10, ping: 42 });
  t.advance(1401);
  expect(t.store.net("dest:1")).toMatchObject({
    buffer: null,
    ping: null,
    pingStale: true,
  });
});

test("inactive descriptors invalidate immediately, keep exact-source history, and require a new observation", () => {
  const t = setup();
  const value = t.send();
  const history = t.store.history("src:strip:1");
  t.store.setInputs(t.fixture.inputs.map((v) => ({ ...v, phase: "stopped" })));
  expect(t.store.read("src:strip:1")).toMatchObject({ db: null, peak: null });
  t.store.setInputs(t.fixture.inputs);
  t.store.ingest(value);
  expect(t.store.read("src:strip:1").db).toBeNull();
  expect(t.store.history("src:strip:1")).toBe(history);
  t.advance(100);
  t.send();
  expect(t.store.read("src:strip:1").db).toBe(-18);
  t.store.setDestinations(
    t.fixture.destinations.map((v) => ({
      ...v,
      connection: { phase: "disconnected" },
    })),
  );
  expect(t.store.read("dst:dest:1")).toMatchObject({ db: null, peak: null });
});

test("omitted measurements in a newer envelope invalidate live values without inventing silence", () => {
  const t = setup();
  t.send();
  t.advance(100);
  t.send({ inputs: [], destinations: [] });
  expect(t.store.read("src:strip:1").db).toBeNull();
  expect(t.store.net("dest:1")).toMatchObject({
    buffer: null,
    ping: null,
    recentDrops: null,
  });
  expect(t.store.history("src:strip:1")?.at(0)).toBeCloseTo(0.7);
});

test("source replacement, destination identity and session changes reset only matching history", () => {
  const t = setup();
  const value = t.send();
  const outputHistory = t.store.history("dst:dest:1");
  t.store.setInputs(
    t.fixture.inputs.map((v) => ({ ...v, sourceId: "replacement" })),
  );
  t.store.ingest(value);
  expect(t.store.history("src:strip:1")).toBeUndefined();
  expect(t.store.read("src:strip:1").db).toBeNull();
  expect(t.store.history("dst:dest:1")).toBe(outputHistory);
  t.store.setDestinations(
    t.fixture.destinations.map((v) => ({ ...v, sessionId: v.sessionId + 1 })),
  );
  t.store.ingest(value);
  expect(t.store.read("dst:dest:1").db).toBeNull();
});

test("silence is observed data; null and nonfinite measurements never create levels", () => {
  const t = setup();
  t.send({
    inputs: t.fixture.telemetry.inputs.map((v) => ({ ...v, levelDbfs: -96 })),
  });
  expect(t.store.read("src:strip:1")).toMatchObject({ db: -96, stale: false });
  expect(t.store.history("src:strip:1")?.at(0)).toBe(0);
  t.advance(100);
  t.send({
    inputs: t.fixture.telemetry.inputs.map((v) => ({
      ...v,
      levelDbfs: Number.NaN,
    })),
  });
  expect(t.store.read("src:strip:1").db).toBeNull();
});

test("recent counters expire by elapsed time and recover after rollback or observation gaps", () => {
  const t = setup();
  let drops = 100;
  const send = () =>
    t.send({
      destinations: t.fixture.telemetry.destinations.map((v) => ({
        ...v,
        droppedFrames: drops,
        underrunFrames: drops * 2,
      })),
    });
  send();
  expect(t.store.net("dest:1").recentDrops).toBeNull();
  t.advance(100);
  drops += 48;
  send();
  expect(t.store.net("dest:1")).toMatchObject({
    recentDrops: 48,
    recentUnderruns: 96,
    drops: 148,
  });
  for (let i = 0; i < 60; i++) {
    t.advance(1000);
    send();
  }
  expect(t.store.net("dest:1").recentDrops).toBe(0);
  t.advance(100);
  drops = 0;
  send();
  expect(t.store.net("dest:1").recentDrops).toBeNull();
  t.advance(100);
  drops = 48;
  send();
  expect(t.store.net("dest:1").recentDrops).toBe(48);
  t.advance(1001);
  send();
  expect(t.store.net("dest:1").recentDrops).toBeNull();
  t.advance(100);
  send();
  expect(t.store.net("dest:1").recentDrops).toBe(0);
});

test("counter retention is bounded and does not underreport an overfull window", () => {
  const t = setup();
  for (let i = 0; i < 1026; i++) {
    t.advance(1);
    t.send({
      destinations: t.fixture.telemetry.destinations.map((v) => ({
        ...v,
        droppedFrames: i,
      })),
    });
  }
  expect(t.store.net("dest:1").recentDrops).toBeNull();
});

test("visibility restoration clears history but repeated origins cannot manufacture new samples", () => {
  const t = setup();
  const value = t.send();
  t.store.setVisible(false);
  t.store.setVisible(true);
  t.store.ingest(value);
  expect(t.store.history("src:strip:1")).toBeUndefined();
  t.advance(100);
  t.send();
  expect(t.store.history("src:strip:1")?.timeAt(0)).toBe(1100);
});

test("recovery retains actual handoff data while null origin marks a real interruption", () => {
  const t = setup();
  t.send();
  t.store.setDestinations(
    t.fixture.destinations.map((v) => ({
      ...v,
      connection: {
        phase: "recovering",
        attempt: 1,
        message: "Checking room permissions.",
      },
    })),
  );
  expect(t.store.read("dst:dest:1").db).toBe(-24);
  t.advance(100);
  t.store.ingest({
    ...t.sample(),
    destinations: t
      .sample()
      .destinations.map((v) => ({ ...v, observedAtMs: null })),
  });
  expect(t.store.read("dst:dest:1")).toMatchObject({ db: null, stale: true });
  t.advance(100);
  t.send();
  expect(t.store.read("dst:dest:1").db).toBe(-24);
});
