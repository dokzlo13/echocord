import { expect, type Page, test } from "@playwright/test";
import { fixtureSnapshot } from "../src/fixtures";

const picker = (page: Page) =>
  page.getByRole("dialog", { name: "Audio sources" });
const openSource = (page: Page) =>
  page
    .getByRole("button", {
      name: /^(Change input|Change source|Choose source)$/,
    })
    .click();
const application = (page: Page, pid = "7412") =>
  picker(page)
    .getByRole("button", { name: /Ambient player/ })
    .filter({ hasText: `PID ${pid}` });
async function harness(page: Page) {
  await page.clock.install();
  await page.goto("/tests/harness.html");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
}
async function stageApplication(page: Page) {
  await openSource(page);
  await picker(page)
    .getByRole("tab", { name: "Applications", exact: true })
    .click();
  await application(page).click();
}
async function transportFits(page: Page) {
  const transport = page.getByRole("region", { name: "Session controls" });
  await expect(transport).toBeInViewport({ ratio: 1 });
  for (const button of await transport.getByRole("button").all()) {
    await expect(button).toBeInViewport({ ratio: 1 });
    const bounds = await button.evaluate((element) => {
      const box = element.getBoundingClientRect();
      const label = element.querySelector(".mantine-Button-label");
      const range = document.createRange();
      range.selectNodeContents(label ?? element);
      const text = range.getBoundingClientRect();
      return {
        left: box.left,
        right: box.right,
        textLeft: text.left,
        textRight: text.right,
        viewport: innerWidth,
        appRight:
          document.querySelector("main")?.getBoundingClientRect().right ?? 0,
      };
    });
    expect(bounds.appRight).toBeLessThanOrEqual(bounds.viewport);
    expect(bounds.left).toBeGreaterThanOrEqual(0);
    expect(bounds.right).toBeLessThanOrEqual(bounds.viewport - 8);
    expect(bounds.textLeft).toBeGreaterThanOrEqual(bounds.left);
    expect(bounds.textRight).toBeLessThanOrEqual(bounds.right);
  }
}

