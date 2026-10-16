import { expect, type Page, test } from "@playwright/test";

async function open(page: Page, fixture = "connected") {
  await page.goto(`/tests/harness.html?fixture=${fixture}`);
  await expect(page.locator(".source-card").first()).toBeVisible();
}
async function recordingFault(page: Page) {
  await open(page, "many");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      inputs: t.current.inputs.map((input) =>
        input.stripId === "strip:3"
          ? {
              ...input,
              phase: "fault",
              gainDb: -8.5,
              muted: true,
              colourSlot: 4,
              fault: {
                code: "capture-failed",
                message:
                  "The selected recording device is no longer available.",
              },
            }
          : input,
      ),
    });
  });
  return page.locator("#card-strip\\:3");
}

test("a recording input retries in place with controls and its saved identity intact", async ({
  page,
}) => {
  const card = await recordingFault(page);
  await card.getByRole("button", { name: /^Capture / }).click();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)),
  ).toEqual({
    type: "start-capture",
    stripId: "strip:3",
  });
  expect(
    await page.evaluate(() => {
      const input = window.uiHarness.transport.current.inputs.find(
        (s) => s.stripId === "strip:3",
      );
      return (
        input && {
          id: input.sourceId,
          gain: input.gainDb,
          muted: input.muted,
          colour: input.colourSlot,
        }
      );
    }),
  ).toEqual({ id: "rec:cable", gain: -8.5, muted: true, colour: 4 });
  await expect(card.getByRole("slider")).toBeVisible();
  await expect(page.locator(".source-card")).toHaveCount(3);
});

test("fault picker refreshes and can explicitly reselect the same available identity", async ({
  page,
}) => {
  const card = await recordingFault(page);
  await card.getByRole("button", { name: /^Select .* again$/ }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)?.type),
  ).toBe("refresh-sources");
  const current = page.getByRole("option").filter({ hasText: "Current" });
  await expect(current).toHaveAttribute("aria-disabled", "false");
  await current.click();
  await page.getByRole("button", { name: /^Use / }).click();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)),
  ).toEqual({
    type: "replace-source",
    stripId: "strip:3",
    sourceId: "rec:cable",
  });
  await expect(card).toHaveAttribute("data-colour", "4");
});

test("worker restart faults do not offer a misleading recapture or picker action", async ({
  page,
}) => {
  await open(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      inputs: t.current.inputs.map((input) => ({
        ...input,
        phase: "fault",
        fault: {
          code: "engine-stopped",
          message:
            "The audio engine stopped. Quit Echocord from the tray icon, then open it again.",
        },
      })),
    });
  });
  const card = page.locator(".source-card");
  await expect(card.getByRole("button", { name: /^Capture / })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expect(
    card.getByRole("button", { name: /^Select .* again$/ }),
  ).toHaveCount(0);
  await expect(
    card.getByText(
      "The audio engine stopped. Quit Echocord from the tray icon, then open it again.",
    ),
  ).toBeVisible();
});

test("a failed output can retry the same destination without disturbing its peers", async ({
  page,
}) => {
  await open(page, "outputs");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      destinations: t.current.destinations.map((d) =>
        d.kind === "discord"
          ? {
              ...d,
              volumeDb: -13.5,
              muted: true,
              connection: {
                phase: "error",
                code: "connection-failed",
                message: "The voice connection stopped. Retry this room.",
              },
            }
          : d,
      ),
    });
  });
  const card = page.locator('.dest-card[data-kind="discord"]');
  await expect(card.getByRole("slider")).toBeVisible();
  await card.getByRole("button", { name: /^Connect / }).click();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)),
  ).toEqual({
    type: "connect-destination",
    destinationId: "dest:1",
  });
  expect(
    await page.evaluate(() => {
      const d = window.uiHarness.transport.current.destinations.find(
        (d) => d.destinationId === "dest:1",
      );
      return (
        d && {
          server: d.serverId,
          channel: d.channelId,
          gain: d.volumeDb,
          muted: d.muted,
        }
      );
    }),
  ).toEqual({ server: "guild:1", channel: "room:1", gain: -13.5, muted: true });
  await expect(
    page.locator('.dest-card[data-kind="playback"]').first(),
  ).toHaveAttribute("data-state", "live");
});

test("unconfirmed cleanup offers only cleanup and remains honest in badge summary and jump state", async ({
  page,
}) => {
  await open(page, "outputs");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      destinations: t.current.destinations.map((d) => ({
        ...d,
        connection:
          d.kind === "discord"
            ? {
                phase: "cleanup-incomplete",
                message: "Leaving the selected room has not been confirmed.",
              }
            : { phase: "disconnected" },
      })),
    });
  });
  const card = page.locator('.dest-card[data-kind="discord"]');
  await expect(card.getByText("Unconfirmed", { exact: true })).toBeVisible();
  await expect(page.locator(".dst-slot .slot-summary")).toContainText(
    "unconfirmed",
  );
  await expect(
    page.locator('.dst-slot .chip[data-state="cleanup-incomplete"]'),
  ).toHaveCount(1);
  await expect(card.getByRole("button", { name: /^Connect / })).toHaveCount(0);
  await expect(
    card.getByRole("button", { name: /^Change room / }),
  ).toHaveAttribute("aria-disabled", "true");
  await card.getByRole("button", { name: /^Disconnect / }).click();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)?.type),
  ).toBe("disconnect-destination");
});

