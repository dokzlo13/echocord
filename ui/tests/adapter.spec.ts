import { expect, test } from "@playwright/test";
import { appError, ControlAdapter, type Transport } from "../src/adapter";
import {
  FixtureTransport,
  fixtureSnapshot,
  freshTelemetry,
} from "../src/fixtures";
import type { ControlCommand, ControlSnapshot } from "../src/generated";

function deferred<T>() {
  let resolve: (v: T) => void = () => {};
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

test("listener-first startup rejects a stale read after a newer subscribed state", async () => {
  const initial = fixtureSnapshot();
  let receive: (s: ControlSnapshot) => void = () => {};
  const read = deferred<ControlSnapshot>();
  const adapter = new ControlAdapter({
    listen: async (f) => {
      receive = f;
      return () => {};
    },
    getSnapshot: () => read.promise,
    dispatch: async () => {},
    configureBot: async () => {},
  });
  const start = adapter.start();
  await wait(0);
  receive({
    ...initial,
    revision: 4,
    bot: { status: "ready", name: "New observation", message: null },
  });
  read.resolve(initial);
  await start;
  expect(adapter.getSnapshot().snapshot?.bot.name).toBe("New observation");
  adapter.dispose();
});

test("disposed asynchronous subscriptions clean up and cannot publish into a later lifecycle", async () => {
  let unlistens = 0;
  const subscription = deferred<() => void>();
  const t: Transport = {
    listen: () => subscription.promise,
    getSnapshot: async () => fixtureSnapshot(),
    dispatch: async () => {},
    configureBot: async () => {},
  };
  const adapter = new ControlAdapter(t);
  const start = adapter.start();
  adapter.dispose();
  subscription.resolve(() => unlistens++);
  await start;
  expect(unlistens).toBe(1);
  expect(adapter.getSnapshot().phase).toBe("loading");
});

test("meter-only snapshot bursts preserve the control subscription identity", async () => {
  const transport = new FixtureTransport("connected");
  const adapter = new ControlAdapter(transport);
  await adapter.start();
  let commits = 0;
  adapter.subscribe(() => commits++);
  const before = adapter.getSnapshot();
  for (let i = 0; i < 500; i++)
    transport.emit({
      telemetry: freshTelemetry({
        ...transport.current.telemetry,
        outputLevelDbfs: -10,
        queuedFrames: i,
      }),
    });
  expect(commits).toBe(0);
  expect(adapter.getSnapshot()).toBe(before);
  expect(adapter.telemetry.read("dst:dest:1").db).toBe(-10);
  adapter.dispose();
});

test("native visibility event outranks an older asynchronous visible read", async () => {
  const visible = deferred<boolean>();
  let callback: (v: boolean) => void = () => {};
  const adapter = new ControlAdapter({
    listen: async () => () => {},
    listenVisibility: async (f) => {
      callback = f;
      return () => {};
    },
    getVisible: () => visible.promise,
    getSnapshot: async () => fixtureSnapshot(),
    dispatch: async () => {},
    configureBot: async () => {},
  });
  const start = adapter.start();
  await wait(0);
  callback(false);
  visible.resolve(true);
  await start;
  expect(adapter.telemetry.visible).toBe(false);
  adapter.dispose();
});

test("faders throttle a sustained drag with one in-flight command and one latest pending value", async () => {
  const first = deferred<void>();
  const calls: ControlCommand[] = [];
  const adapter = new ControlAdapter({
    listen: async () => () => {},
    getSnapshot: async () => fixtureSnapshot(),
    configureBot: async () => {},
    dispatch: async (c) => {
      calls.push(c);
      if (calls.length === 1) await first.promise;
    },
  });
  await adapter.start();
  const gain = (gainDb: number) =>
    adapter.setFader("src:strip:1", {
      type: "set-source-gain",
      stripId: "strip:1",
      gainDb,
    });
  const p1 = gain(-10);
  await wait(100);
  expect(calls).toHaveLength(1);
  const p2 = gain(-20);
  const p3 = gain(-30.5);
  await wait(100);
  expect(calls).toHaveLength(1);
  first.resolve();
  await p1;
  await wait(100);
  await Promise.all([p2, p3]);
  expect(calls).toEqual([
    { type: "set-source-gain", stripId: "strip:1", gainDb: -10 },
    { type: "set-source-gain", stripId: "strip:1", gainDb: -30.5 },
  ]);
  adapter.dispose();
});

test("fader release flushes its fractional value and disposal cancels unsent work", async () => {
  const t = new FixtureTransport();
  const adapter = new ControlAdapter(t);
  await adapter.start();
  const sent = adapter.setFader("src:strip:1", {
    type: "set-source-gain",
    stripId: "strip:1",
    gainDb: -7.5,
  });
  await adapter.flushFader("src:strip:1");
  expect(await sent).toBe(true);
  expect(t.commands[0]).toEqual({
    type: "set-source-gain",
    stripId: "strip:1",
    gainDb: -7.5,
  });
  const cancelled = adapter.setFader("src:strip:1", {
    type: "set-source-gain",
    stripId: "strip:1",
    gainDb: null,
  });
  adapter.dispose();
  expect(await cancelled).toBe(false);
  await wait(100);
  expect(t.commands).toHaveLength(1);
});

test("arbitrary IPC errors and secret-bearing configuration exceptions stay sanitized", async () => {
  expect(
    appError({ code: "unexpected", message: "private path or secret" }).message,
  ).not.toContain("private");
  const t = new FixtureTransport();
  t.rejectSave = true;
  const adapter = new ControlAdapter(t);
  await adapter.start();
  expect(await adapter.configureBot("synthetic-token")).toBe(false);
  expect(adapter.getSnapshot().configurationError?.message).not.toContain(
    "Fixture",
  );
  adapter.dispose();
});

test("a command acknowledgement alone cannot invent capture or connection completion", async () => {
  const initial = fixtureSnapshot();
  const calls: ControlCommand[] = [];
  const adapter = new ControlAdapter({
    listen: async () => () => {},
    getSnapshot: async () => initial,
    configureBot: async () => {},
    dispatch: async (c) => {
      calls.push(c);
    },
  });
  await adapter.start();
  await adapter.dispatch({
    type: "connect-destination",
    destinationId: "dest:1",
  });
  expect(
    adapter.getSnapshot().snapshot?.destinations[0]?.connection.phase,
  ).toBe("disconnected");
  expect(calls).toEqual([
    { type: "connect-destination", destinationId: "dest:1" },
  ]);
  adapter.dispose();
});

test("history starts fresh after native visibility return and destination session change", async () => {
  const t = new FixtureTransport("connected");
  const a = new ControlAdapter(t);
  await a.start();
  t.observeTelemetry();
  expect(a.telemetry.history("src:strip:1")).toBeDefined();
  t.visible(false);
  t.visible(true);
  expect(a.telemetry.history("src:strip:1")).toBeUndefined();
  expect(Number.isNaN(a.telemetry.net().buffer2m.at(0))).toBe(true);
  t.observeTelemetry();
  expect(a.telemetry.history("src:strip:1")).toBeDefined();
  t.emit({ sessionId: t.current.sessionId + 1 });
  expect(Number.isNaN(a.telemetry.net().buffer2m.at(1))).toBe(true);
  a.dispose();
});

test("per-output sessions reject stale samples and retain unaffected histories", async () => {
  const t = new FixtureTransport("outputs");
  const a = new ControlAdapter(t);
  await a.start();
  const scalar = structuredClone(t.current.telemetry);
  a.telemetry.ingest(freshTelemetry(scalar));
  const other = a.telemetry.history("dst:local:3");
  const source = a.telemetry.history("src:strip:1");
  const next = t.current.destinations.map((d) =>
    d.destinationId === "local:1" ? { ...d, sessionId: d.sessionId + 1 } : d,
  );
  a.telemetry.setDestinations(next);
  expect(a.telemetry.history("dst:local:1")).toBeUndefined();
  expect(a.telemetry.history("dst:local:3")).toBe(other);
  expect(a.telemetry.history("src:strip:1")).toBe(source);
  a.telemetry.ingest(
    freshTelemetry({
      ...scalar,
      destinations: scalar.destinations.map((d) =>
        d.destinationId === "local:1" ? { ...d, levelDbfs: -1 } : d,
      ),
    }),
  );
  expect(a.telemetry.read("dst:local:1").db).toBeNull();
  const session =
    next.find((d) => d.destinationId === "local:1")?.sessionId ?? 0;
  a.telemetry.ingest(
    freshTelemetry({
      ...scalar,
      destinations: scalar.destinations.map((d) =>
        d.destinationId === "local:1"
          ? { ...d, sessionId: session, levelDbfs: -32 }
          : d,
      ),
    }),
  );
  expect(a.telemetry.read("dst:local:1").db).toBe(-32);
  a.telemetry.ingest(freshTelemetry(scalar));
  expect(a.telemetry.read("dst:local:1").db).toBe(-32);
  a.dispose();
});

test("unknown scalar IDs and legacy aliases cannot bleed into another output meter", async () => {
  const t = new FixtureTransport("outputs");
  const a = new ControlAdapter(t);
  await a.start();
  const scalar = structuredClone(t.current.telemetry);
  a.telemetry.ingest(
    freshTelemetry({
      ...scalar,
      outputLevelDbfs: 0,
      destinations: scalar.destinations.map((d) => ({
        ...d,
        levelDbfs: d.destinationId === "local:1" ? -10 : -40,
      })),
    }),
  );
  expect(a.telemetry.read("dst:local:1").db).toBe(-10);
  expect(a.telemetry.read("dst:dest:1").db).toBe(-40);
  expect(a.telemetry.read("dst:unknown").db).toBeNull();
  const before = a.telemetry.net("local:1");
  a.telemetry.ingest(
    freshTelemetry({
      ...scalar,
      destinations: [
        {
          destinationId: "unknown",
          observedAtMs: performance.now(),
          gatewayObservedAtMs: performance.now(),
          sessionId: 999,
          levelDbfs: 0,
          queuedFrames: 1920,
          droppedFrames: 100,
          underrunFrames: 100,
          gatewayPingMs: 10,
        },
      ],
    }),
  );
  expect(a.telemetry.net("local:1")).toBe(before);
  expect(a.telemetry.net("unknown").buffer).toBeNull();
  a.dispose();
});

test("unavailable levels remain gaps while measured silence is a real sample", async () => {
  const t = new FixtureTransport("connected");
  const a = new ControlAdapter(t);
  await a.start();
  const scalar = structuredClone(t.current.telemetry);
  a.telemetry.resetHistory();
  a.telemetry.ingest(
    freshTelemetry({
      ...scalar,
      inputs: scalar.inputs.map((s) => ({ ...s, levelDbfs: null })),
      destinations: scalar.destinations.map((d) => ({ ...d, levelDbfs: null })),
    }),
  );
  expect(a.telemetry.history("src:strip:1")).toBeUndefined();
  expect(a.telemetry.history("dst:dest:1")).toBeUndefined();
  a.telemetry.resetHistory();
  a.telemetry.ingest(
    freshTelemetry({
      ...scalar,
      inputs: scalar.inputs.map((s) => ({ ...s, levelDbfs: -96 })),
      destinations: scalar.destinations.map((d) => ({ ...d, levelDbfs: -96 })),
    }),
  );
  expect(a.telemetry.history("src:strip:1")?.at(0)).toBe(0);
  expect(a.telemetry.history("dst:dest:1")?.at(0)).toBe(0);
  a.dispose();
});

test("room telemetry is keyed by destination and session across independent reconnects", async () => {
  const t = new FixtureTransport("rooms");
  const a = new ControlAdapter(t);
  await a.start();
  const scalar = structuredClone(t.current.telemetry);
  scalar.destinations = scalar.destinations.map((d, i) => ({
    ...d,
    levelDbfs: -10 - i * 10,
    queuedFrames: 480 * (i + 1),
    droppedFrames: i,
    gatewayPingMs: d.destinationId === "local:1" ? null : 42,
  }));
  scalar.outputLevelDbfs = 0;
  a.telemetry.ingest(freshTelemetry(scalar));
  expect(a.telemetry.read("dst:dest:1").db).toBe(-10);
  expect(a.telemetry.read("dst:dest:2").db).toBe(-20);
  expect(a.telemetry.read("dst:dest:3").db).toBe(-30);
  expect(a.telemetry.net("dest:2").buffer).toBe(20);
  expect(a.telemetry.net("dest:3").drops).toBe(2);
  expect(a.telemetry.net("local:1").ping).toBeNull();
  const peer = a.telemetry.history("dst:dest:3");
  const source = a.telemetry.history("src:strip:1");
  const next = t.current.destinations.map((d) =>
    d.destinationId === "dest:2" ? { ...d, sessionId: 2 } : d,
  );
  a.telemetry.setDestinations(next);
  expect(a.telemetry.history("dst:dest:2")).toBeUndefined();
  a.telemetry.ingest(freshTelemetry(scalar));
  expect(a.telemetry.read("dst:dest:2").db).toBeNull();
  expect(a.telemetry.history("dst:dest:3")).toBe(peer);
  expect(a.telemetry.history("src:strip:1")).toBe(source);
  a.telemetry.ingest(
    freshTelemetry({
      ...scalar,
      destinations: scalar.destinations.map((d) =>
        d.destinationId === "dest:2"
          ? { ...d, sessionId: 2, levelDbfs: -35 }
          : d,
      ),
    }),
  );
  a.telemetry.ingest(
    freshTelemetry({
      ...scalar,
      destinations: scalar.destinations.map((d) => ({ ...d, levelDbfs: 0 })),
    }),
  );
  expect(a.telemetry.read("dst:dest:2").db).toBe(-35);
  a.telemetry.setDestinations(next.filter((d) => d.destinationId !== "dest:2"));
  a.telemetry.ingest(
    freshTelemetry({
      ...scalar,
      destinations: scalar.destinations.map((d) => ({ ...d, sessionId: 999 })),
    }),
  );
  expect(a.telemetry.read("dst:dest:2").db).toBeNull();
  expect(a.telemetry.history("dst:dest:2")).toBeUndefined();
  a.dispose();
});

test("fixture destination identities remain unique across rapid adds and removals", async () => {
  const t = new FixtureTransport("disconnected");
  await t.dispatch({
    type: "add-destination",
    serverId: "guild:2",
    channelId: "room:3",
  });
  const room = t.current.destinations.at(-1)?.destinationId;
  await t.dispatch({
    type: "add-playback-destination",
    deviceId: "play:headphones",
  });
  const removed = t.current.destinations.at(-1)?.destinationId;
  if (!removed) throw Error("Missing fixture destination");
  await t.dispatch({ type: "remove-destination", destinationId: removed });
  await t.dispatch({
    type: "add-playback-destination",
    deviceId: "play:speakers",
  });
  const replacement = t.current.destinations.at(-1)?.destinationId;
  expect(new Set([room, removed, replacement]).size).toBe(3);
  expect(new Set(t.current.destinations.map((d) => d.destinationId)).size).toBe(
    t.current.destinations.length,
  );
});

test("fixture room joins share a gateway lifetime while joining or recovering peers remain", async () => {
  const t = new FixtureTransport("rooms-restored");
  await t.dispatch({ type: "connect-destination", destinationId: "dest:1" });
  const generation = t.current.gateway.generation;
  t.holdConnect = true;
  await t.dispatch({ type: "connect-destination", destinationId: "dest:2" });
  expect(t.current.gateway).toMatchObject({ phase: "connected", generation });
  expect(
    t.current.destinations.find((d) => d.destinationId === "dest:2")
      ?.connection,
  ).toEqual({ phase: "connecting", stage: "join" });
  await t.dispatch({ type: "disconnect-destination", destinationId: "dest:1" });
  expect(t.current.gateway).toMatchObject({ phase: "connected", generation });
  const ds = structuredClone(t.current.destinations);
  const recovering = ds.find((d) => d.destinationId === "dest:3");
  if (recovering)
    recovering.connection = {
      phase: "recovering",
      attempt: 1,
      message: "Rejoining this room.",
    };
  t.emit({ destinations: ds });
  await t.dispatch({ type: "disconnect-destination", destinationId: "dest:2" });
  expect(t.current.gateway).toMatchObject({ phase: "connected", generation });
  await t.dispatch({ type: "disconnect-destination", destinationId: "dest:3" });
  expect(t.current.gateway.phase).toBe("disconnected");
});

test("card errors survive other card operations and dismiss independently", async () => {
  const t = new FixtureTransport("many");
  const adapter = new ControlAdapter(t);
  await adapter.start();
  t.reject = {
    code: "invalid-selection",
    message: "Select this application again.",
  };
  await adapter.dispatch({ type: "start-capture", stripId: "strip:1" });
  t.reject = null;
  await adapter.dispatch({
    type: "set-source-muted",
    stripId: "strip:2",
    muted: false,
  });
  expect(
    adapter.getSnapshot().targetErrors["src:strip:1"]?.error.message,
  ).toContain("Select this application");
  t.reject = { code: "busy", message: "This output is still stopping." };
  await adapter.dispatch({
    type: "connect-destination",
    destinationId: "dest:1",
  });
  expect(Object.keys(adapter.getSnapshot().targetErrors)).toHaveLength(2);
  adapter.dismissError("src:strip:1");
  expect(adapter.getSnapshot().targetErrors["src:strip:1"]).toBeUndefined();
  expect(
    adapter.getSnapshot().targetErrors["dst:dest:1"]?.error.message,
  ).toContain("still stopping");
  t.reject = null;
  await adapter.dispatch({
    type: "remove-destination",
    destinationId: "dest:1",
  });
  expect(adapter.getSnapshot().targetErrors).toEqual({});
  adapter.dispose();
});

test("token failures retain useful codes without propagating private exception text", async () => {
  for (const [code, text] of [
    ["authentication", "Discord rejected"],
    ["busy", "Disconnect Discord outputs"],
    ["connection-failed", "Could not reach Discord"],
    ["internal", "Could not confirm"],
  ] as const) {
    const t = new FixtureTransport();
    t.configureBot = async () => {
      throw { code, message: "private synthetic token must not be displayed" };
    };
    const adapter = new ControlAdapter(t);
    await adapter.start();
    expect(await adapter.configureBot("synthetic-token")).toBe(false);
    expect(adapter.getSnapshot().configurationError?.message).toContain(text);
    expect(adapter.getSnapshot().configurationError?.message).not.toContain(
      "private",
    );
    adapter.dispose();
  }
});

test("source replacement clears only that identity's history and peak while recapture preserves peers", async () => {
  const t = new FixtureTransport("many");
  t.current.inputs = t.current.inputs.map((v) => ({
    ...v,
    phase: "capturing",
  }));
  t.current.telemetry.inputs = t.current.telemetry.inputs.map((v) => ({
    ...v,
    levelDbfs: -18,
  }));
  const adapter = new ControlAdapter(t);
  await adapter.start();
  adapter.telemetry.ingest(freshTelemetry(t.current.telemetry));
  const peer = adapter.telemetry.history("src:strip:2");
  const original = adapter.telemetry.history("src:strip:1");
  expect(original).toBeDefined();
  adapter.telemetry.read("src:strip:1");
  t.emit({
    inputs: t.current.inputs.map((input) =>
      input.stripId === "strip:1"
        ? {
            ...input,
            sourceId: "recording:new-owned-fixture",
            phase: "opening",
          }
        : input,
    ),
  });
  expect(adapter.telemetry.history("src:strip:1")).toBeUndefined();
  expect(adapter.telemetry.read("src:strip:1").peak).toBeNull();
  expect(adapter.telemetry.history("src:strip:2")).toBe(peer);
  await wait(110);
  t.emit({
    inputs: t.current.inputs.map((v) => ({ ...v, phase: "capturing" })),
  });
  adapter.telemetry.ingest(freshTelemetry(t.current.telemetry));
  const replaced = adapter.telemetry.history("src:strip:1");
  expect(replaced).toBeDefined();
  expect(replaced).not.toBe(original);
  t.emit({
    inputs: t.current.inputs.map((input) => ({ ...input, phase: "capturing" })),
  });
  expect(adapter.telemetry.history("src:strip:1")).toBe(replaced);
  adapter.dispose();
});

test("rejected unknown targets do not grow the card error inventory", async () => {
  const t = new FixtureTransport();
  const adapter = new ControlAdapter(t);
  await adapter.start();
  t.reject = {
    code: "invalid-selection",
    message: "This source no longer exists.",
  };
  for (let index = 0; index < 20; index++)
    await adapter.dispatch({
      type: "start-capture",
      stripId: `missing:${index}`,
    });
  expect(adapter.getSnapshot().targetErrors).toEqual({});
  expect(adapter.getSnapshot().commandError?.message).toContain(
    "no longer exists",
  );
  adapter.dispose();
});
