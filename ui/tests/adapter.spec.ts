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

test("credential exceptions cannot echo a token, and late failures after restart cannot replace current state", async () => {
  const host = new Host();
  host.configureBot = async (token) => {
    throw { code: "configuration", message: `Rejected ${token}` };
  };
  const adapter = new ControlAdapter(host);
  await adapter.start();
  expect(await adapter.configureBot("synthetic-credential")).toBe(false);
  expect(adapter.getSnapshot().configurationError?.code).toBe("configuration");
  expect(JSON.stringify(adapter.getSnapshot())).not.toContain(
    "synthetic-credential",
  );
  expect(adapter.getSnapshot().snapshot?.bot.status).toBe("ready");
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

test("level history contains bounded received changes only, ignores stale events and clears for a missing or different input", async () => {
  const host = new Host();
  host.state = fixtureSnapshot("connected");
  const adapter = new ControlAdapter(host);
  await adapter.start();
  expect(adapter.getSnapshot().levelHistory.map((item) => item.dbfs)).toEqual([
    -21,
  ]);
  host.emit({ output: { volumePercent: 40, muted: true } });
  expect(adapter.getSnapshot().levelHistory).toHaveLength(1);
  host.emit({ capture: { ...host.state.capture, levelDbfs: -19 } });
  host.receive({ ...fixtureSnapshot("connected"), revision: 1 });
  expect(adapter.getSnapshot().levelHistory.map((item) => item.dbfs)).toEqual([
    -21, -19,
  ]);
  for (let index = 0; index < 200; index++)
    host.emit({
      capture: { ...host.state.capture, levelDbfs: index % 2 ? -18 : -22 },
    });
  expect(adapter.getSnapshot().levelHistory).toHaveLength(96);
  host.emit({
    capture: { ...host.state.capture, phase: "missing", levelDbfs: null },
  });
  expect(adapter.getSnapshot().levelHistory).toEqual([]);
  host.emit({
    sessionId: 2,
    capture: { ...host.state.capture, phase: "running", levelDbfs: -30 },
  });
  expect(adapter.getSnapshot().levelHistory.map((item) => item.dbfs)).toEqual([
    -30,
  ]);
  host.emit({
    selection: { ...host.state.selection, sourceId: "cable-b" },
    capture: { ...host.state.capture, levelDbfs: -12 },
  });
  expect(adapter.getSnapshot().levelHistory.map((item) => item.dbfs)).toEqual([
    -12,
  ]);
  adapter.dispose();
});
