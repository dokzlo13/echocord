import { expect, test } from "@playwright/test";
import { appError, ControlAdapter, type Transport } from "../src/adapter";
import { fixtureSnapshot } from "../src/fixtures";
import type {
  ControlCommand,
  ControlSnapshot,
  Selection,
} from "../src/generated";

function deferred<T>() {
  let resolve: (value: T) => void = () => {
    throw new Error("Promise not initialized");
  };
  let reject: (reason: unknown) => void = () => {
    throw new Error("Promise not initialized");
  };
  const promise = new Promise<T>((accept, deny) => {
    resolve = accept;
    reject = deny;
  });
  return { promise, resolve, reject };
}

class Host implements Transport {
  state = fixtureSnapshot();
  receive: (snapshot: ControlSnapshot) => void = () => {};
  calls: string[] = [];
  commands: ControlCommand[] = [];
  stops = 0;
  configureBot = async (_token: string) => {};
  listen = async (receive: (snapshot: ControlSnapshot) => void) => {
    this.calls.push("listen");
    this.receive = receive;
    return () => {
      this.stops++;
    };
  };
  getSnapshot = async () => {
    this.calls.push("get");
    return this.state;
  };
  dispatch = async (command: ControlCommand) => {
    this.commands.push(command);
  };
  emit(patch: Partial<ControlSnapshot>) {
    this.state = { ...this.state, ...patch, revision: this.state.revision + 1 };
    this.receive(this.state);
  }
}

class VisibleHost extends Host {
  visible = true;
  visibilityCalls: string[] = [];
  visibilityStops = 0;
  receiveVisibility: (visible: boolean) => void = () => {};
  listenVisibility = async (receive: (visible: boolean) => void) => {
    this.visibilityCalls.push("listen");
    this.receiveVisibility = receive;
    return () => {
      this.visibilityStops++;
    };
  };
  getVisible = async () => {
    this.visibilityCalls.push("get");
    return this.visible;
  };
}

test("telemetry bursts retain control identity and reject revisions newer than the stable view but older than the latest observation", async () => {
  const host = new Host();
  host.state = fixtureSnapshot("connected");
  const adapter = new ControlAdapter(host);
  expect(adapter.getTelemetrySnapshot()).toEqual({
    sessionId: 0,
    sourceId: null,
    capture: null,
    visible: true,
  });
  await adapter.start();
  const control = adapter.getSnapshot();
  let controls = 0;
  let observations = 0;
  const unsubscribe = adapter.subscribe(() => controls++);
  const unsubscribeTelemetry = adapter.subscribeTelemetry(() => observations++);
  for (let index = 1; index <= 1_000; index++) {
    const snapshot = structuredClone(host.state);
    host.emit({
      ...snapshot,
      capture: {
        ...snapshot.capture,
        levelDbfs: -(index % 60),
        outputLevelDbfs: -(index % 60) - 3,
        droppedFrames: index,
        underrunFrames: index * 2,
        queuedFrames: index % 4,
      },
    });
  }
  expect(controls).toBe(0);
  expect(observations).toBe(1_000);
  expect(adapter.getSnapshot()).toBe(control);
  expect(adapter.getSnapshot().snapshot).toBe(control.snapshot);
  const latest = adapter.getTelemetrySnapshot();
  expect(latest.capture).toBe(host.state.capture);
  expect(adapter.getTelemetrySnapshot()).toBe(latest);
  host.receive({
    ...fixtureSnapshot("capture-error"),
    revision: host.state.revision - 1,
  });
  host.receive({
    ...fixtureSnapshot("missing-device"),
    revision: host.state.revision,
  });
  // A fresh revision with identical scalar values is also silent.
  host.emit(structuredClone(host.state));
  expect(adapter.getSnapshot()).toBe(control);
  expect(adapter.getTelemetrySnapshot()).toBe(latest);
  expect(controls).toBe(0);
  expect(observations).toBe(1_000);
  host.emit({ capture: { ...host.state.capture, outputLevelDbfs: null } });
  expect(adapter.getSnapshot()).toBe(control);
  expect(adapter.getTelemetrySnapshot().capture?.outputLevelDbfs).toBeNull();
  expect(observations).toBe(1_001);
  unsubscribe();
  unsubscribeTelemetry();
  host.emit({ output: { volumePercent: 55, muted: true } });
  host.emit({ capture: { ...host.state.capture, queuedFrames: 9 } });
  expect(controls).toBe(0);
  expect(observations).toBe(1_001);
  adapter.dispose();
});

