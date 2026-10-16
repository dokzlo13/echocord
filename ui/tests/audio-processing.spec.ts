import { expect, type Page, test } from "@playwright/test";
import type { TimingObservation } from "../src/generated";

type Stage = Partial<Record<keyof TimingObservation, unknown>>;
interface Publish {
  /** Age of both origins unless overridden. */
  ageMs?: number;
  stateAgeMs?: number;
  audioAgeMs?: number;
  limiterReductionDb?: number | null;
  sessionId?: number;
  droppedFrames?: number;
  underrunFrames?: number;
}

/** Stops the fixture simulation and publishes exact (possibly malformed) contract values. */
async function publish(
  page: Page,
  stages: Record<string, Stage | null>,
  options: Publish = {},
): Promise<number> {
  return page.evaluate(
    ([stages, options]) => {
      const t = window.uiHarness.transport;
      t.stopTelemetry();
      const now = performance.now();
      const state = now - (options.stateAgeMs ?? options.ageMs ?? 0);
      const audio = now - (options.audioAgeMs ?? options.ageMs ?? 0);
      const timing = (key: string) => {
        const stage = stages[key];
        if (!stage) return undefined;
        return {
          epoch: "fixture:1",
          phase: "running",
          appliedAdjustmentPpm: 12.4,
          queuedFrames: 720,
          targetFrames: 720,
          capacityFrames: 4096,
          gapOutputFrames: 0,
          droppedInputFrames: 0,
          resyncCount: 0,
          lastResetReason: null,
          lastResetAtMs: null,
          stateObservedAtMs: state,
          audioObservedAtMs: audio,
          ...stage,
        } as TimingObservation;
      };
      const value = t.current.telemetry;
      t.observeTelemetry(
        {
          ...value,
          limiterReductionDb:
            options.limiterReductionDb === undefined
              ? value.limiterReductionDb
              : options.limiterReductionDb,
          inputs: value.inputs.map(({ timing: _, ...input }) => {
            const next = timing(`src:${input.stripId}`);
            return next ? { ...input, timing: next } : input;
          }),
          destinations: value.destinations.map(({ timing: _, ...output }) => {
            const next = timing(`dst:${output.destinationId}`);
            return {
              ...output,
              sessionId: options.sessionId ?? output.sessionId,
              droppedFrames: options.droppedFrames ?? output.droppedFrames,
              underrunFrames: options.underrunFrames ?? output.underrunFrames,
              ...(next ? { timing: next } : {}),
            };
          }),
        },
        now,
      );
      return now;
    },
    [stages, options] as const,
  );
}

async function openSettings(page: Page) {
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.locator(".set-advanced")).toBeVisible();
}
async function openSyncDetails(page: Page) {
  await openSettings(page);
  await page.locator(".set-summary").click();
  await page.getByRole("button", { name: /Sync details/ }).click();
  await expect(
    page.getByRole("dialog", { name: "Sync details" }),
  ).toBeVisible();
}
const row = (page: Page, name: string) =>
  page.getByRole("listitem").filter({
    has: page.getByRole("heading", { name: new RegExp(`^${name}`) }),
  });
const isOpen = (page: Page) =>
  page.locator(".set-advanced").evaluate((d: HTMLDetailsElement) => d.open);

test.beforeEach(async ({ page }) => {
  await page.goto("/tests/harness.html?fixture=audio-processing");
  await expect(page.locator(".source-card")).toHaveCount(3);
});

test("Audio processing starts collapsed, states both values and changes one field at a time", async ({
  page,
}) => {
  await openSettings(page);
  const summary = page.locator(".set-summary");
  expect(await isOpen(page)).toBe(false);
  await expect(summary).toContainText(
    "Sync correction on · Limit loud peaks off",
  );
  await summary.focus();
  await page.keyboard.press("Enter");
  expect(await isOpen(page)).toBe(true);
  const settingsText = await page.getByRole("dialog").textContent();
  expect(settingsText).not.toMatch(/\bppm\b|\bdB\b|\d\s?ms\b|stay connected/);
  const before = await page.evaluate(
    () => window.uiHarness.transport.current.settings,
  );
  const sync = page.getByRole("switch", { name: /Sync correction/ });
  await sync.focus();
  await page.keyboard.press("Space");
  const sent = await page.evaluate(() =>
    window.uiHarness.transport.commands.filter(
      (c) => c.type === "set-settings",
    ),
  );
  expect(sent.at(-1)).toEqual({
    type: "set-settings",
    settings: { ...before, driftCorrection: false },
  });
  await expect(summary).toContainText(
    "Sync correction off · Limit loud peaks off",
  );
  await expect(sync).toBeFocused();
  await expect(sync).toHaveAccessibleDescription(
    "Changing these may briefly interrupt the sound.",
  );
  await expect(page.locator(".set-advanced")).toContainText(
    "May also lower loud audio without an overload warning.",
  );
});

