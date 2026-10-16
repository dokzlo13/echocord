import { expect, test } from "@playwright/test";

test("frozen native audio expires without control renders while gateway ages independently", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=outputs");
  const source = page.locator(".source-card");
  const discord = page.locator('.dest-card[data-kind="discord"]');
  await expect(source).toHaveAttribute("data-stale", "false");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.stopTelemetry();
    t.observeTelemetry();
    t.emit({ settings: { ...t.current.settings, meterStyle: "history" } });
  });
  await expect(source).toHaveAttribute("data-stale", "true", { timeout: 2000 });
  await expect(source.locator(".mf-stale")).toBeVisible();
  await expect(discord.locator(".mf-stale")).toBeVisible();
  await expect(discord.locator(".stat").first()).toContainText("38");
  await expect(
    discord.locator(".stat").first().locator("[data-value]"),
  ).toHaveText("—", { timeout: 2500 });
  const before = await page.evaluate(() => window.uiHarness.commits);
  await page.evaluate(() => window.uiHarness.transport.observeTelemetry());
  await expect(source).toHaveAttribute("data-stale", "false");
  await expect(source.locator(".mf-stale")).toBeHidden();
  expect(await page.evaluate(() => window.uiHarness.commits)).toBe(before);
});

test("measured silence stays valid, stopped controls invalidate old peaks, and counters show recent milliseconds", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=connected");
  await expect(page.locator(".source-card")).toHaveAttribute(
    "data-stale",
    "false",
  );
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.stopTelemetry();
    t.observeTelemetry();
    t.observeTelemetry(
      {
        ...t.current.telemetry,
        inputs: t.current.telemetry.inputs.map((v) => ({
          ...v,
          levelDbfs: -96,
        })),
        destinations: t.current.telemetry.destinations.map((v) => ({
          ...v,
          levelDbfs: -96,
          droppedFrames: 96,
          underrunFrames: 48,
        })),
      },
      t.current.telemetry.clockMs + 1,
    );
  });
  const source = page.locator(".source-card");
  await expect(source).toHaveAttribute("data-stale", "false");
  await expect(source.locator(".mf-stale")).toBeHidden();
  await expect(page.locator(".dest-card .stat").last()).toContainText("2 ms");
  await source.getByRole("button", { name: /^Stop capturing / }).click();
  expect(
    await page.evaluate(
      () => window.uiHarness.adapter.telemetry.read("src:strip:1").peak,
    ),
  ).toBeNull();
  await expect(source.locator(".mf-idle")).toHaveText("Not capturing");
});

test("reconnecting displays actual handoff measurements without claiming confirmed transmission", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=connected");
  const card = page.locator(".dest-card");
  await expect(card).toHaveAttribute("data-stale", "false");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.stopTelemetry();
    t.emit({
      destinations: t.current.destinations.map((v) => ({
        ...v,
        connection: {
          phase: "recovering",
          attempt: 1,
          message: "Checking room permissions.",
        },
      })),
    });
    t.observeTelemetry();
  });
  await expect(card.locator(".badge")).toHaveText("Reconnecting");
  await expect(card).toHaveAttribute("data-stale", "false");
  await expect(card).not.toContainText("Not sending");
  await expect(card.locator(".d-meta")).toHaveCount(0);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      telemetry: {
        ...t.current.telemetry,
        clockMs: t.current.telemetry.clockMs + 1,
        destinations: t.current.telemetry.destinations.map((v) => ({
          ...v,
          observedAtMs: null,
        })),
      },
    });
  });
  await expect(card.locator(".mf-stale")).toBeVisible();
});