test("control changes include capture errors, connection details, configuration, selection and nested catalog metadata", async () => {
  const host = new Host();
  const adapter = new ControlAdapter(host);
  await adapter.start();
  let notifications = 0;
  adapter.subscribe(() => notifications++);
  const patches: Partial<ControlSnapshot>[] = [
    {
      capture: {
        ...host.state.capture,
        phase: "error",
        message: "Device lost",
      },
    },
    {
      capture: {
        ...host.state.capture,
        phase: "error",
        message: "Retry failed",
      },
    },
    { capture: { ...host.state.capture, phase: "opening" } },
    { connection: { phase: "connecting", stage: "gateway" } },
    { connection: { phase: "connecting", stage: "join" } },
    { connection: { phase: "recovering", attempt: 1, message: "Rejoining" } },
    { connection: { phase: "recovering", attempt: 2, message: "Rejoining" } },
    { connection: { phase: "recovering", attempt: 2, message: "Waiting" } },
    {
      connection: {
        phase: "error",
        code: "permission-denied",
        message: "Denied",
      },
    },
    {
      connection: {
        phase: "error",
        code: "connection-failed",
        message: "Denied",
      },
    },
    { bot: { status: "checking", name: null, message: null } },
    { bot: { status: "ready", name: "Updated Bot", message: null } },
    { discovery: { phase: "error", message: "Catalog unavailable" } },
    { discovery: { phase: "error", message: "Catalog retry failed" } },
    { selection: { ...host.state.selection, channelId: null } },
    { selection: { ...host.state.selection, sourceId: null } },
    { sessionId: 2 },
    { output: { volumePercent: 20, muted: false } },
    { output: { volumePercent: 20, muted: true } },
    {
      sources: host.state.sources.map((source) => ({
        ...source,
        available: false,
        detail: "Unavailable",
      })),
    },
    {
      servers: host.state.servers.map((server) => ({
        ...server,
        iconUrl: "https://example.invalid/icon.png",
        channels: server.channels.map((channel) => ({
          ...channel,
          name: "Renamed room",
          categoryName: "Updated category",
          position: channel.position + 1,
        })),
      })),
    },
  ];
  for (const [index, patch] of patches.entries()) {
    const before = adapter.getSnapshot();
    host.emit(patch);
    expect(adapter.getSnapshot()).not.toBe(before);
    expect(adapter.getSnapshot().snapshot).toBe(host.state);
    expect(notifications).toBe(index + 1);
  }
  adapter.dispose();
});

test("hidden observations stay fresh without notifications; return publishes only the latest and control commands remain responsive", async () => {
  const host = new VisibleHost();
  host.state = fixtureSnapshot("connected");
  const adapter = new ControlAdapter(host);
  await adapter.start();
  expect(host.visibilityCalls).toEqual(["listen", "get"]);
  const seen: ReturnType<typeof adapter.getTelemetrySnapshot>[] = [];
  adapter.subscribeTelemetry(() => seen.push(adapter.getTelemetrySnapshot()));
  let controls = 0;
  adapter.subscribe(() => controls++);
  const initial = adapter.getSnapshot();
  host.receiveVisibility(false);
  expect(seen).toHaveLength(1);
  expect(seen[0]?.visible).toBe(false);
  for (let index = 1; index <= 200; index++)
    host.emit({
      capture: {
        ...host.state.capture,
        levelDbfs: -index,
        queuedFrames: index,
      },
    });
  expect(adapter.getSnapshot()).toBe(initial);
  expect(controls).toBe(0);
  expect(seen).toHaveLength(1);
  host.emit({
    sessionId: 2,
    selection: { ...host.state.selection, sourceId: "cable-b" },
    capture: { ...host.state.capture, phase: "error", message: "Device lost" },
  });
  expect(controls).toBe(1);
  expect(adapter.getSnapshot().snapshot?.capture.message).toBe("Device lost");
  expect(adapter.getTelemetrySnapshot()).toEqual({
    sessionId: 2,
    sourceId: "cable-b",
    capture: host.state.capture,
    visible: false,
  });
  expect(await adapter.dispatch({ type: "set-muted", muted: true })).toBe(true);
  expect(await adapter.stop()).toBe(true);
  expect(host.commands).toEqual([
    { type: "set-muted", muted: true },
    { type: "disconnect" },
  ]);
  expect(seen).toHaveLength(1);
  host.receiveVisibility(true);
  expect(seen).toHaveLength(2);
  expect(seen[1]).toBe(adapter.getTelemetrySnapshot());
  expect(seen[1]?.capture).toBe(host.state.capture);
  expect(seen[1]?.visible).toBe(true);
  const resumed = adapter.getTelemetrySnapshot();
  host.receiveVisibility(true);
  expect(adapter.getTelemetrySnapshot()).toBe(resumed);
  expect(seen).toHaveLength(2);
  adapter.dispose();
  expect(host.stops).toBe(1);
  expect(host.visibilityStops).toBe(1);
  expect(adapter.getTelemetrySnapshot().visible).toBe(false);
});

