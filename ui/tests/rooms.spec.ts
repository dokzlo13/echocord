import { expect, type Page, test } from "@playwright/test";

async function start(page: Page, fixture = "rooms") {
  await page.goto(`/tests/harness.html?fixture=${fixture}`);
  await expect(page.locator(".dest-card")).toHaveCount(4);
}
async function show(page: Page, name: string) {
  await page.getByRole("button", { name: `Show ${name}`, exact: true }).click();
  return page
    .locator(".dest-card")
    .filter({ has: page.locator(".id-name", { hasText: name }) });
}

test("restored rooms and playback stay idle until each explicit start", async ({
  page,
}) => {
  await start(page, "rooms-restored");
  await expect(page.locator('.dest-card[data-state="idle"]')).toHaveCount(4);
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([]);
  const second = await show(page, "Сессия");
  await second.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(second).toHaveAttribute("data-state", "live");
  await expect(page.locator('.dest-card[data-state="idle"]')).toHaveCount(3);
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([{ type: "connect-destination", destinationId: "dest:2" }]);
});

test("room mute gain disconnect cancel and source controls leave peers untouched", async ({
  page,
}) => {
  await start(page);
  const second = await show(page, "Сессия");
  await second
    .getByRole("button", { name: "Mute Сессия", exact: true })
    .click();
  await second
    .getByRole("slider", { name: "Сессия volume" })
    .press("ArrowLeft");
  await second.getByRole("slider", { name: "Сессия volume" }).blur();
  await second.getByRole("button", { name: "Disconnect", exact: true }).click();
  await page.evaluate(() => {
    window.uiHarness.transport.holdConnect = true;
  });
  await second.getByRole("button", { name: "Connect", exact: true }).click();
  await second.getByRole("button", { name: "Cancel", exact: true }).click();
  await page
    .locator(".source-card")
    .getByRole("button", { name: "Stop capturing Preview player" })
    .click();
  await expect(page.locator('.dest-card[data-state="live"]')).toHaveCount(3);
  const state = await page.evaluate(() => ({
    commands: window.uiHarness.transport.commands,
    destinations: window.uiHarness.transport.current.destinations,
  }));
  expect(
    state.commands
      .filter((c) => "destinationId" in c)
      .every((c) => "destinationId" in c && c.destinationId === "dest:2"),
  ).toBe(true);
  expect(
    state.destinations
      .filter((d) => d.destinationId !== "dest:2")
      .every(
        (d) =>
          d.connection.phase === "connected" && d.sessionId === 1 && !d.muted,
      ),
  ).toBe(true);
});

test("room-specific permission and cleanup faults leave other rooms and playback reachable", async ({
  page,
}) => {
  await start(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const ds = structuredClone(t.current.destinations);
    const denied = ds.find((d) => d.destinationId === "dest:2");
    const cleanup = ds.find((d) => d.destinationId === "dest:3");
    if (denied)
      denied.connection = {
        phase: "error",
        code: "permission-denied",
        message: "The bot cannot speak in this room.",
      };
    if (cleanup)
      cleanup.connection = {
        phase: "cleanup-incomplete",
        message: "Room cleanup has not finished. Retry disconnect.",
      };
    t.emit({ destinations: ds });
  });
  const denied = await show(page, "Сессия");
  await expect(denied.getByRole("alert")).toContainText("cannot speak");
  const cleanup = await show(page, "Listening room");
  await expect(
    cleanup.getByRole("button", { name: "Retry disconnect", exact: true }),
  ).toBeVisible();
  const local = await show(page, "Preview headphones");
  await local.getByRole("button", { name: "Stop", exact: true }).click();
  await local.getByRole("button", { name: "Play", exact: true }).click();
  await expect(local).toHaveAttribute("data-state", "live");
  const first = await show(page, "Session room");
  await expect(first).toHaveAttribute("data-state", "live");
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([
    { type: "disconnect-destination", destinationId: "local:1" },
    { type: "connect-destination", destinationId: "local:1" },
  ]);
});

for (const [width, height] of [
  [440, 640],
  [455, 455],
  [420, 420],
  [760, 440],
  [1000, 640],
] as const)
  test(`three rooms and playback preserve controls and focus at ${width}x${height}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await start(page);
    for (const name of [
      "Session room",
      "Сессия",
      "Listening room",
      "Preview headphones",
    ]) {
      const card = await show(page, name);
      const fits = await card.evaluate((el) => {
        const box = el.getBoundingClientRect();
        const list = el.closest(".dst-list")?.getBoundingClientRect();
        const controls = el
          .querySelector(".controls-row")
          ?.getBoundingClientRect();
        return (
          !!list &&
          !!controls &&
          box.top >= list.top - 1 &&
          box.bottom <= list.bottom + 1 &&
          Array.from(el.querySelectorAll(".controls-row > *")).every(
            (control) => {
              const r = control.getBoundingClientRect();
              return (
                r.left >= controls.left - 1 && r.right <= controls.right + 1
              );
            },
          )
        );
      });
      expect(fits).toBe(true);
      await expect(card.locator(".identity")).toBeFocused();
    }
    expect(
      await page.evaluate(() => [
        document.documentElement.scrollWidth,
        document.documentElement.scrollHeight,
      ]),
    ).toEqual([width, height]);
  });