test("Sync details returns focus to its row with the section open, then to the gear", async ({
  page,
}) => {
  await openSyncDetails(page);
  await expect(
    page.getByRole("button", { name: "Back", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.locator(".sync-open")).toBeFocused();
  expect(await isOpen(page)).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "Settings" })).toBeFocused();
});

test("absent timing omits a row while Off and unmeasured stay distinct", async ({
  page,
}) => {
  await openSyncDetails(page);
  await publish(page, {
    "src:strip:1": {},
    "src:strip:2": {
      phase: "off",
      appliedAdjustmentPpm: null,
      targetFrames: null,
    },
    "src:strip:3": {
      phase: "priming",
      appliedAdjustmentPpm: null,
      queuedFrames: 240,
    },
  });
  await expect(page.locator(".sync-row")).toHaveCount(3);
  await expect(page.getByText("Playback devices")).toHaveCount(0);
  await expect(page.locator(".sync-columns")).not.toContainText(
    "Session room",
    { useInnerText: true },
  );
  await expect(row(page, "Preview player").locator(".sync-tag")).toHaveText(
    "Running",
  );
  await expect(row(page, "Preview player")).toContainText(
    "Reads input slightly faster (+12.4 ppm)",
  );
  const off = row(page, "Preview browser");
  await expect(off.locator(".sync-tag")).toHaveText("Off");
  await expect(off).toContainText("15 ms");
  await expect(off).not.toContainText(/aiming for|ppm|Rate adjustment/, {
    useInnerText: true,
  });
  const priming = row(page, "Preview recording input");
  await expect(priming.locator(".sync-tag")).toHaveText("Starting…");
  await expect(priming).not.toContainText("Rate adjustment", {
    useInnerText: true,
  });
  await expect(priming).toContainText("5 ms (aiming for 15 ms)");
});

test("adjustment wording describes consumed input and keeps a true minus sign", async ({
  page,
}) => {
  await openSyncDetails(page);
  await publish(page, {
    "src:strip:1": { appliedAdjustmentPpm: -3.14 },
    "src:strip:2": { appliedAdjustmentPpm: 0.01 },
  });
  await expect(row(page, "Preview player")).toContainText(
    "Reads input slightly slower (−3.1 ppm)",
  );
  await expect(row(page, "Preview browser")).toContainText(
    "No adjustment (0.0 ppm)",
  );
});

test("adjustments at the controller bound are accepted", async ({ page }) => {
  await openSyncDetails(page);
  const ratio = 1 / (1 - 1000 / 1e6);
  await publish(page, {
    "src:strip:1": { appliedAdjustmentPpm: (1 / ratio - 1) * 1e6 },
    "src:strip:2": { appliedAdjustmentPpm: 1000 },
  });
  await expect(row(page, "Preview player")).toContainText("\u22121000.0 ppm");
  await expect(row(page, "Preview browser")).toContainText("+1000.0 ppm");
});

test("a fresh state origin never renews stale audio facts", async ({
  page,
}) => {
  await openSyncDetails(page);
  const player = row(page, "Preview player");
  await publish(
    page,
    { "src:strip:1": {} },
    { stateAgeMs: 0, audioAgeMs: 3000 },
  );
  await expect(player.locator(".sync-tag")).toHaveText("Running");
  await expect(player).not.toContainText(
    /Rate adjustment|Buffered|Last minute/,
    { useInnerText: true },
  );
  await expect(player).toContainText("3 s ago");
  await publish(
    page,
    { "src:strip:1": {} },
    { stateAgeMs: 3000, audioAgeMs: 0 },
  );
  await expect(player.locator(".sync-tag")).toHaveText("No recent data");
  await expect(player).not.toContainText(/Rate adjustment|Buffered/, {
    useInnerText: true,
  });
});