for (const visible of [false, true]) {
  test(`visibility event ${visible} wins a racing initial read without blocking control startup`, async () => {
    const host = new VisibleHost();
    const read = deferred<boolean>();
    host.getVisible = () => {
      host.visibilityCalls.push("get");
      return read.promise;
    };
    const adapter = new ControlAdapter(host);
    const start = adapter.start();
    await expect.poll(() => host.visibilityCalls).toEqual(["listen", "get"]);
    await expect.poll(() => adapter.getSnapshot().phase).toBe("ready");
    expect(adapter.getTelemetrySnapshot().visible).toBe(false);
    host.receiveVisibility(visible);
    read.resolve(!visible);
    await start;
    expect(adapter.getTelemetrySnapshot().visible).toBe(visible);
    adapter.dispose();
  });
}

test("visibility events during listener setup outrank an initial read", async () => {
  const host = new VisibleHost();
  host.listenVisibility = async (receive) => {
    host.visibilityCalls.push("listen");
    host.receiveVisibility = receive;
    receive(false);
    return () => host.visibilityStops++;
  };
  const adapter = new ControlAdapter(host);
  await adapter.start();
  expect(host.visibilityCalls).toEqual(["listen", "get"]);
  expect(adapter.getTelemetrySnapshot().visible).toBe(false);
  adapter.dispose();
});

test("visibility read failure preserves a hide event and does not disable controls", async () => {
  const host = new VisibleHost();
  const read = deferred<boolean>();
  host.getVisible = () => read.promise;
  const adapter = new ControlAdapter(host);
  const start = adapter.start();
  await expect.poll(() => adapter.getSnapshot().phase).toBe("ready");
  host.receiveVisibility(false);
  read.reject(new Error("Visibility unavailable"));
  await start;
  expect(adapter.getSnapshot().phase).toBe("ready");
  expect(adapter.getTelemetrySnapshot().visible).toBe(false);
  expect(await adapter.dispatch({ type: "connect" })).toBe(true);
  host.receiveVisibility(true);
  expect(adapter.getTelemetrySnapshot().visible).toBe(true);
  adapter.dispose();
});

test("dispose during visibility setup cleans its late listener and never reads visibility", async () => {
  const host = new VisibleHost();
  const listener = deferred<() => void>();
  host.listenVisibility = async (receive) => {
    host.receiveVisibility = receive;
    return listener.promise;
  };
  const adapter = new ControlAdapter(host);
  const start = adapter.start();
  await expect.poll(() => adapter.getSnapshot().phase).toBe("ready");
  adapter.dispose();
  const control = adapter.getSnapshot();
  const telemetry = adapter.getTelemetrySnapshot();
  listener.resolve(() => host.visibilityStops++);
  await start;
  host.receiveVisibility(true);
  host.emit({ connection: { phase: "connected" } });
  expect(adapter.getSnapshot()).toBe(control);
  expect(adapter.getTelemetrySnapshot()).toBe(telemetry);
  expect(host.visibilityCalls).toEqual([]);
  expect(host.stops).toBe(1);
  expect(host.visibilityStops).toBe(1);
});

test("restart ignores prior visibility events and reads, then recovers at the same snapshot revision", async () => {
  const host = new VisibleHost();
  const read = deferred<boolean>();
  host.getVisible = () => read.promise;
  const adapter = new ControlAdapter(host);
  const first = adapter.start();
  await expect.poll(() => adapter.getSnapshot().phase).toBe("ready");
  const staleVisibility = host.receiveVisibility;
  const staleSnapshot = host.receive;
  host.getVisible = async () => false;
  await adapter.start();
  expect(adapter.getSnapshot().phase).toBe("ready");
  const control = adapter.getSnapshot();
  const telemetry = adapter.getTelemetrySnapshot();
  staleVisibility(true);
  staleSnapshot({ ...fixtureSnapshot("connected"), revision: 500 });
  read.resolve(true);
  await first;
  expect(adapter.getSnapshot()).toBe(control);
  expect(adapter.getTelemetrySnapshot()).toBe(telemetry);
  expect(telemetry.visible).toBe(false);
  expect(host.stops).toBe(1);
  expect(host.visibilityStops).toBe(1);
  adapter.dispose();
  expect(host.stops).toBe(2);
  expect(host.visibilityStops).toBe(2);
});

