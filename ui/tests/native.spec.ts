import { readdir, readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";
import { fixtureSnapshot } from "../src/fixtures";
import type { ControlCommand, ControlSnapshot } from "../src/generated";

type TestHost = {
  emit: (snapshot: ControlSnapshot) => void;
  calls: { name: string; args: Record<string, unknown> }[];
  activeListeners: number;
};

test("production outside Tauri fails explicitly, excludes fixture transport, and cannot silently mock", async ({
  page,
}) => {
  await page.goto("http://127.0.0.1:4176/?fixture=connected");
  await expect(page.getByTestId("connection-status")).toHaveText(
    "App unavailable",
  );
  await expect(page.getByRole("alert")).toContainText(
    "Open the Echocord desktop app",
  );
  await expect(page.getByLabel("Fixture mode")).toHaveCount(0);
  await expect(page.getByLabel("Fixture state")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Connect", exact: true }),
  ).toBeDisabled();
  const assets = await readdir(new URL("../dist/assets/", import.meta.url));
  for (const file of assets.filter((name) => name.endsWith(".js"))) {
    const script = await readFile(
      new URL(`../dist/assets/${file}`, import.meta.url),
      "utf8",
    );
    expect(script).not.toContain("Fixture backend unavailable");
    expect(script).not.toContain("CABLE Output (VB-Audio Virtual Cable)");
  }
});

test("production uses actual Tauri SDK names/payloads and events; command acceptance never fakes completion", async ({
  page,
}) => {
  await page.addInitScript((initial: ControlSnapshot) => {
    let current = initial,
      next = 0;
    const callbacks = new Map<
      number,
      (event: { event: string; id: number; payload: ControlSnapshot }) => void
    >();
    const registered = new Map<number, { handler: number; event: string }>();
    const calls: TestHost["calls"] = [];
    const host = window as unknown as {
      isTauri: boolean;
      __TAURI_INTERNALS__: unknown;
      __TAURI_EVENT_PLUGIN_INTERNALS__: unknown;
      echocordTestHost: TestHost;
    };
    host.isTauri = true;
    const emit = (snapshot: ControlSnapshot) => {
      current = snapshot;
      for (const [id, registration] of registered)
        if (registration.event === "echocord://snapshot")
          callbacks.get(registration.handler)?.({
            event: "echocord://snapshot",
            id,
            payload: snapshot,
          });
    };
    host.echocordTestHost = { emit, calls, activeListeners: 0 };
    host.__TAURI_INTERNALS__ = {
      transformCallback: (
        callback: (event: {
          event: string;
          id: number;
          payload: ControlSnapshot;
        }) => void,
      ) => {
        const id = ++next;
        callbacks.set(id, callback);
        return id;
      },
      invoke: async (name: string, args: Record<string, unknown>) => {
        calls.push({ name, args });
        if (name === "plugin:event|listen") {
          const id = ++next;
          registered.set(id, {
            handler: args.handler as number,
            event: args.event as string,
          });
          host.echocordTestHost.activeListeners = registered.size;
          return id;
        }
        if (name === "plugin:event|unlisten") return;
        if (name === "get_visibility") return true;
        if (name === "get_snapshot") {
          if (!registered.size)
            throw new Error("Snapshot read before listener");
          return current;
        }
        if (name === "dispatch") {
          const command = args.command as ControlCommand;
          if (command.type === "set-muted")
            emit({
              ...current,
              revision: current.revision + 1,
              output: { ...current.output, muted: command.muted },
            });
          if (command.type === "set-volume")
            emit({
              ...current,
              revision: current.revision + 1,
              output: { ...current.output, volumePercent: command.percent },
            });
          return;
        }
        throw new Error("Unexpected IPC command");
      },
    };
    host.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: (_event: string, id: number) => {
        registered.delete(id);
        host.echocordTestHost.activeListeners = registered.size;
      },
    };
  }, fixtureSnapshot());
  await page.goto("http://127.0.0.1:4176/");
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  const connected = { ...fixtureSnapshot("connected"), revision: 20 };
  await page.evaluate(
    (snapshot) =>
      (
        window as unknown as { echocordTestHost: TestHost }
      ).echocordTestHost.emit(snapshot),
    connected,
  );
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await page.getByRole("button", { name: "Change input", exact: true }).click();
  await page
    .getByRole("button", { name: "Refresh sources", exact: true })
    .click();
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await expect(
    page.getByRole("dialog", { name: "Audio sources" }),
  ).toContainText("CABLE Output");
  await page.getByRole("button", { name: "Mute output", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Mute output", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await page.evaluate(
    (snapshot) =>
      (
        window as unknown as { echocordTestHost: TestHost }
      ).echocordTestHost.emit(snapshot),
    { ...fixtureSnapshot(), revision: 2 },
  );
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  const host = await page.evaluate(() => {
    const host = (window as unknown as { echocordTestHost: TestHost })
      .echocordTestHost;
    return { calls: host.calls, activeListeners: host.activeListeners };
  });
  expect(host.activeListeners).toBe(2);
  expect(host.calls.some((call) => call.name === "get_visibility")).toBe(true);
  expect(
    host.calls.some(
      (call) =>
        call.name === "plugin:event|listen" &&
        call.args.event === "echocord://visibility",
    ),
  ).toBe(true);
  expect(host.calls.find((call) => call.name === "get_snapshot")?.args).toEqual(
    {},
  );
  expect(
    host.calls.find(
      (call) =>
        call.name === "plugin:event|listen" &&
        call.args.event === "echocord://snapshot",
    )?.args.event,
  ).toBe("echocord://snapshot");
  expect(
    host.calls
      .filter((call) => call.name === "dispatch")
      .map((call) => call.args),
  ).toEqual([
    { command: { type: "connect" } },
    { command: { type: "refresh-sources" } },
    { command: { type: "set-muted", muted: true } },
  ]);
});