test("an older origin cannot replace newer audio facts", async ({ page }) => {
  await openSyncDetails(page);
  const player = row(page, "Preview player");
  const first = await publish(page, {
    "src:strip:1": { appliedAdjustmentPpm: 5 },
  });
  await publish(page, {
    "src:strip:1": { appliedAdjustmentPpm: 99, audioObservedAtMs: first - 1 },
  });
  await expect(player).toContainText("+5.0 ppm", { useInnerText: true });
  await expect(player).not.toContainText("99", { useInnerText: true });
});

test("recent restarts come from observed increases; new epochs and gaps restart the window", async ({
  page,
}) => {
  await openSyncDetails(page);
  const player = row(page, "Preview player");
  await publish(page, { "src:strip:1": {} });
  await page.waitForTimeout(120);
  await publish(page, {
    "src:strip:1": {
      phase: "priming",
      resyncCount: 1,
      gapOutputFrames: 480,
      lastResetReason: "stalled-input",
      lastResetAtMs: 0,
    },
  });
  await expect(player.locator(".sync-tag")).toHaveText("Restarting…");
  await expect(player).toContainText("1 restart · 10 ms of silence");
  await expect(player.locator("dd[data-warn]")).toHaveCount(1);
  await expect(player).toContainText("the sound stopped arriving");
  // A user action or mode change publishes a new epoch: it is never reported as a restart.
  await publish(page, { "src:strip:1": { epoch: "fixture:2" } });
  await page.waitForTimeout(120);
  await publish(page, { "src:strip:1": { epoch: "fixture:2" } });
  await expect(player).toContainText("No dropouts");
  await expect(player.locator("dd[data-warn]")).toHaveCount(0);
  // An observation gap longer than one second invalidates the delta baseline.
  await page.waitForTimeout(1200);
  await publish(page, {
    "src:strip:1": { epoch: "fixture:2", gapOutputFrames: 960 },
  });
  await expect(player).not.toContainText("Last minute", { useInnerText: true });
  await expect(player).not.toContainText("20 ms of silence", {
    useInnerText: true,
  });
});

test("saturated u64 counters report a count limit instead of a clean window", async ({
  page,
}) => {
  await openSyncDetails(page);
  const player = row(page, "Preview player");
  const max = Number.MAX_SAFE_INTEGER;
  await publish(page, { "src:strip:1": { gapOutputFrames: max } });
  await page.waitForTimeout(120);
  await publish(page, { "src:strip:1": { gapOutputFrames: max } });
  await expect(player).toContainText("Unavailable (count limit reached)");
  await expect(player).not.toContainText("No dropouts", { useInnerText: true });
  await page.waitForTimeout(120);
  await publish(page, { "src:strip:1": { gapOutputFrames: max } });
  await expect(player).toContainText("Unavailable (count limit reached)");
});

for (const [name, stage, options] of [
  ["unknown phase", { phase: "paused" }, {}],
  ["unknown reason", { lastResetReason: "cosmic", lastResetAtMs: 0 }, {}],
  ["reason without a time", { lastResetReason: "overrun" }, {}],
  ["non-finite adjustment", { appliedAdjustmentPpm: Number.NaN }, {}],
  ["out-of-range adjustment", { appliedAdjustmentPpm: 5000 }, {}],
  [
    "Off with an adjustment",
    { phase: "off", appliedAdjustmentPpm: 1, targetFrames: null },
    {},
  ],
  ["queue above capacity", { queuedFrames: 5000 }, {}],
  ["zero capacity", { capacityFrames: 0 }, {}],
  ["fractional counter", { resyncCount: 1.5 }, {}],
  ["empty epoch", { epoch: "" }, {}],
  ["oversized epoch", { epoch: "e".repeat(129) }, {}],
  ["control characters in epoch", { epoch: "a\nb" }, {}],
  ["numeric epoch", { epoch: 7 }, {}],
  ["future state origin", {}, { stateAgeMs: -5000 }],
] as const)
  test(`malformed timing is never displayed: ${name}`, async ({ page }) => {
    await openSyncDetails(page);
    await publish(page, { "src:strip:1": stage }, options);
    const player = row(page, "Preview player");
    await expect(player.locator(".sync-tag")).toHaveText("No recent data");
    await expect(player).not.toContainText(
      /ppm|Buffered|Last minute|Running|Updated/,
      { useInnerText: true },
    );
  });