test("restart visibility cannot present an old peak before a fresh equal-revision read", async () => {
  const host = new VisibleHost();
  host.state = fixtureSnapshot("connected");
  host.state.capture = {
    ...host.state.capture,
    levelDbfs: -6,
    outputLevelDbfs: -12,
  };
  const adapter = new ControlAdapter(host);
  await adapter.start();
  expect(adapter.getTelemetrySnapshot().capture?.outputLevelDbfs).toBe(-12);
  const read = deferred<ControlSnapshot>();
  host.getSnapshot = () => read.promise;
  const presented: ReturnType<typeof adapter.getTelemetrySnapshot>[] = [];
  adapter.subscribeTelemetry(() => {
    const telemetry = adapter.getTelemetrySnapshot();
    if (telemetry.visible) presented.push(telemetry);
  });
  const restart = adapter.start();
  await expect.poll(() => adapter.getTelemetrySnapshot().visible).toBe(true);
  expect(adapter.getSnapshot().phase).toBe("loading");
  expect(adapter.getTelemetrySnapshot()).toEqual({
    sessionId: 0,
    sourceId: null,
    capture: null,
    visible: true,
  });
  expect(presented.every((telemetry) => telemetry.capture === null)).toBe(true);
  const fresh = {
    ...host.state,
    capture: { ...host.state.capture, levelDbfs: -30, outputLevelDbfs: -36 },
  };
  read.resolve(fresh);
  await restart;
  expect(adapter.getSnapshot().phase).toBe("ready");
  expect(adapter.getSnapshot().snapshot).toBe(fresh);
  expect(adapter.getTelemetrySnapshot().capture).toBe(fresh.capture);
  expect(presented.at(-1)?.capture?.outputLevelDbfs).toBe(-36);
  adapter.dispose();
});

test("failed restart read leaves telemetry empty after visibility has resumed", async () => {
  const host = new VisibleHost();
  host.state = fixtureSnapshot("connected");
  host.state.capture = { ...host.state.capture, outputLevelDbfs: -12 };
  const adapter = new ControlAdapter(host);
  await adapter.start();
  expect(adapter.getTelemetrySnapshot().capture?.outputLevelDbfs).toBe(-12);
  const read = deferred<ControlSnapshot>();
  host.getSnapshot = () => read.promise;
  const presented: ReturnType<typeof adapter.getTelemetrySnapshot>[] = [];
  adapter.subscribeTelemetry(() => {
    const telemetry = adapter.getTelemetrySnapshot();
    if (telemetry.visible) presented.push(telemetry);
  });
  const restart = adapter.start();
  await expect.poll(() => adapter.getTelemetrySnapshot().visible).toBe(true);
  expect(adapter.getTelemetrySnapshot().capture).toBeNull();
  read.reject(new Error("Fresh status unavailable"));
  await restart;
  expect(adapter.getSnapshot().phase).toBe("error");
  expect(adapter.getTelemetrySnapshot().visible).toBe(true);
  expect(adapter.getTelemetrySnapshot().capture).toBeNull();
  expect(presented.every((telemetry) => telemetry.capture === null)).toBe(true);
  expect(await adapter.dispatch({ type: "connect" })).toBe(false);
  adapter.dispose();
});

