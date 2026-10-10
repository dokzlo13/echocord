import { expect, test } from "@playwright/test";
import { appError, ControlAdapter, type Transport } from "../src/adapter";
import { FixtureTransport, fixtureSnapshot } from "../src/fixtures";
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
      telemetry: {
        ...transport.current.telemetry,
        outputLevelDbfs: -10,
        queuedFrames: i,
      },
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
  t.emit({ telemetry: t.current.telemetry });
  expect(a.telemetry.history("src:strip:1")).toBeDefined();
  t.visible(false);
  t.visible(true);
  expect(a.telemetry.history("src:strip:1")).toBeUndefined();
  expect(Number.isNaN(a.telemetry.net().buffer2m.at(0))).toBe(true);
  t.emit({ telemetry: t.current.telemetry });
  expect(a.telemetry.history("src:strip:1")).toBeDefined();
  t.emit({ sessionId: t.current.sessionId + 1 });
  expect(Number.isNaN(a.telemetry.net().buffer2m.at(1))).toBe(true);
  a.dispose();
});