test("Limit loud peaks status uses plain words, ignores earlier modes and ages out", async ({
  page,
}) => {
  await openSettings(page);
  await page.locator(".set-summary").click();
  const line = page.locator(".set-live");
  await publish(page, {}, { limiterReductionDb: null });
  await expect(line).toBeHidden();
  const limiter = page.getByRole("switch", { name: /Limit loud peaks/ });
  await limiter.click();
  await publish(page, {}, { limiterReductionDb: 0 });
  await expect(line).toHaveText("No peaks needed lowering recently");
  await publish(page, {}, { limiterReductionDb: 2.1 });
  await expect(line).toHaveText("Lowered loud peaks in the last 10 seconds");
  await expect(line).not.toContainText("dB", { useInnerText: true });
  // Disable and re-enable: the earlier reduction must not be shown for the new period.
  await limiter.click();
  await publish(page, {}, { limiterReductionDb: null });
  await expect(line).toBeHidden();
  await limiter.click();
  await publish(page, {}, { limiterReductionDb: 0 });
  await expect(line).toHaveText("No peaks needed lowering recently");
  await expect(line).toHaveText("No recent data", { timeout: 2500 });
});

test("value-only telemetry never re-renders the control tree", async ({
  page,
}) => {
  await openSyncDetails(page);
  await publish(page, { "src:strip:1": {}, "src:strip:2": {} });
  await expect(page.locator(".sync-row")).toHaveCount(2);
  const before = await page.evaluate(() => window.uiHarness.commits);
  for (const ppm of [3, 4, 5])
    await publish(page, {
      "src:strip:1": { appliedAdjustmentPpm: ppm },
      "src:strip:2": {},
    });
  await expect(row(page, "Preview player")).toContainText("+5.0 ppm");
  expect(await page.evaluate(() => window.uiHarness.commits)).toBe(before);
});

test("source mute keeps its stage; replacing the source drops it until new telemetry", async ({
  page,
}) => {
  await openSyncDetails(page);
  await publish(page, { "src:strip:1": {}, "src:strip:2": {} });
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      inputs: t.current.inputs.map((input) =>
        input.stripId === "strip:1" ? { ...input, muted: true } : input,
      ),
    });
  });
  await expect(row(page, "Preview player").locator(".sync-tag")).toHaveText(
    "Running",
  );
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      inputs: t.current.inputs.map((input) =>
        input.stripId === "strip:1"
          ? { ...input, sourceId: "rec:cable", displayName: "Replacement" }
          : input,
      ),
    });
  });
  await expect(page.locator(".sync-row")).toHaveCount(1);
});

test("a stage from another destination session is ignored", async ({
  page,
}) => {
  await openSyncDetails(page);
  await publish(page, { "dst:local:1": {} }, { sessionId: 99 });
  await expect(page.getByText("Playback devices")).toHaveCount(0);
  await publish(page, { "dst:local:1": {} }, { sessionId: 1 });
  await expect(row(page, "Preview headphones")).toBeVisible();
});

test("On Air stays uncluttered and the overload hint never claims a reduction", async ({
  page,
}) => {
  await page.waitForTimeout(600);
  await expect(page.locator(".surface")).not.toContainText(/ppm|restart/, {
    useInnerText: true,
  });
  const clip = page
    .locator('.dest-card[data-kind="playback"] .meter-clip')
    .first();
  await expect(clip).toHaveAttribute(
    "title",
    "The combined sound is too loud. Lower a source.",
  );
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({ settings: { ...t.current.settings, mixLimiter: true } });
  });
  await expect(clip).toHaveAttribute(
    "title",
    "The combined sound is too loud. Lower a source. Limit loud peaks is on.",
  );
});

test("playback details add one Sync correction line and keep a single buffer readout", async ({
  page,
}) => {
  await page.waitForTimeout(1800);
  await page
    .getByRole("button", {
      name: /Playback output details for Preview headphones/,
    })
    .first()
    .click();
  const sheet = page.getByRole("dialog");
  await expect(
    sheet.locator("dt", { hasText: /^Sync correction$/ }),
  ).toBeVisible();
  await expect(sheet).toContainText(/Running · reads input slightly/);
  await expect(sheet.locator("dt", { hasText: /^Buffered$/ })).toHaveCount(0);
});