test("native SDK uses boolean visibility IPC, refreshes after a hide, and unregisters both listeners", async ({
  page,
}) => {
  await page.goto("/");
  const result = await page.evaluate(async (initial: ControlSnapshot) => {
    const modulePath = "/src/nativeAdapter.ts";
    const { NativeAdapter } = (await import(modulePath)) as {
      NativeAdapter: new () => ControlAdapter;
    };
    const callbacks = new Map<
      number,
      (event: { event: string; id: number; payload: unknown }) => void
    >();
    const registered = new Map<number, { event: string; handler: number }>();
    const calls: { name: string; args: Record<string, unknown> }[] = [];
    let nextId = 0;
    let current = initial;
    const host = window as unknown as {
      isTauri: boolean;
      __TAURI_INTERNALS__: unknown;
      __TAURI_EVENT_PLUGIN_INTERNALS__: unknown;
    };
    const emit = (event: string, payload: unknown) => {
      for (const [id, listener] of registered)
        if (listener.event === event)
          callbacks.get(listener.handler)?.({ event, id, payload });
    };
    host.isTauri = true;
    host.__TAURI_INTERNALS__ = {
      transformCallback: (
        callback: (event: {
          event: string;
          id: number;
          payload: unknown;
        }) => void,
      ) => {
        const id = ++nextId;
        callbacks.set(id, callback);
        return id;
      },
      invoke: async (name: string, args: Record<string, unknown>) => {
        calls.push({ name, args });
        if (name === "plugin:event|listen") {
          const id = ++nextId;
          registered.set(id, {
            event: args.event as string,
            handler: args.handler as number,
          });
          return id;
        }
        if (name === "plugin:event|unlisten" || name === "dispatch") return;
        if (name === "get_visibility") {
          if (
            ![...registered.values()].some(
              (listener) => listener.event === "echocord://visibility",
            )
          )
            throw new Error("Visibility read before listener");
          return false;
        }
        if (name === "get_snapshot") return current;
        throw new Error("Unexpected IPC command");
      },
    };
    host.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: (_event: string, id: number) => registered.delete(id),
    };
    const adapter = new NativeAdapter();
    await adapter.start();
    const active = registered.size;
    const control = adapter.getSnapshot();
    let observations = 0;
    adapter.subscribeTelemetry(() => observations++);
    current = {
      ...current,
      revision: current.revision + 1,
      capture: { ...current.capture, levelDbfs: -9, outputLevelDbfs: -12 },
    };
    emit("echocord://snapshot", current);
    const hidden = adapter.getTelemetrySnapshot();
    const hiddenNotifications = observations;
    emit("echocord://visibility", true);
    const resumed = adapter.getTelemetrySnapshot();
    const resumedNotifications = observations;
    const stable = adapter.getSnapshot() === control;
    const accepted = await adapter.dispatch({ type: "set-muted", muted: true });
    adapter.dispose();
    return {
      calls,
      active,
      remaining: registered.size,
      hidden,
      resumed,
      hiddenNotifications,
      resumedNotifications,
      stable,
      accepted,
    };
  }, fixtureSnapshot("connected"));
  expect(result.active).toBe(2);
  expect(result.remaining).toBe(0);
  expect(result.hidden.visible).toBe(false);
  expect(result.hidden.capture?.outputLevelDbfs).toBe(-12);
  expect(result.hiddenNotifications).toBe(0);
  expect(result.resumed.visible).toBe(true);
  expect(result.resumed.capture).toEqual(result.hidden.capture);
  expect(result.resumedNotifications).toBe(1);
  expect(result.stable).toBe(true);
  expect(result.accepted).toBe(true);
  expect(
    result.calls.find((call) => call.name === "get_visibility")?.args,
  ).toEqual({});
  expect(
    result.calls
      .filter((call) => call.name === "plugin:event|listen")
      .map((call) => call.args.event),
  ).toEqual(["echocord://visibility", "echocord://snapshot"]);
  expect(
    result.calls
      .filter((call) => call.name === "plugin:event|unlisten")
      .map((call) => call.args.event)
      .sort(),
  ).toEqual(["echocord://snapshot", "echocord://visibility"]);
  expect(result.calls.find((call) => call.name === "dispatch")?.args).toEqual({
    command: { type: "set-muted", muted: true },
  });
});

test("listen precedes initial fetch; racing events, old sessions, and equal revisions cannot regress state", async () => {
  const host = new Host();
  const initial = deferred<ControlSnapshot>();
  host.getSnapshot = () => {
    host.calls.push("get");
    return initial.promise;
  };
  const adapter = new ControlAdapter(host);
  const start = adapter.start();
  await expect.poll(() => host.calls).toEqual(["listen", "get"]);
  host.emit({ revision: 20, sessionId: 2, connection: { phase: "connected" } });
  const latest = adapter.getSnapshot().snapshot;
  initial.resolve({ ...fixtureSnapshot(), revision: 1, sessionId: 1 });
  await start;
  host.receive({ ...fixtureSnapshot(), revision: 1, sessionId: 1 });
  host.receive({ ...fixtureSnapshot(), revision: latest?.revision ?? 0 });
  expect(adapter.getSnapshot().snapshot).toBe(latest);
  expect(adapter.getSnapshot().snapshot?.connection.phase).toBe("connected");
  adapter.dispose();
  expect(host.stops).toBe(1);
});

test("dispose during listener setup immediately unlistens its late result and never fetches", async () => {
  const host = new Host();
  const listener = deferred<() => void>();
  host.listen = async (receive) => {
    host.receive = receive;
    return listener.promise;
  };
  const adapter = new ControlAdapter(host);
  const start = adapter.start();
  adapter.dispose();
  const before = adapter.getSnapshot();
  listener.resolve(() => {
    host.stops++;
  });
  await start;
  host.emit({ connection: { phase: "connected" } });
  expect(host.stops).toBe(1);
  expect(host.calls).toEqual([]);
  expect(adapter.getSnapshot()).toBe(before);
});

