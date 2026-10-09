import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { expect, type Page, test } from "@playwright/test";
import { previews } from "../src/mockAdapter";

const screenshots = fileURLToPath(
  new URL("../../../.trash/control-ui/screenshots/", import.meta.url),
);

async function openDemo(page: Page) {
  if ((await page.locator("details").getAttribute("open")) === null) {
    await page.getByText("Demo controls", { exact: true }).click();
  }
}

async function connect(page: Page) {
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByTestId("connection-status")).toHaveText("Connecting");
  await page.clock.runFor(1300);
}

test.beforeEach(async ({ page }) => {
  await page.clock.install();
  await page.goto("/");
});

test("fresh launch is disconnected with separate simulated input and delivery; route locks during a session", async ({
  page,
}) => {
  await expect(page.getByLabel("Prototype notice")).toContainText(
    "Mock adapter · no audio or Discord connection",
  );
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await expect(page.getByTestId("input-level-value")).toHaveText("-22 dBFS");
  await expect(page.getByTestId("delivery-status")).toHaveText("Not sending");
  await connect(page);
  await expect(page.getByTestId("connection-status")).toHaveText("Streaming");
  await expect(
    page.getByLabel("Recording source", { exact: true }),
  ).toBeDisabled();
  await expect(page.getByLabel("Server", { exact: true })).toBeDisabled();
  await expect(
    page.getByLabel("Voice channel", { exact: true }),
  ).toBeDisabled();
  await expect(page.getByTestId("delivery-status")).toHaveText(
    "Sending · simulated",
  );
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(
    page.getByLabel("Recording source", { exact: true }),
  ).toBeEnabled();
  await expect(page.getByTestId("delivery-status")).toHaveText("Not sending");
});

test("server changes require an explicit channel; only the selected source and destination are used", async ({
  page,
}) => {
  await page
    .getByLabel("Recording source", { exact: true })
    .selectOption("cable-b");
  await page.getByLabel("Server", { exact: true }).selectOption("oneshot");
  await expect(page.getByLabel("Voice channel", { exact: true })).toHaveValue(
    "",
  );
  await expect(
    page.getByRole("button", { name: "Connect", exact: true }),
  ).toBeDisabled();
  await page
    .getByLabel("Voice channel", { exact: true })
    .selectOption("tavern");
  await connect(page);
  await expect(page.getByTestId("connection-status")).toHaveText("Streaming");
  await expect(
    page.getByLabel("Recording source", { exact: true }),
  ).toHaveValue("cable-b");
  await expect(page.getByLabel("Voice channel", { exact: true })).toHaveValue(
    "tavern",
  );
});

test("keyboard mute and volume survive network recovery and an explicit reconnect", async ({
  page,
}) => {
  const slider = page.getByRole("slider", { name: "Output volume" });
  await slider.focus();
  await slider.press("ArrowLeft");
  await slider.press("ArrowLeft");
  await expect(slider).toHaveAttribute("aria-valuenow", "78");
  const mute = page.getByRole("button", { name: "Mute output" });
  await mute.focus();
  await mute.press("Space");
  await expect(
    page.getByRole("button", { name: "Unmute output" }),
  ).toHaveAttribute("aria-pressed", "true");
  await connect(page);
  await expect(page.getByTestId("connection-status")).toHaveText("Muted");
  await expect(page.getByTestId("input-level-value")).toHaveText("-22 dBFS");
  await expect(page.getByTestId("delivery-status")).toHaveText("Muted");
  await openDemo(page);
  await page.getByRole("button", { name: "Drop demo connection" }).click();
  await expect(page.getByTestId("connection-status")).toHaveText("Recovering");
  await expect(
    page.getByRole("button", { name: "Unmute output" }),
  ).toHaveAttribute("aria-pressed", "true");
  await page.clock.runFor(1900);
  await expect(page.getByTestId("connection-status")).toHaveText("Muted");
  await expect(slider).toHaveAttribute("aria-valuenow", "78");
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await connect(page);
  await expect(page.getByTestId("connection-status")).toHaveText("Muted");
});

