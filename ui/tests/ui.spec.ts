import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { expect, type Page, test } from "@playwright/test";
import { fixtureNames } from "../src/fixtures";

const screenshots = fileURLToPath(
  new URL("../../.trash/ec011/screenshots/", import.meta.url),
);

async function preview(page: Page, name = "disconnected") {
  await page.clock.install();
  await page.goto(`/?fixture=${name}`);
}

async function capture(page: Page, name: string) {
  await mkdir(screenshots, { recursive: true });
  await page.screenshot({
    path: `${screenshots}${name}.png`,
    animations: "disabled",
  });
}

async function persistentControls(page: Page) {
  const controls = page.getByRole("region", { name: "Session controls" });
  await expect(controls).toBeInViewport({ ratio: 1 });
  await expect(
    page.getByRole("button", { name: "Mute output", exact: true }),
  ).toBeInViewport({ ratio: 1 });
  await expect(
    page.getByText("Discord volume", { exact: true }),
  ).toBeInViewport({ ratio: 1 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
}

test("startup is stopped; connect, pre-output level, and stable mute remain independent", async ({
  page,
}) => {
  await preview(page);
  await expect(page.getByLabel("Fixture mode")).toContainText(
    "no audio or Discord connection",
  );
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await expect(page.getByTestId("capture-status")).toHaveText(
    "Capture stopped",
  );
  await expect(page.getByTestId("input-level")).toHaveText("No level");
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByTestId("connection-status")).toHaveText("Connecting");
  await expect(page.getByTestId("connection-progress")).toHaveText(
    "Connecting to Discord…",
  );
  await expect(page.getByTestId("capture-status")).toHaveText("Opening input");
  await page.clock.runFor(800);
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await expect(page.getByTestId("input-level")).toHaveText("-21 dBFS");
  await page.getByRole("button", { name: "Mute output", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Mute output", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await expect(page.getByTestId("input-level")).toHaveText("-21 dBFS");
  await expect(page.getByTestId("output-status")).toHaveText("Muted");
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnecting",
  );
  await expect(
    page.getByRole("button", { name: "Disconnect", exact: true }),
  ).toBeDisabled();
  await page.clock.runFor(500);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await expect(
    page.getByRole("button", { name: "Mute output", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
});

test("source and destination are explicit; changing servers clears channel and session locks route", async ({
  page,
}) => {
  await preview(page);
  await page
    .getByLabel("Recording device", { exact: true })
    .selectOption("cable-b");
  await page
    .getByLabel("Server", { exact: true })
    .selectOption("1234567890123456790");
  await expect(page.getByLabel("Voice channel", { exact: true })).toHaveValue(
    "",
  );
  await expect(
    page.getByRole("button", { name: "Connect", exact: true }),
  ).toBeDisabled();
  await page
    .getByLabel("Voice channel", { exact: true })
    .selectOption("9876543210987654323");
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(
    page.getByLabel("Recording device", { exact: true }),
  ).toBeDisabled();
  await expect(page.getByLabel("Server", { exact: true })).toBeDisabled();
  await expect(page.getByLabel("Voice channel", { exact: true })).toHaveValue(
    "9876543210987654323",
  );
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await page.clock.runFor(5000);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
});

test("first run has actionable configuration/discovery and never auto-selects a route", async ({
  page,
}) => {
  await preview(page, "first-run");
  await expect(
    page.getByRole("button", { name: "Connect", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Bot settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Bot settings" });
  await expect(dialog.locator("input, textarea")).toHaveCount(0);
  await page
    .getByRole("button", { name: "Recheck configuration", exact: true })
    .click();
  await expect(dialog).toContainText("Checking bot configuration");
  await page.clock.runFor(600);
  await page
    .getByRole("button", { name: "Refresh servers", exact: true })
    .click();
  await page.clock.runFor(600);
  await page.getByRole("button", { name: "Close bot settings" }).click();
  await expect(
    page.getByLabel("Recording device", { exact: true }),
  ).toHaveValue("");
  await expect(page.getByLabel("Server", { exact: true })).toHaveValue("");
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await expect(page.getByTestId("readiness")).toHaveText(
    "Select a recording device.",
  );
});

test("connected capture failure is visible without replacing connection or claiming sending", async ({
  page,
}) => {
  await preview(page, "missing-device");
  await expect(page.getByTestId("connection-status")).toHaveText("Connected");
  await expect(page.getByTestId("capture-status")).toHaveText("Device missing");
  await expect(page.getByTestId("input-level")).toHaveText("No level");
  await expect(
    page.getByLabel("Recording device", { exact: true }),
  ).toHaveValue("cable");
  await expect(
    page.getByLabel("Recording device", { exact: true }),
  ).toBeDisabled();
  await expect(page.getByTestId("output-status")).toHaveText("Output enabled");
  await expect(page.getByText("Sending", { exact: true })).toHaveCount(0);
});

test("rejected commands surface without optimistic connection or mute changes", async ({
  page,
}) => {
  await preview(page, "rejected-command");
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("denied permission");
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await expect(page.getByTestId("capture-status")).toHaveText(
    "Capture stopped",
  );
  await page.getByRole("button", { name: "Dismiss error" }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("keyboard focus stays valid in a connected modal with disabled actions, then returns to trigger", async ({
  page,
}) => {
  await preview(page, "connected");
  await page.keyboard.press("Tab");
  const settings = page.getByRole("button", {
    name: "Bot settings",
    exact: true,
  });
  await expect(settings).toBeFocused();
  expect(
    await settings.evaluate(
      (element) => getComputedStyle(element).outlineStyle,
    ),
  ).not.toBe("none");
  await page.keyboard.press("Enter");
  const close = page.getByRole("button", { name: "Close bot settings" });
  await expect(close).toBeFocused();
  await expect(
    page.getByRole("button", { name: "Recheck configuration", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Refresh servers", exact: true }),
  ).toBeDisabled();
  await page.keyboard.press("Tab");
  await expect(close).toBeFocused();
  await page.keyboard.press("Escape");
  await page.clock.runFor(200);
  await expect(settings).toBeFocused();
  const volume = page.getByRole("slider", { name: "Discord volume" });
  await volume.focus();
  await volume.press("ArrowLeft");
  await page.clock.runFor(200);
  await expect(volume).toHaveAttribute("aria-valuenow", "79");
  const mute = page.getByRole("button", { name: "Mute output", exact: true });
  await mute.focus();
  await mute.press("Space");
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  await persistentControls(page);
});

test("cleanup incomplete prevents connect and route changes while settings remain readable", async ({
  page,
}) => {
  await preview(page, "cleanup-incomplete");
  await expect(
    page.getByRole("button", { name: "Connect", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Retry disconnect" }),
  ).toBeEnabled();
  await expect(
    page.getByLabel("Recording device", { exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Bot settings", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "Bot settings" }),
  ).toContainText("Settings remain readable");
  await expect(
    page.getByRole("button", { name: "Close bot settings" }),
  ).toBeFocused();
});

function luminance(color: string) {
  const channels = (color.match(/[\d.]+/g) ?? []).slice(0, 3).map((value) => {
    const channel = Number(value) / 255;
    return channel <= 0.04045
      ? channel / 12.92
      : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return (
    (channels[0] ?? 0) * 0.2126 +
    (channels[1] ?? 0) * 0.7152 +
    (channels[2] ?? 0) * 0.0722
  );
}

function contrast(foreground: string, background: string) {
  const first = luminance(foreground),
    second = luminance(background);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}

test("accent, hover, pressed mute, disabled controls and selected locked text remain legible", async ({
  page,
}) => {
  await preview(page);
  const connect = page.getByRole("button", { name: "Connect", exact: true });
  const measure = (element: Element) => ({
    color: getComputedStyle(element).color,
    background: getComputedStyle(element).backgroundColor,
  });
  let colors = await connect.evaluate(measure);
  expect(colors.color).toBe("rgb(24, 24, 24)");
  expect(contrast(colors.color, colors.background)).toBeGreaterThan(4.5);
  await connect.hover();
  colors = await connect.evaluate(measure);
  expect(contrast(colors.color, colors.background)).toBeGreaterThan(4.5);
  await page.getByRole("button", { name: "Mute output", exact: true }).click();
  colors = await page
    .getByRole("button", { name: "Mute output", exact: true })
    .evaluate(measure);
  expect(contrast(colors.color, colors.background)).toBeGreaterThan(4.5);
  await connect.click();
  colors = await page
    .getByLabel("Recording device", { exact: true })
    .evaluate(measure);
  expect(contrast(colors.color, colors.background)).toBeGreaterThan(4.5);
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  colors = await page
    .getByRole("button", { name: "Disconnect", exact: true })
    .evaluate(measure);
  expect(contrast(colors.color, colors.background)).toBeGreaterThan(4.5);
  await capture(page, "contrast-focus-480x600");
});

for (const name of fixtureNames) {
  test(`inspected evidence fixture: ${name} at 480x600`, async ({ page }) => {
    await preview(page, name);
    await expect(page.getByTestId("connection-status")).not.toHaveText(
      "Loading app",
    );
    await persistentControls(page);
    await capture(page, `${name}-480x600`);
  });
}

for (const size of [
  { width: 420, height: 540, scale: 1.25 },
  { width: 480, height: 600, scale: 1.5 },
  { width: 360, height: 500, scale: 2 },
]) {
  test(`persistent controls at ${size.width}x${size.height} DPR ${size.scale}`, async ({
    browser,
  }) => {
    const context = await browser.newContext({
      viewport: { width: size.width, height: size.height },
      deviceScaleFactor: size.scale,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    for (const name of [
      "disconnected",
      "muted",
      "cleanup-incomplete",
      "first-run",
    ]) {
      await page.goto(`http://127.0.0.1:4175/?fixture=${name}`);
      await expect(page.getByTestId("connection-status")).not.toHaveText(
        "Loading app",
      );
      await persistentControls(page);
      await page.getByText("Routing help", { exact: true }).click();
      await page.getByText("Fixture states", { exact: true }).click();
      await page.getByLabel("Fixture state").scrollIntoViewIfNeeded();
      await persistentControls(page);
      await capture(
        page,
        `${name}-${size.width}x${size.height}-dpr-${size.scale}`,
      );
    }
    await page
      .getByRole("button", { name: "Bot settings", exact: true })
      .click();
    await expect(
      page.getByRole("button", { name: "Close bot settings" }),
    ).toBeFocused();
    await capture(page, `bot-settings-${size.width}-dpr-${size.scale}`);
    await context.close();
  });
}

test("CSS zoom preserves the footer and all actions without horizontal overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 1200 });
  await preview(page, "muted");
  await page.evaluate(() => {
    document.documentElement.style.zoom = "2";
  });
  await persistentControls(page);
  await capture(page, "css-zoom-200-percent");
});

test("fixture preview has no service requests, capture permissions, or page errors", async ({
  page,
}) => {
  const errors: string[] = [],
    services: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (new URL(request.url()).hostname !== "127.0.0.1")
      services.push(request.url());
  });
  await preview(page, "connected");
  await page.getByRole("button", { name: "Mute output", exact: true }).click();
  await page.getByRole("button", { name: "Bot settings", exact: true }).click();
  await expect(
    page
      .getByRole("dialog", { name: "Bot settings" })
      .locator("input, textarea"),
  ).toHaveCount(0);
  expect(errors).toEqual([]);
  expect(services).toEqual([]);
});
