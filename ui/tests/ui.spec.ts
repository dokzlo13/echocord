import { expect, type Page, test } from "@playwright/test";
import { fixtureNames } from "../src/fixtures";

async function preview(page: Page, name = "disconnected") {
  await page.clock.install();
  await page.goto(`/?fixture=${name}`);
  await expect(page.getByTestId("connection-status")).not.toHaveText(
    "Loading app",
  );
}
async function fit(page: Page) {
  await expect(
    page.getByRole("region", { name: "Session controls" }),
  ).toBeInViewport({ ratio: 1 });
  await expect(page.getByTestId("destination")).toBeInViewport({ ratio: 1 });
  await expect(
    page.getByRole("slider", { name: /Master volume/ }),
  ).toBeInViewport({ ratio: 1 });
  const scrolling = await page.evaluate(
    () =>
      [
        ...document.querySelectorAll("main, main *"),
        document.documentElement,
        document.body,
      ].filter((node) => {
        const style = getComputedStyle(node);
        return (
          ["auto", "scroll"].includes(style.overflowY) &&
          node.scrollHeight > node.clientHeight + 1
        );
      }).length,
  );
  expect(scrolling).toBe(0);
  const geometry = await page.evaluate(() => {
    const blocks = [
      ...document.querySelectorAll(
        "main > :not(style), [data-testid='route-surface'] > section",
      ),
    ];
    const defects: string[] = [];
    for (const node of blocks) {
      const box = node.getBoundingClientRect();
      if (
        box.top < -1 ||
        box.bottom > innerHeight + 1 ||
        box.left < -1 ||
        box.right > innerWidth + 1
      )
        defects.push(`Outside viewport: ${node.className}`);
      for (const child of node.querySelectorAll(
        "h2, button, [role='slider'], [data-testid$='level'], [data-testid='queue-frames']",
      )) {
        const childBox = child.getBoundingClientRect();
        if (
          childBox.top < box.top - 1 ||
          childBox.bottom > box.bottom + 1 ||
          childBox.left < box.left - 1 ||
          childBox.right > box.right + 1
        )
          defects.push(`Clipped: ${child.textContent}`);
      }
    }
    const siblings = [...document.querySelectorAll("main > :not(style)")];
    for (let i = 1; i < siblings.length; i++) {
      if (
        (siblings[i - 1]?.getBoundingClientRect().bottom ?? 0) >
        (siblings[i]?.getBoundingClientRect().top ?? 0) + 1
      )
        defects.push("Main blocks overlap");
    }
    const regions = [
      ...document.querySelectorAll("[data-testid='route-surface'] > section"),
    ];
    for (let i = 1; i < regions.length; i++) {
      if (
        (regions[i - 1]?.getBoundingClientRect().bottom ?? 0) >
        (regions[i]?.getBoundingClientRect().top ?? 0) + 1
      )
        defects.push("Signal regions overlap");
    }
    return defects;
  });
  expect(geometry).toEqual([]);
  expect(
    await page.evaluate(
      () =>
        document.documentElement.scrollWidth <= innerWidth &&
        document.documentElement.scrollHeight <= innerHeight,
    ),
  ).toBe(true);
  await expect(
    page.getByRole("button", { name: "Mute output", exact: true }),
  ).toBeInViewport({ ratio: 1 });
}
async function stageSource(page: Page) {
  await page.getByRole("button", { name: "Change input", exact: true }).click();
  await page.getByRole("button", { name: /CABLE-B Output/ }).click();
}
async function stageRoom(page: Page) {
  await page.getByRole("button", { name: "Change room", exact: true }).click();
  await page.getByRole("button", { name: "The tavern", exact: true }).click();
}

test("connect, actual independent meters, mute, gain and stop", async ({
  page,
}) => {
  await preview(page);
  await expect(page.getByTestId("input-level")).toHaveText("No level");
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByTestId("connection-progress")).toHaveText(
    "Connecting to Discord…",
  );
  await expect(
    page.getByRole("button", { name: "Cancel", exact: true }),
  ).toBeEnabled();
  await page.clock.runFor(800);
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await expect(page.getByTestId("input-level")).toHaveText("-21.0 dBFS");
  await page.getByRole("button", { name: "Mute output" }).click();
  await page.clock.runFor(250);
  await expect(page.getByTestId("input-level")).toHaveText("-21.0 dBFS");
  await expect(page.getByTestId("mix-level")).toHaveText("Silent");
  const slider = page.getByRole("slider", { name: /Master volume/ });
  await slider.focus();
  await slider.press("ArrowLeft");
  await page.clock.runFor(200);
  await expect(slider).toHaveAttribute("aria-valuenow", "79");
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await page.clock.runFor(500);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await expect(
    page.getByRole("button", { name: "Mute output" }),
  ).toHaveAttribute("aria-pressed", "true");
});