test("dispose/restart drops late initial reads and stale command errors from the previous lifecycle", async () => {
  const host = new Host();
  const adapter = new ControlAdapter(host);
  await adapter.start();
  const rejected = deferred<void>();
  host.dispatch = () => rejected.promise;
  const send = adapter.dispatch({ type: "connect" });
  adapter.dispose();
  await adapter.start();
  rejected.reject({ code: "permission-denied", message: "Old command failed" });
  await send;
  expect(adapter.getSnapshot().commandError).toBeNull();
  expect(adapter.getSnapshot().pending).toEqual([]);
  const read = deferred<ControlSnapshot>();
  host.getSnapshot = () => read.promise;
  const restart = adapter.start();
  await Promise.resolve();
  adapter.dispose();
  const before = adapter.getSnapshot();
  read.resolve({ ...fixtureSnapshot("connected"), revision: 100 });
  await restart;
  expect(adapter.getSnapshot()).toBe(before);
});

test("void command acceptance is not a connection; rejected commands preserve authoritative output", async () => {
  const host = new Host();
  const adapter = new ControlAdapter(host);
  await adapter.start();
  expect(await adapter.dispatch({ type: "connect" })).toBe(true);
  expect(adapter.getSnapshot().snapshot?.connection.phase).toBe("disconnected");
  host.dispatch = async () => {
    throw {
      code: "permission-denied",
      message: "Connect permission is missing.",
    };
  };
  expect(await adapter.dispatch({ type: "set-muted", muted: true })).toBe(
    false,
  );
  expect(adapter.getSnapshot().snapshot?.output.muted).toBe(false);
  expect(adapter.getSnapshot().commandError).toEqual({
    code: "permission-denied",
    message: "Connect permission is missing.",
  });
  expect(adapter.getSnapshot().pending).toEqual([]);
  expect(appError("untrusted exception details").message).not.toContain(
    "untrusted",
  );
  adapter.dispose();
});

test("volume is coalesced before IPC and while IPC is pending; mute bypasses pending volume", async () => {
  const host = new Host();
  const adapter = new ControlAdapter(host);
  await adapter.start();
  const first = deferred<void>();
  host.dispatch = async (command) => {
    host.commands.push(command);
    if (command.type === "set-volume" && command.percent === 40)
      await first.promise;
  };
  for (let percent = 1; percent <= 40; percent++) adapter.setVolume(percent);
  const flush = adapter.flushVolume();
  expect(host.commands).toEqual([{ type: "set-volume", percent: 40 }]);
  adapter.setVolume(90);
  adapter.setVolume(100);
  const slider = adapter.getSnapshot();
  for (let index = 1; index <= 50; index++)
    host.emit({
      capture: {
        ...host.state.capture,
        levelDbfs: -index,
        queuedFrames: index,
      },
    });
  expect(adapter.getSnapshot()).toBe(slider);
  expect(adapter.getSnapshot().volumeDraft).toBe(100);
  await adapter.flushVolume();
  await adapter.dispatch({ type: "set-muted", muted: true });
  expect(host.commands).toHaveLength(2);
  host.emit({ output: { volumePercent: 40, muted: true } });
  expect(adapter.getSnapshot().volumeDraft).toBe(100);
  first.resolve();
  await flush;
  expect(host.commands).toEqual([
    { type: "set-volume", percent: 40 },
    { type: "set-muted", muted: true },
    { type: "set-volume", percent: 100 },
  ]);
  host.emit({ output: { volumePercent: 100, muted: true } });
  expect(adapter.getSnapshot().volumeDraft).toBeNull();
  adapter.dispose();
});

test("disconnect/disposal cancel unsent volume; rejected volume restores backend value", async () => {
  const host = new Host();
  const adapter = new ControlAdapter(host);
  await adapter.start();
  adapter.setVolume(10);
  await adapter.dispatch({ type: "disconnect" });
  await adapter.flushVolume();
  expect(host.commands).toEqual([{ type: "disconnect" }]);
  host.dispatch = async () => {
    throw { code: "invalid-volume", message: "Volume rejected." };
  };
  adapter.setVolume(45);
  await adapter.flushVolume();
  expect(adapter.getSnapshot().volumeDraft).toBeNull();
  expect(adapter.getSnapshot().snapshot?.output.volumePercent).toBe(80);
  adapter.setVolume(70);
  adapter.dispose();
  await adapter.flushVolume();
  expect(host.commands).toHaveLength(1);
});