test("recovery uses the backend explanation and never invents a retry budget or completed stages", async ({
  page,
}) => {
  await open(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      destinations: t.current.destinations.map((d) => ({
        ...d,
        connection: {
          phase: "recovering",
          attempt: 1,
          message: "Updating this room's voice connection.",
        },
      })),
    });
  });
  const card = page.locator(".dest-card");
  await expect(card.locator(".badge")).toHaveText("Reconnecting");
  await expect(card).toContainText("Updating this room's voice connection.");
  await expect(card).not.toContainText("of 3");
  await expect(card).not.toContainText("Connection dropped");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      destinations: t.current.destinations.map((d) => ({
        ...d,
        connection: { phase: "disconnecting" },
      })),
    });
  });
  await expect(card.locator(".badge")).toHaveText("Leaving");
  await expect(card).not.toContainText("Off air");
});

test("saved bot failures explain the reason and allow checking without replacing its token", async ({
  page,
}) => {
  await open(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      bot: {
        ...t.current.bot,
        status: "error",
        message: "Could not reach Discord. Check your connection.",
      },
    });
  });
  await page
    .getByRole("button", { name: "Bot check failed", exact: true })
    .click();
  const sheet = page.getByRole("dialog");
  await expect(sheet).toContainText(
    "Could not reach Discord. Check your connection.",
  );
  await expect(sheet.getByLabel("Bot token", { exact: true })).toBeDisabled();
  await sheet.getByRole("button", { name: "Check again", exact: true }).click();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)),
  ).toEqual({ type: "recheck-configuration" });
});

test("a source operation error stays readable when another fader changes", async ({
  page,
}) => {
  await open(page, "restored");
  await page.evaluate(() => {
    window.uiHarness.transport.reject = {
      code: "invalid-selection",
      message: "This app instance is no longer running. Select the app again.",
    };
  });
  const first = page.locator(".source-card").nth(1);
  await first.getByRole("button", { name: /^Capture / }).click();
  await expect(first.getByRole("note")).toContainText("Select the app again.");
  await expect(first.getByRole("slider")).toBeVisible();
  await page.evaluate(() => {
    window.uiHarness.transport.reject = null;
  });
  await page
    .locator(".source-card")
    .nth(1)
    .getByRole("slider")
    .press("ArrowLeft");
  await page.locator(".source-card").first().getByRole("slider").blur();
  await expect(first.getByRole("note")).toContainText("Select the app again.");
  await expect(
    first.getByRole("button", { name: /^Select .* again$/ }),
  ).toBeVisible();
});

test("a checking to join sequence never claims an unobserved gateway stage succeeded", async ({
  page,
}) => {
  await open(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      destinations: t.current.destinations.map((d) => ({
        ...d,
        connection: { phase: "connecting", stage: "checking" },
      })),
    });
  });
  await expect(page.locator(".d-log")).toContainText("Checking the bot");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      destinations: t.current.destinations.map((d) => ({
        ...d,
        connection: { phase: "connecting", stage: "join" },
      })),
    });
  });
  await expect(page.locator(".d-log")).not.toContainText("Reaching Discord");
  await expect(page.locator(".d-log li")).toHaveCount(2);
});

test("the destination action keeps keyboard focus through joining recovery and cleanup", async ({
  page,
}) => {
  await open(page);
  const card = page.locator(".dest-card");
  const action = card.locator(".controls-row > .btn-connect");
  await action.focus();
  const original = await action.elementHandle();
  for (const connection of [
    { phase: "connecting", stage: "join" },
    { phase: "connected" },
    {
      phase: "recovering",
      attempt: 1,
      message: "Updating this room's voice connection.",
    },
    { phase: "disconnecting" },
    {
      phase: "cleanup-incomplete",
      message: "Leaving this room has not been confirmed.",
    },
  ] as const) {
    await page.evaluate((connection) => {
      const t = window.uiHarness.transport;
      t.emit({
        destinations: t.current.destinations.map((destination) => ({
          ...destination,
          connection,
        })),
      });
    }, connection);
    await expect(action).toBeFocused();
    expect(
      await action.evaluate((node, original) => node === original, original),
    ).toBe(true);
  }
  await expect(action).toHaveText("Disconnect");
});

test("new card failures announce identity once without interrupting initial restoration", async ({
  page,
}) => {
  await open(page, "faults");
  const announcements = page.locator("#recovery-alert");
  await expect(announcements).toHaveText("");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      inputs: t.current.inputs.map((input, index) =>
        index === 1
          ? {
              ...input,
              phase: "fault",
              fault: {
                code: "capture-failed",
                message:
                  "The selected recording device is no longer available.",
              },
            }
          : input,
      ),
    });
  });
  await expect(announcements).toContainText("Source 2, Preview browser");
  await expect(announcements).toContainText("no longer available");
  const content = await announcements.locator("span").elementHandle();
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({ settings: { ...t.current.settings, meterStyle: "history" } });
  });
  expect(
    await announcements
      .locator("span")
      .evaluate((element, original) => element === original, content),
  ).toBe(true);
});