test("room search grouped by server/category stages, discard and explicit reconnect", async ({
  page,
}) => {
  await preview(page, "connected");
  await page.keyboard.press("Control+k");
  await expect(
    page.getByRole("dialog", { name: "Discord rooms" }),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "Game night" })).toBeVisible();
  await page.getByLabel("Search servers and rooms").fill("tavern");
  await expect(page.getByRole("button", { name: /Session room/ })).toHaveCount(
    0,
  );
  await page.getByRole("button", { name: "The tavern", exact: true }).click();
  await expect(page.getByTestId("destination")).toContainText("Session room");
  await expect(
    page.getByRole("region", { name: "Queued route" }),
  ).toContainText("The tavern");
  await fit(page);
  await page.getByRole("button", { name: "Discard", exact: true }).click();
  await expect(page.getByRole("region", { name: "Queued route" })).toHaveCount(
    0,
  );
  await stageRoom(page);
  await page
    .getByRole("button", { name: "Apply & reconnect", exact: true })
    .click();
  await expect(page.getByRole("button", { name: "Mute output" })).toBeEnabled();
  await page.clock.runFor(500);
  await expect(page.getByTestId("destination")).toContainText("The tavern");
  await expect(page.getByTestId("connection-status")).toHaveText("Connecting");
});

test("disconnected apply saves selection without connecting; cancel during apply does not reconnect", async ({
  page,
}) => {
  await preview(page);
  await stageSource(page);
  await page
    .getByRole("button", { name: "Apply selection", exact: true })
    .click();
  await expect(page.getByTestId("route-surface")).toContainText(
    "CABLE-B Output",
  );
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await page.goto("/tests/harness.html");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await page.evaluate(() => {
    window.uiHarness.holdDisconnect = true;
  });
  await stageRoom(page);
  await page
    .getByRole("button", { name: "Apply & reconnect", exact: true })
    .click();
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await page.evaluate(() =>
    window.uiHarness.emit({ connection: { phase: "disconnected" } }),
  );
  await page.clock.runFor(1500);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await expect(page.getByTestId("destination")).toContainText("Session room");
  expect(
    await page.evaluate(() =>
      window.uiHarness.commands.some((command) => command.type === "connect"),
    ),
  ).toBe(false);
});

test("first-run Test & Save validates, serializes and clears token; route is never auto-selected", async ({
  page,
}) => {
  await preview(page, "first-run");
  await page.getByRole("button", { name: "Set up bot", exact: true }).click();
  const input = page.getByLabel("Bot token", { exact: true });
  await page.getByRole("button", { name: "Test & Save", exact: true }).click();
  await expect(page.getByText("Enter your bot token.")).toBeVisible();
  await input.fill("synthetic-example-only");
  await page.getByRole("button", { name: "Test & Save", exact: true }).click();
  await expect(input).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Testing & saving…" }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Mute output" }).click();
  await page.clock.runFor(600);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await expect(page.getByTestId("destination")).toContainText(
    "Choose a voice room",
  );
  await page.getByRole("button", { name: "Bot settings" }).click();
  await expect(input).toHaveValue("");
});