test("initial transport errors surface, retry recovers, and subscriptions keep cached state", async () => {
  const host = new Host();
  host.getSnapshot = async () => {
    throw new Error("transport down");
  };
  const adapter = new ControlAdapter(host);
  let notifications = 0;
  const unsubscribe = adapter.subscribe(() => {
    notifications++;
  });
  await adapter.start();
  expect(adapter.getSnapshot().phase).toBe("error");
  host.getSnapshot = async () => fixtureSnapshot();
  await adapter.start();
  expect(adapter.getSnapshot().phase).toBe("ready");
  expect(host.stops).toBe(1);
  const before = adapter.getSnapshot();
  expect(adapter.getSnapshot()).toBe(before);
  const count = notifications;
  unsubscribe();
  host.emit({ output: { volumePercent: 20, muted: false } });
  expect(notifications).toBe(count);
  adapter.dispose();
});

const nextRoute: Selection = {
  sourceId: "cable-b",
  serverId: "1234567890123456790",
  channelId: "9876543210987654323",
};

test("apply waits for confirmed disconnect and selections; mute works throughout and connect acceptance does not fake a session", async () => {
  const host = new Host();
  host.state = fixtureSnapshot("connected");
  host.dispatch = async (command) => {
    host.commands.push(command);
    if (command.type === "disconnect")
      host.emit({ connection: { phase: "disconnecting" } });
    if (command.type === "set-muted")
      host.emit({ output: { ...host.state.output, muted: command.muted } });
    if (command.type === "select-source")
      host.emit({
        selection: { ...host.state.selection, sourceId: command.sourceId },
      });
    if (command.type === "select-destination")
      host.emit({
        selection: {
          ...host.state.selection,
          serverId: command.serverId,
          channelId: command.channelId,
        },
      });
  };
  const adapter = new ControlAdapter(host);
  await adapter.start();
  const apply = adapter.applyRoute(nextRoute, true);
  await expect.poll(() => host.commands).toEqual([{ type: "disconnect" }]);
  expect(adapter.getSnapshot().routeOperation).toBe("stopping");
  await adapter.dispatch({ type: "set-muted", muted: true });
  expect(host.commands).toHaveLength(2);
  expect(host.state.selection).not.toEqual(nextRoute);
  for (let index = 1; index <= 50; index++)
    host.emit({
      capture: {
        ...host.state.capture,
        levelDbfs: -index,
        queuedFrames: index,
      },
    });
  host.receive({
    ...host.state,
    revision: host.state.revision - 1,
    connection: { phase: "disconnected" },
  });
  expect(host.commands).toHaveLength(2);
  expect(adapter.getSnapshot().routeOperation).toBe("stopping");
  host.emit({ connection: { phase: "disconnected" } });
  expect(await apply).toBe(true);
  expect(host.commands.map((command) => command.type)).toEqual([
    "disconnect",
    "set-muted",
    "select-source",
    "select-destination",
    "connect",
  ]);
  expect(adapter.getSnapshot().snapshot?.selection).toEqual(nextRoute);
  expect(adapter.getSnapshot().snapshot?.output.muted).toBe(true);
  expect(adapter.getSnapshot().snapshot?.connection.phase).toBe("disconnected");
  expect(adapter.getSnapshot().routeOperation).toBeNull();
  adapter.dispose();
});

test("cleanup failure and a rejected selection never apply another route or reconnect", async () => {
  const host = new Host();
  host.state = fixtureSnapshot("muted");
  host.dispatch = async (command) => {
    host.commands.push(command);
    host.emit({
      connection: {
        phase: "cleanup-incomplete",
        message: "The session has not exited.",
      },
    });
  };
  const adapter = new ControlAdapter(host);
  await adapter.start();
  expect(await adapter.applyRoute(nextRoute, true)).toBe(false);
  expect(host.commands).toEqual([{ type: "disconnect" }]);
  expect(adapter.getSnapshot().commandError?.code).toBe("cleanup-incomplete");
  expect(adapter.getSnapshot().snapshot?.output.muted).toBe(true);
  adapter.dispose();

  const rejected = new Host();
  rejected.dispatch = async (command) => {
    rejected.commands.push(command);
    throw {
      code: "invalid-selection",
      message: "The recording device is unavailable.",
    };
  };
  const second = new ControlAdapter(rejected);
  await second.start();
  expect(await second.applyRoute(nextRoute, true)).toBe(false);
  expect(rejected.commands).toEqual([
    { type: "select-source", sourceId: "cable-b" },
  ]);
  expect(second.getSnapshot().snapshot?.selection).toEqual(
    fixtureSnapshot().selection,
  );
  expect(second.getSnapshot().commandError?.code).toBe("invalid-selection");
  second.dispose();
});

