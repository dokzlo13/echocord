import { readdir, readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";
import { fixtureSnapshot } from "../src/fixtures";
import type {
  ControlCommand,
  ControlSnapshot,
  MixTelemetry,
} from "../src/generated";

const releaseUrl = `http://127.0.0.1:${process.env.ECHOCORD_PREVIEW_PORT ?? 4185}`;

interface Host {
  calls: { name: string; args: Record<string, unknown> }[];
  emit: (s: ControlSnapshot) => void;
  telemetry: (t: MixTelemetry) => void;
  visible: (v: boolean) => void;
  activeListeners: number;
}

test("release outside Tauri shows a real unavailable error and excludes all fixture data", async ({
  page,
}) => {
  await page.goto(`${releaseUrl}/?fixture=connected`);
  await expect(page.getByRole("alert")).toContainText(
    "Open the Echocord desktop app",
  );
  await expect(page.locator(".source-card")).toHaveCount(0);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.locator("main")).not.toHaveAttribute("inert", "");
  const assets = await readdir(new URL("../dist/assets/", import.meta.url));
  for (const name of assets.filter((x) => x.endsWith(".js"))) {
    const script = await readFile(
      new URL(`../dist/assets/${name}`, import.meta.url),
      "utf8",
    );
    for (const forbidden of [
      "FixtureTransport",
      "Preview player",
      "synthetic-test-token",
      "SCENARIOS",
      "futureCapacity",
      "design prototype",
    ])
      expect(script).not.toContain(forbidden);
  }
});

test("release binds exact Tauri names and telemetry event without faking accepted operations", async ({
  page,
}) => {
  await page.addInitScript((initial: ControlSnapshot) => {
    let current = initial,
      next = 0;
    const callbacks = new Map<
      number,
      (event: { event: string; id: number; payload: unknown }) => void
    >();
    const registrations = new Map<number, { event: string; handler: number }>();
    const calls: Host["calls"] = [];
    const host = window as unknown as {
      isTauri: boolean;
      __TAURI_INTERNALS__: unknown;
      __TAURI_EVENT_PLUGIN_INTERNALS__: unknown;
      echocordTestHost: Host;
    };
    host.isTauri = true;
    const event = (name: string, payload: unknown) => {
      for (const [id, r] of registrations)
        if (r.event === name)
          callbacks.get(r.handler)?.({ event: name, id, payload });
    };
    host.echocordTestHost = {
      calls,
      activeListeners: 0,
      emit: (s) => {
        current = s;
        event("echocord://snapshot", s);
      },
      telemetry: (t) => event("echocord://telemetry", t),
      visible: (v) => event("echocord://visibility", v),
    };
    host.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" } },
      transformCallback: (
        callback: (e: { event: string; id: number; payload: unknown }) => void,
      ) => {
        const id = ++next;
        callbacks.set(id, callback);
        return id;
      },
      invoke: async (name: string, args: Record<string, unknown>) => {
        calls.push({ name, args });
        if (name === "plugin:event|listen") {
          const id = ++next;
          registrations.set(id, {
            event: args.event as string,
            handler: args.handler as number,
          });
          host.echocordTestHost.activeListeners = registrations.size;
          return id;
        }
        if (name === "plugin:event|unlisten") return;
        if (name === "get_visibility") return true;
        if (name === "get_snapshot") return current;
        if (name === "dispatch" || name.startsWith("plugin:window|")) return;
        throw Error("Unexpected test IPC");
      },
    };
    host.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: (_event: string, id: number) => {
        registrations.delete(id);
        host.echocordTestHost.activeListeners = registrations.size;
      },
    };
  }, fixtureSnapshot());
  await page.goto(releaseUrl);
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "idle",
  );
  await page.getByRole("button", { name: /^Connect / }).click();
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "idle",
  );
  const live = { ...fixtureSnapshot("connected"), revision: 20 };
  await page.evaluate(
    (s) =>
      (window as unknown as { echocordTestHost: Host }).echocordTestHost.emit(
        s,
      ),
    live,
  );
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "live",
  );
  await page.getByRole("button", { name: "Minimize", exact: true }).click();
  await page
    .getByRole("button", { name: "Close to tray", exact: true })
    .click();
  await page
    .locator(".brand")
    .dispatchEvent("mousedown", { button: 0, detail: 1 });
  await page.evaluate(
    (t) =>
      (
        window as unknown as { echocordTestHost: Host }
      ).echocordTestHost.telemetry({
        ...t,
        clockMs: t.clockMs + 1,
        mixObservedAtMs: t.clockMs + 1,
        destinations: t.destinations.map((d) => ({
          ...d,
          observedAtMs: t.clockMs + 1,
          levelDbfs: -6,
        })),
        outputLevelDbfs: -6,
      }),
    live.telemetry,
  );
  await expect
    .poll(() =>
      page
        .locator(".dest-card")
        .evaluate((e) => e.style.getPropertyValue("--level")),
    )
    .toBe("0.900");
  const calls = await page.evaluate(
    () =>
      (window as unknown as { echocordTestHost: Host }).echocordTestHost.calls,
  );
  const dispatch = calls.find((c) => c.name === "dispatch");
  expect(dispatch?.args.command as ControlCommand).toEqual({
    type: "connect-destination",
    destinationId: "dest:1",
  });
  expect(
    calls.some(
      (c) =>
        c.name === "plugin:event|listen" &&
        c.args.event === "echocord://telemetry",
    ),
  ).toBe(true);
  for (const name of [
    "plugin:window|minimize",
    "plugin:window|close",
    "plugin:window|start_dragging",
  ])
    expect(calls.some((c) => c.name === name)).toBe(true);
});
