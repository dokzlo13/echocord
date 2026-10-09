import { expect, test } from "@playwright/test";
import { appError, ControlAdapter, type Transport } from "../src/adapter";
import { fixtureSnapshot } from "../src/fixtures";
import type { ControlCommand, ControlSnapshot } from "../src/generated";

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