test("explicit disconnect cancels apply while awaiting selection; no late reconnect follows", async () => {
  const host = new Host();
  const adapter = new ControlAdapter(host);
  await adapter.start();
  const apply = adapter.applyRoute(nextRoute, true);
  await expect
    .poll(() => host.commands)
    .toEqual([{ type: "select-source", sourceId: "cable-b" }]);
  await adapter.dispatch({ type: "disconnect" });
  expect(await apply).toBe(false);
  host.emit({ selection: { ...host.state.selection, sourceId: "cable-b" } });
  expect(host.commands).toEqual([
    { type: "select-source", sourceId: "cable-b" },
    { type: "disconnect" },
  ]);
  expect(adapter.getSnapshot().routeOperation).toBeNull();
  expect(adapter.getSnapshot().commandError).toBeNull();
  adapter.dispose();
});

test("dispose cancels an apply waiting on cleanup, and unavailable drafts issue no commands", async () => {
  const host = new Host();
  host.state = fixtureSnapshot("connected");
  const adapter = new ControlAdapter(host);
  await adapter.start();
  const apply = adapter.applyRoute(nextRoute, true);
  await expect.poll(() => host.commands).toEqual([{ type: "disconnect" }]);
  adapter.dispose();
  expect(await apply).toBe(false);
  expect(host.commands).toEqual([{ type: "disconnect" }]);
  const second = new ControlAdapter(new Host());
  await second.start();
  expect(
    await second.applyRoute({ ...nextRoute, sourceId: "missing" }, true),
  ).toBe(false);
  expect(second.getSnapshot().commandError?.code).toBe("invalid-selection");
  second.dispose();
});

test("token submission is separate, serial, redacted from view state, and does not block mute", async () => {
  const host = new Host();
  const save = deferred<void>();
  let calls = 0;
  host.configureBot = async (token) => {
    expect(token).toBe("synthetic-credential");
    calls++;
    await save.promise;
  };
  const adapter = new ControlAdapter(host);
  await adapter.start();
  const pending = adapter.configureBot("synthetic-credential");
  expect(adapter.getSnapshot().configuringBot).toBe(true);
  expect(JSON.stringify(adapter.getSnapshot())).not.toContain(
    "synthetic-credential",
  );
  expect(await adapter.configureBot("second-synthetic-credential")).toBe(false);
  expect(await adapter.dispatch({ type: "connect" })).toBe(false);
  expect(await adapter.dispatch({ type: "set-muted", muted: true })).toBe(true);
  expect(host.commands).toEqual([{ type: "set-muted", muted: true }]);
  save.resolve();
  expect(await pending).toBe(true);
  expect(calls).toBe(1);
  expect(adapter.getSnapshot().configuringBot).toBe(false);
  expect(adapter.getSnapshot().snapshot?.bot).toEqual(fixtureSnapshot().bot);
  adapter.dispose();
});

test("unconfirmed token saves preserve authoritative status without echoing a token; late failures cannot replace current state", async () => {
  const host = new Host();
  host.configureBot = async (token) => {
    // Saving may finish and publish a new profile before the IPC response fails.
    host.emit({ bot: { status: "ready", name: "Updated Bot", message: null } });
    throw { code: "configuration", message: `Rejected ${token}` };
  };
  const adapter = new ControlAdapter(host);
  await adapter.start();
  expect(await adapter.configureBot("synthetic-credential")).toBe(false);
  expect(adapter.getSnapshot().configurationError?.code).toBe("configuration");
  expect(adapter.getSnapshot().configurationError?.message).toBe(
    "Could not confirm this bot token was saved. Check bot status and try again.",
  );
  expect(JSON.stringify(adapter.getSnapshot())).not.toContain(
    "synthetic-credential",
  );
  expect(adapter.getSnapshot().snapshot?.bot).toEqual({
    status: "ready",
    name: "Updated Bot",
    message: null,
  });
  const late = deferred<void>();
  host.configureBot = () => late.promise;
  const pending = adapter.configureBot("synthetic-credential");
  await adapter.start();
  late.reject(new Error("synthetic-credential"));
  expect(await pending).toBe(false);
  expect(adapter.getSnapshot().configurationError).toBeNull();
  expect(adapter.getSnapshot().configuringBot).toBe(false);
  adapter.dispose();
});

test("active and incomplete sessions cannot configure credentials", async () => {
  for (const name of [
    "connecting",
    "connected",
    "recovering",
    "disconnecting",
    "cleanup-incomplete",
  ] as const) {
    const host = new Host();
    host.state = fixtureSnapshot(name);
    let called = false;
    host.configureBot = async () => {
      called = true;
    };
    const adapter = new ControlAdapter(host);
    await adapter.start();
    expect(await adapter.configureBot("synthetic-credential")).toBe(false);
    expect(called).toBe(false);
    adapter.dispose();
  }
});