test("drawers preserve transport, keyboard focus, empty search and Escape return", async ({
  page,
}) => {
  await preview(page, "connected");
  const opener = page.getByRole("button", { name: "Change room", exact: true });
  await opener.focus();
  await opener.press("Enter");
  await expect(
    page.getByRole("dialog", { name: "Discord rooms" }),
  ).toHaveAttribute("aria-modal", "false");
  await page.getByLabel("Search servers and rooms").fill("no-matches");
  await expect(page.getByText("No rooms match your search.")).toBeVisible();
  await page.getByRole("button", { name: "Mute output" }).click();
  await expect(
    page.getByRole("button", { name: "Mute output" }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Mute output" })).toBeFocused();
  await opener.click();
  await page.keyboard.press("Escape");
  await page.clock.runFor(50);
  await expect(opener).toBeFocused();
  expect(
    await opener.evaluate((el) => getComputedStyle(el).outlineStyle),
  ).not.toBe("none");
  await page.getByRole("button", { name: "Change input", exact: true }).click();
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await page.clock.runFor(500);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
});

for (const width of [480, 420]) {
  test(`compact pickers preserve hierarchy, list-only scroll, focus and transport at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: width === 480 ? 600 : 540 });
    await page.goto("/tests/harness.html");
    await expect(page.getByTestId("connection-status")).toHaveText("Connected");
    await page.evaluate(() => {
      const snapshot = window.uiHarness.snapshot();
      window.uiHarness.emit({
        servers: snapshot.servers.map((server, index) => ({
          ...server,
          channels: [
            {
              id: `duplicate-${index}`,
              name: "Session room",
              categoryName: "Game night",
              position: -1,
            },
            ...server.channels,
            ...Array.from({ length: 12 }, (_, room) => ({
              id: `extra-${index}-${room}`,
              name: `Extra room ${room}`,
              categoryName: "Other rooms",
              position: room + 2,
            })),
          ],
        })),
      });
    });
    const opener = page.getByRole("button", {
      name: "Change room",
      exact: true,
    });
    await opener.click();
    const picker = page.getByRole("dialog", { name: "Discord rooms" });
    await expect(opener).toHaveAttribute("aria-expanded", "true");
    await expect(page.getByLabel("Search servers and rooms")).toBeFocused();
    await expect(picker).toHaveAttribute("aria-modal", "false");
    const metrics = await picker.evaluate((element) => {
      const box = element.getBoundingClientRect();
      const transport = document
        .querySelector('[aria-label="Session controls"]')
        ?.getBoundingClientRect();
      const scrollable = [...element.querySelectorAll("*")].filter(
        (node) =>
          ["auto", "scroll"].includes(getComputedStyle(node).overflowY) &&
          node.scrollHeight > node.clientHeight + 1,
      );
      return {
        width: box.width,
        height: box.height,
        top: box.top,
        bottom: box.bottom,
        transportTop: transport?.top ?? 0,
        scrollable: scrollable.map((node) => node.className),
      };
    });
    expect(metrics.width).toBeLessThanOrEqual(344);
    expect(metrics.height).toBeLessThanOrEqual(368);
    expect(metrics.top).toBeGreaterThanOrEqual(8);
    expect(metrics.bottom).toBeLessThan(metrics.transportTop);
    expect(metrics.scrollable).toHaveLength(1);
    expect(metrics.scrollable[0]).toContain("drawerList");
    await expect(
      page.getByRole("button", { name: "Refresh rooms" }),
    ).toBeInViewport({ ratio: 1 });
    await page.getByLabel("Search servers and rooms").fill("Session room");
    for (const server of ["The Sunday Table", "One-shot Club"]) {
      const group = picker.getByRole("region", { name: server, exact: true });
      await expect(
        group.getByRole("heading", { name: "Game night" }),
      ).toBeVisible();
      await expect(
        group.getByRole("button", { name: /Session room/ }).first(),
      ).toHaveAttribute("aria-description", `${server}, Game night`);
    }
    const room = picker
      .getByRole("region", { name: "One-shot Club", exact: true })
      .getByRole("button", { name: "Session room", exact: true });
    await room.focus();
    await room.press("Enter");
    await expect(
      page.getByRole("region", { name: "Queued route" }),
    ).toContainText("One-shot Club");
    await expect(page.getByTestId("destination")).toContainText(
      "The Sunday Table",
    );
    await expect(opener).toBeFocused();
    await opener.click();
    await expect(
      picker
        .getByRole("region", { name: "One-shot Club", exact: true })
        .getByRole("button", { name: "Session room Queued", exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    // The last choice in this server is followed by disabled refresh, then live transport.
    await picker
      .getByRole("button", { name: "Extra room 11", exact: true })
      .last()
      .focus();
    await page.keyboard.press("Tab");
    // Chromium's portal-to-document wrap can briefly focus the document itself.
    for (
      let step = 0;
      step < 2 &&
      !(await page
        .getByRole("button", { name: "Mute output" })
        .evaluate((element) => element === document.activeElement));
      step++
    )
      await page.keyboard.press("Tab");
    await expect(
      page.getByRole("button", { name: "Mute output" }),
    ).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(picker).toHaveCount(0);
    await expect(opener).toBeFocused();
    await page
      .getByRole("button", { name: "Change input", exact: true })
      .click();
    const sourcePicker = page.getByRole("dialog", { name: "Audio sources" });
    await expect(page.getByLabel("Search recording inputs")).toBeFocused();
    await expect(
      sourcePicker.getByRole("button", { name: /CABLE Output/ }),
    ).toBeVisible();
    const fullSourceHeight = await sourcePicker.evaluate(
      (element) => element.getBoundingClientRect().height,
    );
    expect(fullSourceHeight).toBeLessThanOrEqual(368);
    await page.getByLabel("Search recording inputs").fill("CABLE-B");
    const filteredSourceHeight = await sourcePicker.evaluate(
      (element) => element.getBoundingClientRect().height,
    );
    expect(filteredSourceHeight).toBeLessThan(fullSourceHeight - 40);
    await sourcePicker
      .getByRole("button", { name: "Close audio sources" })
      .click();
    await expect(
      page.getByRole("button", { name: "Change input", exact: true }),
    ).toBeFocused();
  });
}

test("queued fields show confirmed and proposed values, distinguish duplicate rooms and reconcile partial apply", async ({
  page,
}) => {
  await page.setViewportSize({ width: 420, height: 540 });
  await page.goto("/tests/harness.html");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await stageSource(page);
  const queue = page.getByRole("region", { name: "Queued route" });
  await expect(queue.locator("dt")).toHaveText(["Input"]);
  await expect(queue.locator("dd")).toHaveText(
    "Confirmed: CABLE Output (VB-Audio Virtual Cable)Queued: CABLE-B Output",
  );
  await page.getByRole("button", { name: "Discard", exact: true }).click();
  await page.evaluate(() => {
    const snapshot = window.uiHarness.snapshot();
    window.uiHarness.emit({
      servers: snapshot.servers.map((server, index) =>
        index === 1
          ? {
              ...server,
              channels: [
                {
                  id: "other-session",
                  name: "Session room",
                  categoryName: "Game night",
                  position: 0,
                },
              ],
            }
          : server,
      ),
      connection: {
        phase: "error",
        code: "permission-denied",
        message: "Synthetic room permission failure",
      },
    });
  });
  await stageSource(page);
  await page.getByRole("button", { name: "Change room", exact: true }).click();
  await page
    .getByRole("dialog")
    .getByRole("region", { name: "One-shot Club", exact: true })
    .getByRole("button", { name: "Session room", exact: true })
    .click();
  await expect(queue.locator("dt")).toHaveText(["Input", "Server", "Room"]);
  await expect(queue.locator("dd").nth(1)).toHaveText(
    "Confirmed: The Sunday TableQueued: One-shot Club",
  );
  await expect(queue.locator("dd").nth(2)).toHaveText(
    "Confirmed: Session roomQueued: Session room",
  );
  await fit(page);
  const gaps = await page.evaluate(() => {
    const endTick = [...document.querySelectorAll(".mantine-Slider-markLabel")]
      .find((element) => element.textContent === "100")
      ?.getBoundingClientRect();
    const readout = document
      .querySelector('[data-testid="mix-level"]')
      ?.getBoundingClientRect();
    const fields = [
      ...document.querySelectorAll('[aria-label="Queued route"] dl > div'),
    ].map((row) => {
      const label = row.querySelector("dt")?.getBoundingClientRect();
      const values = row.querySelector("dd")?.getBoundingClientRect();
      return (values?.left ?? 0) - (label?.right ?? 0);
    });
    return { meter: (readout?.left ?? 0) - (endTick?.right ?? 0), fields };
  });
  expect(gaps.meter).toBeGreaterThanOrEqual(4);
  expect(gaps.fields.every((gap) => gap >= 5)).toBe(true);
  await page.evaluate(() => {
    window.uiHarness.rejectDestination = true;
  });
  await page
    .getByRole("button", { name: "Apply selection", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Route change incomplete",
  );
  await expect(queue.locator("dt")).toHaveText(["Server", "Room"]);
  await expect(page.getByTestId("route-surface")).toContainText(
    "CABLE-B Output",
  );
  await fit(page);
});

test("meter targets keep exact readings and bounded attack/release with immediate null, route, mute and visibility resets", async ({
  page,
}) => {
  await page.clock.install();
  await page.goto("/tests/harness.html");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await page.clock.runFor(100);
  const inputBar = page.getByTestId("input-bar");
  const mixBar = page.getByTestId("mix-bar");
  const duration = (stage: "input" | "mix") =>
    page
      .getByTestId(`${stage}-bar`)
      .evaluate((element) => (element as HTMLElement).style.transitionDuration);
  await page.evaluate(() =>
    window.uiHarness.emit({
      capture: {
        ...window.uiHarness.snapshot().capture,
        levelDbfs: -3,
        outputLevelDbfs: -6,
      },
    }),
  );
  await page.clock.runFor(50);
  await expect(page.getByTestId("input-level")).toHaveText("-3.0 dBFS");
  await expect(
    page.getByRole("meter", { name: "Input peak before gain" }),
  ).toHaveJSProperty("value", -3);
  expect(await duration("input")).toBe("80ms");
  await page.evaluate(() =>
    window.uiHarness.emit({
      capture: {
        ...window.uiHarness.snapshot().capture,
        levelDbfs: -96,
        outputLevelDbfs: -96,
      },
    }),
  );
  await page.clock.runFor(50);
  await expect(page.getByTestId("input-level")).toHaveText("Silent");
  expect(await duration("input")).toBe("180ms");
  await page.evaluate(() =>
    window.uiHarness.emit({
      capture: {
        ...window.uiHarness.snapshot().capture,
        levelDbfs: null,
        outputLevelDbfs: null,
      },
    }),
  );
  await page.clock.runFor(50);
  expect(await duration("input")).toBe("0ms");
  await expect(inputBar).toHaveCSS("transform", "matrix(1, 0, 0, 0, 0, 0)");
  await page.evaluate(() =>
    window.uiHarness.emit({
      sessionId: 2,
      capture: {
        ...window.uiHarness.snapshot().capture,
        levelDbfs: -18,
        outputLevelDbfs: -24,
      },
    }),
  );
  await page.clock.runFor(50);
  expect(await duration("input")).toBe("0ms");
  await page.getByRole("button", { name: "Mute output" }).click();
  await page.clock.runFor(50);
  await expect(page.getByTestId("input-level")).toHaveText("-18.0 dBFS");
  await expect(page.getByTestId("mix-level")).toHaveText("Silent");
  expect(await duration("mix")).toBe("0ms");
  await expect(mixBar).toHaveCSS("transform", "matrix(1, 0, 0, 0, 0, 0)");
  await page.evaluate(() =>
    window.uiHarness.emit({
      capture: { ...window.uiHarness.snapshot().capture, levelDbfs: -6 },
    }),
  );
  await page.clock.runFor(50);
  expect(await duration("input")).toBe("80ms");
  await page.evaluate(() => window.uiHarness.visible(false));
  expect(await duration("input")).toBe("0ms");
  await page.evaluate(() =>
    window.uiHarness.emit({
      capture: { ...window.uiHarness.snapshot().capture, levelDbfs: -45 },
    }),
  );
  await page.clock.runFor(250);
  await expect(page.getByTestId("input-level")).toHaveText("-6.0 dBFS");
  await page.evaluate(() => window.uiHarness.visible(true));
  await expect(page.getByTestId("input-level")).toHaveText("-45.0 dBFS");
  expect(await duration("input")).toBe("0ms");
  // Native/browser reduced motion suppresses visual interpolation without changing text.
  expect(
    await inputBar.evaluate((element) =>
      Number.parseFloat(getComputedStyle(element).transitionDuration),
    ),
  ).toBeLessThanOrEqual(0.00001);
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(page.getByTestId("input-level")).toHaveText("No level");
  expect(await duration("input")).toBe("0ms");
});

test("CSS release moves only the visual bar between exact observations", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/tests/harness.html");
  await expect(page.getByTestId("input-level")).toHaveText("-21.0 dBFS");
  const result = await page.evaluate(async () => {
    const bar = document.querySelector('[data-testid="input-bar"]');
    if (!bar) throw new Error("Missing input bar");
    const start = new DOMMatrix(getComputedStyle(bar).transform).m22;
    window.uiHarness.emit({
      capture: { ...window.uiHarness.snapshot().capture, levelDbfs: -60 },
    });
    await new Promise<void>((resolve) => setTimeout(resolve, 55));
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
    );
    return {
      start,
      moving: new DOMMatrix(getComputedStyle(bar).transform).m22,
      text: document.querySelector('[data-testid="input-level"]')?.textContent,
      semantic: document.querySelector<HTMLMeterElement>(
        'meter[aria-label="Input peak before gain"]',
      )?.value,
    };
  });
  expect(result.moving).toBeGreaterThan(0);
  expect(result.moving).toBeLessThan(result.start);
  expect(result.text).toBe("-60.0 dBFS");
  expect(result.semantic).toBe(-60);
  await expect(page.getByTestId("input-bar")).toHaveCSS(
    "transform",
    "matrix(1, 0, 0, 0, 0, 0)",
  );
});

test("partial apply preserves confirmed input and remaining room draft with truthful details", async ({
  page,
}) => {
  await page.clock.install();
  await page.goto("/tests/harness.html");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await stageSource(page);
  await stageRoom(page);
  await page.evaluate(() => {
    window.uiHarness.rejectDestination = true;
  });
  await page
    .getByRole("button", { name: "Apply & reconnect", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Route change incomplete",
  );
  await expect(page.getByTestId("route-surface")).toContainText(
    "CABLE-B Output",
  );
  await expect(page.getByTestId("destination")).toContainText("Session room");
  await expect(
    page.getByRole("region", { name: "Queued route" }),
  ).toContainText("The tavern");
  await fit(page);
  const commands = await page.evaluate(() => window.uiHarness.commands);
  expect(commands.map((command) => command.type)).toEqual([
    "disconnect",
    "select-source",
    "select-destination",
  ]);
  await page.getByRole("button", { name: "Details", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText(
    "Room change failed. Check permissions.",
  );
});

for (const width of [480, 420]) {
  test(`all main failure and pending states fit at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: width === 480 ? 600 : 540 });
    for (const name of fixtureNames) {
      await preview(page, name);
      await fit(page);
    }
    await preview(page, "connection-error");
    await stageSource(page);
    await stageRoom(page);
    await fit(page);
  });
}