test("saturated u32 output counters read as lower bounds, never clean zero", async ({
  page,
}) => {
  await page.waitForTimeout(600);
  await page
    .getByRole("button", {
      name: /Playback output details for Preview headphones/,
    })
    .first()
    .click();
  const sheet = page.getByRole("dialog");
  const max = 4_294_967_295;
  await publish(page, {}, { droppedFrames: max, underrunFrames: max });
  await page.waitForTimeout(120);
  await publish(page, {}, { droppedFrames: max, underrunFrames: max });
  await expect(sheet).toContainText(`${max} or more`);
  await expect(sheet).toContainText("Unavailable (count limit reached)");
  await expect(
    sheet.locator(".stat", { hasText: "Dropped" }).locator("[data-value]"),
  ).toHaveText("—");
});

test("a rejected Advanced toggle explains itself in view at 420x420 and keeps the honest value", async ({
  page,
}) => {
  await page.setViewportSize({ width: 420, height: 420 });
  await openSettings(page);
  await page.locator(".set-summary").click();
  const limiter = page.getByRole("switch", { name: /Limit loud peaks/ });
  await limiter.scrollIntoViewIfNeeded();
  await page.evaluate(() => {
    window.uiHarness.transport.reject = {
      code: "busy",
      message: "Wait for the current operation to finish.",
    };
  });
  await limiter.focus();
  await page.keyboard.press("Space");
  const alert = page.getByRole("alert");
  await expect(alert).toHaveCount(1);
  await expect(alert).toHaveText(
    "Setting change failed. Wait for the current operation to finish.",
  );
  await expect(alert).toBeInViewport();
  await expect(limiter).toBeInViewport();
  await expect(limiter).toBeFocused();
  // A save warning makes the footer taller; the focused switch must stay unobscured.
  await page.evaluate(() =>
    window.uiHarness.transport.emit({
      preferencesWarning: {
        code: "configuration",
        message: "Preferences could not be saved.",
      },
    }),
  );
  await expect(page.getByRole("dialog").locator(".save-notice")).toBeVisible();
  await expect
    .poll(() =>
      limiter.evaluate((el) => {
        const box = el.getBoundingClientRect();
        const foot = document.querySelector(".sheet-foot");
        const body = document.querySelector(".sheet-body");
        return (
          !!foot &&
          !!body &&
          box.bottom + 4 <= foot.getBoundingClientRect().top &&
          box.top - 4 >= body.getBoundingClientRect().top
        );
      }),
    )
    .toBe(true);
  await expect(alert).toBeInViewport();
  await expect(limiter).toBeFocused();
  await expect(limiter).not.toBeChecked();
  await expect(page.locator(".set-summary")).toContainText(
    "Limit loud peaks off",
  );
  expect(
    await page.evaluate(
      () => window.uiHarness.transport.current.settings.mixLimiter,
    ),
  ).toBe(false);
  await page.evaluate(() => {
    window.uiHarness.transport.reject = null;
  });
  await page.keyboard.press("Space");
  await expect(limiter).toBeChecked();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a settings rejection arriving after Settings closes stays visible and dismissible", async ({
  page,
}) => {
  await openSettings(page);
  await page.locator(".set-summary").click();
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const original = t.dispatch;
    t.dispatch = (command) =>
      command.type === "set-settings"
        ? new Promise<void>((_, reject) =>
            setTimeout(
              () =>
                reject({
                  code: "busy",
                  message: "Wait for the current operation to finish.",
                }),
              300,
            ),
          )
        : original(command);
  });
  await page.getByRole("switch", { name: /Limit loud peaks/ }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const banner = page.locator(".app-error");
  await expect(banner).toHaveText(
    "Setting change failed. Wait for the current operation to finish.",
  );
  await expect(page.getByRole("alert")).toHaveCount(1);
  await banner.getByRole("button", { name: "Dismiss error" }).click();
  await expect(banner).toHaveCount(0);
  expect(
    await page.evaluate(
      () => window.uiHarness.transport.current.settings.mixLimiter,
    ),
  ).toBe(false);
});