test("silence remains a connected session and zero volume never changes the input meter", async ({
  page,
}) => {
  await connect(page);
  await openDemo(page);
  await page.getByRole("switch", { name: "Silent demo input" }).check();
  await page.clock.runFor(10_000);
  await expect(page.getByTestId("connection-status")).toHaveText("Streaming");
  await expect(page.getByTestId("input-level-value")).toHaveText("Silent");
  await expect(page.getByTestId("delivery-status")).toHaveText("Silent input");
  await page.getByRole("switch", { name: "Silent demo input" }).uncheck();
  const slider = page.getByRole("slider", { name: "Output volume" });
  await slider.focus();
  for (let index = 0; index < 80; index++) await slider.press("ArrowLeft");
  await expect(page.getByTestId("delivery-status")).toHaveText(
    "Volume is zero",
  );
  await expect(page.getByTestId("input-level-value")).toHaveText("-22 dBFS");
});

test("disconnect cancels both a pending connection and network recovery", async ({
  page,
}) => {
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await page.clock.runFor(5000);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await connect(page);
  await openDemo(page);
  await page.getByRole("button", { name: "Drop demo connection" }).click();
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await page.clock.runFor(5000);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
});

test("device loss preserves the exact source and mute; device return after disconnect cannot reconnect", async ({
  page,
}) => {
  await page.getByRole("button", { name: "Mute output" }).click();
  await connect(page);
  await openDemo(page);
  await page.getByRole("button", { name: "Remove demo device" }).click();
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Device missing",
  );
  await expect(page.getByTestId("input-level-value")).toHaveText("Unavailable");
  await expect(
    page.getByLabel("Recording source", { exact: true }),
  ).toHaveValue("cable");
  await page.getByRole("button", { name: "Return demo device" }).click();
  await page.clock.runFor(1300);
  await expect(page.getByTestId("connection-status")).toHaveText("Muted");
  await page.getByRole("button", { name: "Remove demo device" }).click();
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Connect", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Return demo device" }).click();
  await page.clock.runFor(5000);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await expect(
    page.getByRole("button", { name: "Unmute output" }),
  ).toHaveAttribute("aria-pressed", "true");
});

test("bot setup accepts fixed demo profiles only; permission errors require user action and reload forgets settings", async ({
  page,
}) => {
  await page.getByRole("button", { name: "Bot setup", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Bot setup · demo only" });
  await expect(dialog).toContainText(
    "cannot accept or store a Discord bot token",
  );
  await expect(dialog.locator("input, textarea")).toHaveCount(0);
  await page
    .getByLabel("Demo bot profile", { exact: true })
    .selectOption("unconfigured");
  await page.getByRole("button", { name: "Use demo setup" }).click();
  await expect(
    page.getByRole("button", { name: "Set up bot", exact: true }),
  ).toBeEnabled();
  await openDemo(page);
  await page.getByLabel("State preview", { exact: true }).selectOption("error");
  await page.clock.runFor(60_000);
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Connection error",
  );
  await page.getByRole("button", { name: "Set up bot", exact: true }).click();
  await page
    .getByLabel("Demo bot profile", { exact: true })
    .selectOption("ready");
  await page.getByRole("button", { name: "Use demo setup" }).click();
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
  await page.getByRole("button", { name: "Mute output" }).click();
  await page.reload();
  await expect(
    page.getByRole("button", { name: "Mute output" }),
  ).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByTestId("connection-status")).toHaveText(
    "Disconnected",
  );
});