test("render bursts leave Profiler control commits unchanged, cap visual paints and resume latest after native/document hide", async ({
  page,
}) => {
  await page.clock.install();
  await page.goto("/tests/harness.html");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await page.clock.runFor(250);
  const baseline = await page.evaluate(() => window.uiHarness.commits);
  await page.evaluate(() => window.uiHarness.burst(1000));
  await page.clock.runFor(250);
  expect(await page.evaluate(() => window.uiHarness.commits)).toBe(baseline);
  await expect(page.getByTestId("input-level")).toHaveText("-21.0 dBFS");
  await expect(page.getByTestId("mix-level")).toHaveText("-26.0 dBFS");
  await expect(page.getByTestId("queue-frames")).toContainText("999/1,920");
  const count = await page.evaluate(() => {
    const readout = document.querySelector('[data-testid="input-level"]');
    const tracked = window as unknown as {
      paintCount: number;
      observer: MutationObserver;
    };
    tracked.paintCount = 0;
    tracked.observer = new MutationObserver((records) => {
      tracked.paintCount += records.filter(
        (r) => r.type === "childList",
      ).length;
    });
    if (readout) tracked.observer.observe(readout, { childList: true });
    return 0;
  });
  expect(count).toBe(0);
  await page.evaluate(() => {
    const interval = setInterval(() => window.uiHarness.burst(10), 5);
    setTimeout(() => clearInterval(interval), 1000);
  });
  await page.clock.runFor(1000);
  const paints = await page.evaluate(
    () => (window as unknown as { paintCount: number }).paintCount,
  );
  await test.info().attach("telemetry-proof.json", {
    body: JSON.stringify({
      initialBurst: 1000,
      baselineControlCommits: baseline,
      afterBurstControlCommits: await page.evaluate(
        () => window.uiHarness.commits,
      ),
      visualPaintsPerSimulatedSecond: paints,
      cadenceLimit: 20,
      nativeAndDocumentHiddenResume: "asserted below",
    }),
    contentType: "application/json",
  });
  expect(paints).toBeLessThanOrEqual(21);
  expect(paints).toBeGreaterThan(10);
  expect(await page.evaluate(() => window.uiHarness.commits)).toBe(baseline);
  await page.evaluate(() => {
    window.uiHarness.visible(false);
    window.uiHarness.emit({
      capture: {
        ...window.uiHarness.snapshot().capture,
        levelDbfs: -9,
        outputLevelDbfs: -70,
      },
    });
  });
  const beforeHidden = await page.getByTestId("input-level").textContent();
  await page.clock.runFor(1000);
  await expect(page.getByTestId("input-level")).toHaveText(beforeHidden ?? "");
  await page.evaluate(() => window.uiHarness.visible(true));
  await page.clock.runFor(250);
  await expect(page.getByTestId("input-level")).toHaveText("-9.0 dBFS");
  await expect(page.getByTestId("mix-level")).toHaveText("-70.0 dBFS");
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", {
      configurable: true,
      value: true,
    });
    document.dispatchEvent(new Event("visibilitychange"));
    window.uiHarness.emit({
      capture: {
        ...window.uiHarness.snapshot().capture,
        levelDbfs: -5,
        outputLevelDbfs: null,
      },
    });
  });
  await page.clock.runFor(1000);
  await expect(page.getByTestId("input-level")).toHaveText("-9.0 dBFS");
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", {
      configurable: true,
      value: false,
    });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.clock.runFor(250);
  await expect(page.getByTestId("input-level")).toHaveText("-5.0 dBFS");
  await expect(page.getByTestId("mix-level")).toHaveText("No level");
  expect(await page.evaluate(() => window.uiHarness.commits)).toBe(baseline);
  await page.getByRole("button", { name: "Mute output" }).click();
  await page.clock.runFor(100);
  expect(await page.evaluate(() => window.uiHarness.commits)).toBeGreaterThan(
    baseline,
  );
});