test("source-kind keyboard selection stages opaque app identity, discards and applies explicitly", async ({
  page,
}) => {
  await harness(page);
  await openSource(page);
  const recording = picker(page).getByRole("tab", {
    name: "Recording inputs",
    exact: true,
  });
  await expect(recording).toHaveAttribute("aria-selected", "true");
  await recording.focus();
  await recording.press("ArrowLeft");
  await expect(
    picker(page).getByRole("tab", { name: "Applications", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(
    picker(page).getByText(
      "Browser capture can include other tabs and windows. Keep calls in another app. Local playback continues.",
      { exact: true },
    ),
  ).toHaveCount(1);
  await expect(
    picker(page).getByRole("button", { name: /Ambient player/ }),
  ).toHaveCount(2);
  await expect(picker(page)).not.toContainText("fixture-app-player");
  await application(page).click();
  await expect(
    page.getByRole("heading", { name: "Recording input", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Queued route" }),
  ).toContainText("Ambient player");
  expect(await page.evaluate(() => window.uiHarness.commands)).toEqual([]);
  await openSource(page);
  await expect(application(page)).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Discard", exact: true }).click();
  await openSource(page);
  await expect(
    picker(page).getByRole("tab", { name: "Recording inputs", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Escape");
  await stageApplication(page);
  await page
    .getByRole("button", { name: "Apply & reconnect", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Application audio", exact: true }),
  ).toBeVisible();
  expect(await page.evaluate(() => window.uiHarness.commands)).toEqual([
    { type: "disconnect" },
    { type: "select-source", sourceId: "fixture-app-player" },
    { type: "connect" },
  ]);
  await expect(page.getByTestId("destination")).toContainText("Session room");
});

for (const width of [480, 420]) {
  test(`active local refresh preserves app draft/cached results and bounded transport at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: width === 480 ? 600 : 540 });
    await harness(page);
    await stageApplication(page);
    await openSource(page);
    await page.evaluate(() => {
      window.uiHarness.rejectSourceRefresh = true;
    });
    const refresh = picker(page).getByRole("button", {
      name: "Refresh sources",
      exact: true,
    });
    await expect(refresh).toBeEnabled();
    await refresh.click();
    await expect(
      picker(page).getByRole("button", { name: "Refreshing…", exact: true }),
    ).toBeDisabled();
    await page.clock.runFor(400);
    await expect(picker(page).getByRole("alert")).toContainText(
      "Could not enumerate application windows",
    );
    await expect(refresh).toBeEnabled();
    await expect(application(page)).toHaveAttribute("aria-pressed", "true");
    await expect(
      picker(page).getByRole("button", { name: /Table Audio/ }),
    ).toContainText("Extended soundtrack and ambience collection");
    const geometry = await picker(page).evaluate((element) => {
      const box = element.getBoundingClientRect();
      const transport = document
        .querySelector('[aria-label="Session controls"]')
        ?.getBoundingClientRect();
      const scrolling = [...element.querySelectorAll("*")].filter(
        (node) =>
          ["auto", "scroll"].includes(getComputedStyle(node).overflowY) &&
          node.scrollHeight > node.clientHeight + 1,
      );
      return {
        width: box.width,
        height: box.height,
        left: box.left,
        right: box.right,
        bottom: box.bottom,
        transportTop: transport?.top ?? 0,
        scrolling: scrolling.map((node) => node.className),
      };
    });
    expect(geometry.width).toBeLessThanOrEqual(344);
    expect(geometry.height).toBeLessThanOrEqual(368);
    expect(geometry.left).toBeGreaterThanOrEqual(0);
    expect(geometry.right).toBeLessThanOrEqual(width);
    expect(geometry.bottom).toBeLessThan(geometry.transportTop);
    expect(geometry.scrolling).toHaveLength(1);
    expect(geometry.scrolling[0]).toContain("drawerList");
    await expect(refresh).toBeInViewport({ ratio: 1 });
    await transportFits(page);
    const baseline = await page.evaluate(() => window.uiHarness.commits);
    await page.evaluate(() => window.uiHarness.burst(1000));
    await page.clock.runFor(100);
    expect(await page.evaluate(() => window.uiHarness.commits)).toBe(baseline);
    await page
      .getByRole("button", { name: "Mute output", exact: true })
      .click();
    await expect(picker(page)).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Mute output", exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    await expect(
      page.getByRole("region", { name: "Queued route" }),
    ).toContainText("Ambient player");
    expect(
      await page.evaluate(() =>
        window.uiHarness.commands.map((command) => command.type),
      ),
    ).toEqual(["refresh-sources", "set-muted"]);
  });
}

test("cached-error source picker and Stop remain contained after live 480 to 420 resize", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 600 });
  await harness(page);
  await openSource(page);
  await picker(page)
    .getByRole("tab", { name: "Applications", exact: true })
    .click();
  await page.evaluate(() =>
    window.uiHarness.emit({
      sourceDiscovery: {
        phase: "error",
        message:
          "Could not enumerate application windows. Refresh sources to retry.",
      },
    }),
  );
  await page.setViewportSize({ width: 420, height: 540 });
  await page.clock.runFor(400);
  await transportFits(page);
  await expect(
    page.getByRole("button", { name: "Stop", exact: true }),
  ).toBeInViewport({ ratio: 1 });
  const bounds = await picker(page).evaluate((element) => {
    const box = element.getBoundingClientRect();
    return { width: box.width, right: box.right, bottom: box.bottom };
  });
  expect(bounds.width).toBeLessThanOrEqual(336);
  expect(bounds.right).toBeLessThanOrEqual(420);
  expect(bounds.bottom).toBeLessThan(474);
  await expect(
    picker(page).getByRole("button", { name: "Refresh sources", exact: true }),
  ).toBeEnabled();
  await expect(picker(page).getByRole("alert")).toContainText(
    "Could not enumerate application windows",
  );
  await expect(
    picker(page).getByRole("button", { name: /Ambient player/ }),
  ).toHaveCount(2);
});

for (const status of ["unconfigured", "offline"] as const) {
  test(`local discovery and source staging work with Discord ${status}`, async ({
    page,
  }) => {
    await harness(page);
    await page.evaluate((status) => {
      const snapshot = window.uiHarness.snapshot();
      window.uiHarness.emit({
        connection: { phase: "disconnected" },
        selection: { ...snapshot.selection, sourceId: null },
        sources: [],
        sourceDiscovery: { phase: "idle" },
        bot:
          status === "unconfigured"
            ? { status: "unconfigured", name: null, message: null }
            : snapshot.bot,
        discovery:
          status === "unconfigured"
            ? { phase: "idle" }
            : { phase: "error", message: "Discord is offline" },
      });
      window.uiHarness.sourceResults = [
        {
          id: "opaque-local-app",
          name: "Local audio app",
          kind: "application",
          available: true,
          detail: "PID 9950 · includes child processes",
        },
      ];
    }, status);
    await expect(
      page.getByRole("heading", { name: "Audio source", exact: true }),
    ).toBeVisible();
    await openSource(page);
    await picker(page)
      .getByRole("button", { name: "Refresh sources", exact: true })
      .click();
    await page.clock.runFor(400);
    await expect(
      picker(page).getByRole("button", { name: /Local audio app/ }),
    ).toBeEnabled();
    expect(
      await page.evaluate(() => window.uiHarness.snapshot().selection.sourceId),
    ).toBeNull();
    await picker(page)
      .getByRole("button", { name: /Local audio app/ })
      .click();
    await expect(
      page.getByRole("region", { name: "Queued route" }),
    ).toContainText("Local audio app");
    expect(await page.evaluate(() => window.uiHarness.commands)).toEqual([
      { type: "refresh-sources" },
    ]);
    await expect(
      page.getByRole("button", { name: "Apply selection", exact: true }),
    ).toBeDisabled();
    await openSource(page);
    await page.evaluate(() => {
      window.uiHarness.sourceResults = [];
    });
    await picker(page)
      .getByRole("button", { name: "Refresh sources", exact: true })
      .click();
    await page.clock.runFor(400);
    await expect(picker(page)).toContainText(
      "No applications found. Open an app window, then refresh.",
    );
    await expect(
      picker(page).getByRole("button", {
        name: "Refresh sources",
        exact: true,
      }),
    ).toBeEnabled();
  });
}

test("an unavailable app remains current while a restarted lifetime requires explicit reselection", async ({
  page,
}) => {
  await harness(page);
  await page.setViewportSize({ width: 420, height: 540 });
  await page.evaluate(
    (initial) => window.uiHarness.emit(initial),
    fixtureSnapshot("application-connected"),
  );
  await transportFits(page);
  await openSource(page);
  await application(page, "7612").click();
  const queuedInput = page
    .getByRole("region", { name: "Queued route" })
    .locator("dd")
    .first();
  await expect(queuedInput).toContainText("PID 7412");
  await expect(queuedInput).toContainText("PID 7612");
  await expect(
    queuedInput.locator('[title^="Confirmed input:"]'),
  ).toHaveAttribute(
    "title",
    "Confirmed input: Ambient player\nPID 7412 · includes child processes",
  );
  await expect(queuedInput.locator('[title^="Queued input:"]')).toHaveAttribute(
    "title",
    "Queued input: Ambient player\nPID 7612 · includes child processes",
  );
  await page.getByRole("button", { name: "Discard", exact: true }).click();
  await page.evaluate((initial) => {
    window.uiHarness.emit({
      ...initial,
      connection: {
        phase: "error",
        code: "capture-failed",
        message: "The application exited",
      },
    });
    const old = window.uiHarness
      .snapshot()
      .sources.find((source) => source.id === "fixture-app-player");
    if (!old) throw new Error("Missing application fixture");
    window.uiHarness.sourceResults = [
      old,
      {
        ...old,
        id: "opaque-new-lifetime",
        available: true,
        detail: "PID 9950 · includes child processes",
      },
    ];
  }, fixtureSnapshot("application-missing"));
  await expect(
    page.getByRole("heading", { name: "Application audio", exact: true }),
  ).toBeVisible();
  await expect(page.getByTestId("capture-status")).toHaveText(
    "Application unavailable",
  );
  await expect(page.getByRole("alert")).toContainText(
    "refresh sources and select it again",
  );
  await openSource(page);
  await expect(application(page)).toBeDisabled();
  await expect(application(page)).toContainText("Current");
  await picker(page)
    .getByRole("button", { name: "Refresh sources", exact: true })
    .click();
  await page.clock.runFor(400);
  expect(
    await page.evaluate(() => window.uiHarness.snapshot().selection.sourceId),
  ).toBe("fixture-app-player");
  await application(page, "9950").click();
  await expect(
    page.getByRole("region", { name: "Queued route" }),
  ).toContainText("Ambient player");
  await expect(queuedInput).toContainText("PID 7412");
  await expect(queuedInput).toContainText("PID 9950");
  await expect(
    page
      .getByRole("heading", { name: "Application audio", exact: true })
      .locator("..", {
        has: page.getByText("Application unavailable", { exact: true }),
      }),
  ).toBeVisible();
  await expect(
    page.getByTitle("Ambient player\nPID 7412 · includes child processes", {
      exact: true,
    }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Change room", exact: true }).click();
  await page.getByRole("button", { name: "The tavern", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "Queued route" }).locator("dt"),
  ).toHaveText(["Input", "Server", "Room"]);
  await transportFits(page);
  expect(await page.evaluate(() => window.uiHarness.commands)).toEqual([
    { type: "refresh-sources" },
  ]);
  await page
    .getByRole("button", { name: "Apply selection", exact: true })
    .click();
  expect(
    await page.evaluate(() => window.uiHarness.snapshot().selection.sourceId),
  ).toBe("opaque-new-lifetime");
  expect(await page.evaluate(() => window.uiHarness.commands)).toEqual([
    { type: "refresh-sources" },
    { type: "select-source", sourceId: "opaque-new-lifetime" },
    {
      type: "select-destination",
      serverId: "1234567890123456790",
      channelId: "9876543210987654323",
    },
  ]);
});

test("application partial apply and unsupported capture errors preserve truthful source recovery", async ({
  page,
}) => {
  await harness(page);
  await stageApplication(page);
  await page.getByRole("button", { name: "Change room", exact: true }).click();
  await page.getByRole("button", { name: "The tavern", exact: true }).click();
  await page.evaluate(() => {
    window.uiHarness.rejectDestination = true;
  });
  await page
    .getByRole("button", { name: "Apply & reconnect", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Application audio", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Queued route" }).locator("dt"),
  ).toHaveText(["Server", "Room"]);
  await expect(page.getByTestId("destination")).toContainText("Session room");
  await page.goto("/?fixture=application-error");
  await expect(page.getByRole("alert")).toContainText(
    "Application capture failed",
  );
  await expect(page.getByRole("alert")).not.toContainText(
    "Recording input failed",
  );
  await page.getByRole("button", { name: "Details", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText(
    "This Windows version does not support application audio capture. Use a recording input.",
  );
  await expect(page.getByRole("dialog")).toContainText(
    "Original local playback continues",
  );
  await page
    .getByRole("button", { name: "Close audio details", exact: true })
    .click();
  await openSource(page);
  await picker(page)
    .getByRole("tab", { name: "Recording inputs", exact: true })
    .click();
  await expect(
    picker(page).getByRole("button", { name: /CABLE-B Output/ }),
  ).toBeEnabled();
  await expect(
    page.getByRole("button", { name: /Mute local|Suppress local/ }),
  ).toHaveCount(0);
});