test("tab order, visible focus, slider arrows, and modal escape/return focus work", async ({
  page,
}) => {
  await page.keyboard.press("Tab");
  const setup = page.getByRole("button", { name: "Bot setup", exact: true });
  await expect(setup).toBeFocused();
  expect(
    await setup.evaluate((element) => getComputedStyle(element).outlineStyle),
  ).not.toBe("none");
  await page.keyboard.press("Enter");
  await page.clock.runFor(300);
  await expect(
    page.getByLabel("Demo bot profile", { exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(
    page.getByRole("button", { name: "Close bot setup" }),
  ).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(
    page.getByRole("button", { name: "Use demo setup" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await page.clock.runFor(300);
  await expect(setup).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    page.getByLabel("Recording source", { exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByLabel("Server", { exact: true })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByLabel("Voice channel", { exact: true })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    page.getByRole("button", { name: "Connect", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    page.getByRole("slider", { name: "Output volume" }),
  ).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "Mute output" })).toBeFocused();
});

const expectedStatuses = {
  disconnected: "Disconnected",
  connecting: "Connecting",
  streaming: "Streaming",
  muted: "Muted",
  recovering: "Recovering",
  "missing-device": "Device missing",
  error: "Connection error",
};

for (const preview of previews) {
  test(`visual evidence: ${preview} at compact desktop size`, async ({
    page,
  }) => {
    await openDemo(page);
    await page
      .getByLabel("State preview", { exact: true })
      .selectOption(preview);
    await page.getByText("Demo controls", { exact: true }).click();
    await page.clock.runFor(5000);
    await expect(page.getByTestId("connection-status")).toHaveText(
      expectedStatuses[preview],
    );
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    expect(
      await page
        .locator("main")
        .evaluate((element) => element.getBoundingClientRect().height),
    ).toBeLessThanOrEqual(760);
    await mkdir(screenshots, { recursive: true });
    await page.screenshot({
      path: `${screenshots}${preview}-720x760.png`,
      fullPage: true,
      animations: "disabled",
    });
  });
}

for (const size of [
  { width: 640, height: 720, scale: 1.25 },
  { width: 480, height: 640, scale: 1.5 },
  { width: 360, height: 640, scale: 2 },
]) {
  test(`scaling evidence: ${size.width}x${size.height} at DPR ${size.scale}`, async ({
    browser,
  }) => {
    const context = await browser.newContext({
      viewport: size,
      deviceScaleFactor: size.scale,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    await page.goto("/");
    await expect(
      page.getByRole("button", { name: "Connect", exact: true }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    if (size.width === 640)
      expect(
        await page
          .locator("main")
          .evaluate((element) => element.getBoundingClientRect().height),
      ).toBeLessThanOrEqual(size.height);
    await page
      .getByRole("button", { name: "Mute output" })
      .scrollIntoViewIfNeeded();
    await expect(
      page.getByRole("button", { name: "Mute output" }),
    ).toBeInViewport();
    await mkdir(screenshots, { recursive: true });
    await page.screenshot({
      path: `${screenshots}compact-${size.width}x${size.height}-dpr-${size.scale}.png`,
      fullPage: true,
      animations: "disabled",
    });
    await page.getByRole("button", { name: "Bot setup", exact: true }).click();
    await expect(
      page.getByRole("dialog", { name: "Bot setup · demo only" }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: `${screenshots}bot-setup-${size.width}-dpr-${size.scale}.png`,
      fullPage: true,
      animations: "disabled",
    });
    await context.close();
  });
}

test("200 percent CSS zoom keeps controls reachable without horizontal scrolling", async ({
  page,
}) => {
  await page.evaluate(() => {
    document.documentElement.style.zoom = "2";
  });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page
    .getByRole("button", { name: "Mute output" })
    .scrollIntoViewIfNeeded();
  await expect(
    page.getByRole("button", { name: "Mute output" }),
  ).toBeInViewport();
  await page.getByRole("button", { name: "Mute output" }).click();
  await expect(
    page.getByRole("button", { name: "Unmute output" }),
  ).toHaveAttribute("aria-pressed", "true");
  await mkdir(screenshots, { recursive: true });
  await page.screenshot({
    path: `${screenshots}css-zoom-200-percent.png`,
    fullPage: true,
    animations: "disabled",
  });
});

test("browser has no runtime errors or outbound service requests", async ({
  page,
}) => {
  const errors: string[] = [];
  const requests: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (new URL(request.url()).hostname !== "127.0.0.1")
      requests.push(request.url());
  });
  await page.reload();
  await connect(page);
  await page.getByRole("button", { name: "Bot setup", exact: true }).click();
  await expect(
    page.getByLabel("Demo bot profile", { exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Use demo setup" }),
  ).toBeDisabled();
  expect(errors).toEqual([]);
  expect(requests).toEqual([]);
});
