import { expect, test } from "@playwright/test";
import { deliveryLabel } from "../src/contract";
import { MockControlAdapter } from "../src/mockAdapter";

test("adapter snapshots are stable; subscriptions unsubscribe and invalid commands leave state intact", async () => {
  const adapter = new MockControlAdapter();
  const before = adapter.getSnapshot();
  expect(adapter.getSnapshot()).toBe(before);
  let notifications = 0;
  const unsubscribe = adapter.subscribe(() => notifications++);
  expect(
    await adapter.dispatch({ type: "set-volume", percent: Number.NaN }),
  ).toMatchObject({ ok: false });
  expect(
    await adapter.dispatch({ type: "select-source", sourceId: "unknown" }),
  ).toMatchObject({ ok: false });
  expect(
    await adapter.dispatch({
      type: "select-destination",
      serverId: "oneshot",
      channelId: "session",
    }),
  ).toMatchObject({ ok: false });
  expect(adapter.getSnapshot()).toBe(before);
  expect(notifications).toBe(0);
  await adapter.dispatch({ type: "set-muted", muted: true });
  expect(adapter.getSnapshot()).not.toBe(before);
  expect(before.output.muted).toBe(false);
  expect(notifications).toBe(1);
  unsubscribe();
  await adapter.dispatch({ type: "set-volume", percent: 37 });
  expect(notifications).toBe(1);
});

test("adapter rejects route changes during sessions and requires an available source and explicit channel", async () => {
  const adapter = new MockControlAdapter();
  adapter.preview("recovering");
  const before = adapter.getSnapshot();
  expect(
    await adapter.dispatch({ type: "select-source", sourceId: "cable-b" }),
  ).toMatchObject({ ok: false });
  expect(
    await adapter.dispatch({
      type: "select-destination",
      serverId: "oneshot",
      channelId: "tavern",
    }),
  ).toMatchObject({ ok: false });
  expect(adapter.getSnapshot()).toBe(before);
  await adapter.dispatch({ type: "disconnect" });
  await adapter.dispatch({
    type: "select-destination",
    serverId: "oneshot",
    channelId: null,
  });
  expect(await adapter.dispatch({ type: "connect" })).toMatchObject({
    ok: false,
  });
  await adapter.dispatch({
    type: "select-destination",
    serverId: "oneshot",
    channelId: "tavern",
  });
  adapter.removeDevice();
  expect(await adapter.dispatch({ type: "connect" })).toMatchObject({
    ok: false,
  });
  adapter.configureDemoBot("unconfigured");
  adapter.returnDevice();
  expect(await adapter.dispatch({ type: "connect" })).toMatchObject({
    ok: false,
  });
});

test("delivery never conflates input level, mute, zero volume, or connection state", async () => {
  const adapter = new MockControlAdapter();
  expect(deliveryLabel(adapter.getSnapshot())).toBe("Not sending");
  adapter.preview("streaming");
  await adapter.dispatch({ type: "set-volume", percent: 0 });
  expect(deliveryLabel(adapter.getSnapshot())).toBe("Volume is zero");
  expect(adapter.getSnapshot().capture.levelDbfs).toBe(-22);
  await adapter.dispatch({ type: "set-muted", muted: true });
  expect(deliveryLabel(adapter.getSnapshot())).toBe("Muted");
  await adapter.dispatch({ type: "set-muted", muted: false });
  await adapter.dispatch({ type: "set-volume", percent: 80 });
  adapter.setDemoSignal(true);
  expect(deliveryLabel(adapter.getSnapshot())).toBe("Silent input");
  expect(adapter.getSnapshot().connection.phase).toBe("connected");
  adapter.removeDevice();
  expect(deliveryLabel(adapter.getSnapshot())).toBe("Not sending");
  expect(adapter.getSnapshot().capture.levelDbfs).toBeNull();
});