test("text scaling and DPR keep main controls within supported surface", async ({
  browser,
}) => {
  const context = await browser.newContext({
    viewport: { width: 420, height: 540 },
    deviceScaleFactor: 1.5,
  });
  const page = await context.newPage();
  await preview(page, "connected");
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "18px";
  });
  await stageSource(page);
  await stageRoom(page);
  await fit(page);
  await context.close();
});

test("bot save failure stays truthful and credential entry clears when closed", async ({
  page,
}) => {
  await page.clock.install();
  await page.goto("/tests/harness.html");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await page.evaluate(() => {
    window.uiHarness.emit({ connection: { phase: "disconnected" } });
    window.uiHarness.rejectSave = true;
  });
  await page.getByRole("button", { name: "Bot settings", exact: true }).click();
  await page
    .getByLabel("Bot token", { exact: true })
    .fill("synthetic-private-token");
  await page.getByRole("button", { name: "Test & Save", exact: true }).click();
  await page.clock.runFor(400);
  await expect(page.getByRole("dialog")).toContainText(
    "Could not confirm this bot token was saved",
  );
  await expect(page.getByRole("alert")).not.toContainText(
    "synthetic-private-token",
  );
  await expect(
    page.getByRole("button", { name: "Test & Save", exact: true }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Close bot settings" }).click();
  await page.getByRole("button", { name: "Bot settings", exact: true }).click();
  await expect(page.getByLabel("Bot token", { exact: true })).toHaveValue("");
});

test("catalog icons use actual metadata with fallback; cleanup and missing input preserve truthful controls", async ({
  page,
}) => {
  await preview(page, "cleanup-incomplete");
  await expect(
    page.getByRole("button", { name: "Retry stop", exact: true }),
  ).toBeEnabled();
  await expect(
    page.getByRole("button", { name: "Change input", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Change room", exact: true }),
  ).toBeDisabled();
  await preview(page, "missing-device");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await expect(page.getByTestId("capture-status")).toHaveText("Device missing");
  await expect(page.getByTestId("input-level")).toHaveText("No level");
  await page.goto("/tests/harness.html");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  const icon =
    "data:image/svg+xml," +
    encodeURIComponent(
      '<svg xmlns="http://www.w3.org/2000/svg" width="28" height="28"><rect width="28" height="28" fill="#9bd9c7"/></svg>',
    );
  await page.evaluate(
    (iconUrl) =>
      window.uiHarness.emit({
        servers: window.uiHarness
          .snapshot()
          .servers.map((server, index) =>
            index === 0 ? { ...server, iconUrl } : server,
          ),
      }),
    icon,
  );
  await expect(page.getByTestId("destination").locator("img")).toHaveAttribute(
    "src",
    icon,
  );
  await page.getByRole("button", { name: "Change room", exact: true }).click();
  await expect(page.getByRole("dialog").locator("img")).toHaveAttribute(
    "src",
    icon,
  );
});
