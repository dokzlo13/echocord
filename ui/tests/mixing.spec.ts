import { expect, type Page, test } from "@playwright/test";
import type { Harness } from "./harness";

async function open(page: Page, fixture = "connected") {
  await page.goto(`/tests/harness.html?fixture=${fixture}`);
  await expect(page.locator(".source-card").first()).toBeVisible();
}
async function commands(page: Page) {
  return page.evaluate(() => (window.uiHarness as Harness).transport.commands);
}

test("first run has two empty slots and opens a full window source picker", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=first-run");
  await expect(page.getByText("No sources yet")).toBeVisible();
  await expect(page.getByText("No destinations yet")).toBeVisible();
  await page
    .getByRole("button", { name: "Add source", exact: true })
    .last()
    .click();
  await expect(page.getByRole("dialog", { name: "Add source" })).toBeVisible();
  const box = await page.getByRole("dialog").boundingBox();
  expect(box?.width).toBe(440);
  expect(box?.y).toBe(36);
  await expect(page.getByRole("searchbox")).toBeFocused();
});

test("adding an input immediately targets capture without connecting Discord", async ({
  page,
}) => {
  await open(page, "disconnected");
  await page.getByRole("button", { name: "Add source", exact: true }).click();
  await page.getByRole("tab", { name: "Inputs" }).click();
  await page
    .getByRole("option", { name: /Preview recording input/ })
    .dblclick();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator(".source-card")).toHaveCount(2);
  expect((await commands(page)).map((c) => c.type)).toEqual(["add-source"]);
});

test("source replacement and source operations retain the live destination", async ({
  page,
}) => {
  await open(page);
  const card = page.locator(".source-card").first();
  await card.getByRole("button", { name: "Change Preview player" }).click();
  await page.getByRole("option", { name: /Preview browser/ }).press("Enter");
  await expect(card).toContainText("Preview browser");
  await card
    .getByRole("button", { name: "Stop capturing Preview browser" })
    .click();
  await card
    .getByRole("button", { name: "Capture Preview browser", exact: true })
    .click();
  await card
    .getByRole("button", { name: "Mute Preview browser in the mix" })
    .click();
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "live",
  );
  expect((await commands(page)).map((c) => c.type)).toEqual([
    "replace-source",
    "stop-capture",
    "start-capture",
    "set-source-muted",
  ]);
});

test("source removal is targeted and colors stay assigned to surviving cards", async ({
  page,
}) => {
  await open(page, "many");
  await page
    .getByRole("button", { name: "Remove Preview player", exact: true })
    .click();
  await expect(page.locator(".source-card").first()).toHaveAttribute(
    "data-colour",
    "1",
  );
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "live",
  );
  expect((await commands(page)).map((c) => c.type)).toEqual(["remove-source"]);
});

test("source failure is contained on its card while destination remains live", async ({
  page,
}) => {
  await open(page, "many");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const inputs = structuredClone(t.current.inputs);
    const s = inputs[0];
    if (s) {
      s.phase = "fault";
      s.fault = {
        code: "capture-failed",
        message: "The selected process exited. Pick again.",
      };
    }
    t.emit({ inputs });
  });
  await expect(
    page.locator(".source-card").first().getByRole("alert"),
  ).toContainText("process exited");
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "live",
  );
  await expect(page.locator(".source-card").nth(1)).toHaveAttribute(
    "data-state",
    "capturing",
  );
});

test("restored application waits for explicit Capture and preserves destination idle", async ({
  page,
}) => {
  await open(page, "restored");
  const restored = page.locator(".source-card").nth(1);
  await expect(restored).toHaveAttribute("data-state", "stopped");
  await expect(restored).toContainText("press Capture to reattach");
  expect(await commands(page)).toEqual([]);
  await restored
    .getByRole("button", { name: "Capture Preview browser", exact: true })
    .click();
  await expect(restored).toHaveAttribute("data-state", "capturing");
  expect((await commands(page))[0]).toEqual({
    type: "start-capture",
    stripId: "strip:2",
  });
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "idle",
  );
});

test("fractional dB keyboard control and Off preserve native precision", async ({
  page,
}) => {
  await open(page);
  const gain = page.getByRole("slider", { name: "Preview player gain" });
  await gain.focus();
  await gain.press("ArrowLeft");
  await expect(gain).toHaveAttribute("aria-valuetext", "−3.5 dB");
  await expect
    .poll(async () => commands(page))
    .toContainEqual({
      type: "set-source-gain",
      stripId: "strip:1",
      gainDb: -3.5,
    });
  await gain.press("PageDown");
  await expect(gain).toHaveAttribute("aria-valuetext", "−9.5 dB");
  await gain.press("End");
  await expect(gain).toHaveAttribute("aria-valuetext", "0.0 dB");
  await gain.press("Home");
  await expect(gain).toHaveAttribute("aria-valuetext", "Off");
  await gain.blur();
  await expect
    .poll(async () => commands(page))
    .toContainEqual({
      type: "set-source-gain",
      stripId: "strip:1",
      gainDb: null,
    });
});

test("destination mute uses its own command and keeps source metering", async ({
  page,
}) => {
  await open(page);
  await page
    .locator(".dest-card")
    .getByRole("button", { name: "Mute Session room", exact: true })
    .click();
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-muted",
    "true",
  );
  await expect(page.locator(".source-card")).toHaveAttribute(
    "data-state",
    "capturing",
  );
  expect((await commands(page))[0]).toEqual({
    type: "set-destination-muted",
    destinationId: "dest:1",
    muted: true,
  });
});

test("connect, cancel and disconnect remain destination operations", async ({
  page,
}) => {
  await open(page, "disconnected");
  await page.evaluate(() => (window.uiHarness.transport.holdConnect = true));
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Cancel", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Connect", exact: true }),
  ).toBeVisible();
  expect((await commands(page)).map((c) => c.type)).toEqual([
    "connect-destination",
    "disconnect-destination",
  ]);
  await expect(page.locator(".source-card")).toHaveAttribute(
    "data-state",
    "capturing",
  );
});

test("cleanup incomplete blocks reconnect and exposes Retry disconnect", async ({
  page,
}) => {
  await open(page, "cleanup");
  await expect(
    page.getByRole("button", { name: "Retry disconnect", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Connect", exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Retry disconnect", exact: true })
    .click();
  expect((await commands(page))[0]?.type).toBe("disconnect-destination");
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "idle",
  );
});

test("live destination removal has inline confirmation and Keep cancels", async ({
  page,
}) => {
  await open(page);
  await page
    .getByRole("button", { name: "Remove Session room", exact: true })
    .click();
  await expect(page.getByRole("alertdialog")).toBeVisible();
  await page.getByRole("button", { name: "Keep", exact: true }).click();
  expect(await commands(page)).toEqual([]);
  await page
    .getByRole("button", { name: "Remove Session room", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Disconnect & remove", exact: true })
    .click();
  await expect(page.locator(".dest-card")).toHaveCount(0);
  expect((await commands(page))[0]?.type).toBe("remove-destination");
  await expect(page.locator(".source-card")).toHaveCount(1);
});

test("one Discord room limit is explicit and room replacement names reconnect", async ({
  page,
}) => {
  await open(page);
  await expect(
    page.getByRole("button", { name: "Add destination", exact: true }),
  ).toBeEnabled();
  await page
    .getByRole("button", { name: "Add destination", exact: true })
    .click();
  await page.getByRole("tab", { name: "Discord", exact: true }).click();
  await expect(
    page.getByText(/One Discord room is supported currently/),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Change room", exact: true }).click();
  await page.getByRole("option", { name: "Lounge", exact: true }).click();
  await expect(
    page.getByText("Briefly reconnects to change rooms"),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Move to Lounge", exact: true })
    .click();
  await expect(page.locator(".dest-card")).toContainText("Lounge");
  expect((await commands(page))[0]).toEqual({
    type: "change-destination",
    destinationId: "dest:1",
    serverId: "guild:1",
    channelId: "room:2",
  });
});

test("source picker shows existing and unsupported inputs without invented activity", async ({
  page,
}) => {
  await open(page, "many");
  await page.getByRole("button", { name: "Add source", exact: true }).click();
  await expect(
    page.getByRole("option", { name: /Preview player/ }),
  ).toHaveAttribute("aria-disabled", "true");
  await page.getByRole("tab", { name: "Inputs" }).click();
  await expect(
    page.getByRole("option", { name: /Unsupported input/ }),
  ).toHaveAttribute("aria-disabled", "true");
  await expect(
    page.getByRole("option", { name: /Unsupported input/ }),
  ).toContainText("48 kHz");
  await expect(page.locator(".activity")).toHaveCount(0);
});

test("real refresh and command failure remain visible in the picker", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: "Add source", exact: true }).click();
  await page.getByRole("button", { name: "Refresh", exact: true }).click();
  expect((await commands(page))[0]?.type).toBe("refresh-sources");
  await page.evaluate(
    () =>
      (window.uiHarness.transport.reject = {
        code: "capture-failed",
        message:
          "This application is no longer available. Refresh and pick again.",
      }),
  );
  await page.getByRole("option", { name: /Preview browser/ }).click();
  await page
    .getByRole("button", { name: "Add Preview browser", exact: true })
    .click();
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
    "no longer available",
  );
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("sheet keyboard focus is trapped and Escape returns to card identity", async ({
  page,
}) => {
  await open(page);
  const identity = page.getByRole("button", { name: "Change Preview player" });
  await identity.click();
  await expect(page.getByRole("searchbox")).toBeFocused();
  await page.getByRole("searchbox").press("Shift+Tab");
  await expect(page.getByRole("tab", { name: "Inputs" })).toBeFocused();
  await page.getByRole("button", { name: "Back", exact: true }).focus();
  await page.keyboard.press("Shift+Tab");
  await expect(
    page.getByRole("button", { name: "Refresh", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(identity).toBeFocused();
});

test("settings use native persisted commands for meters restore and tray", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("radio", { name: "History", exact: true }).check();
  await page.getByRole("switch", { name: /Restore my setup/ }).uncheck();
  await page
    .getByRole("switch", { name: /Keep running in the tray/ })
    .uncheck();
  const sent = await commands(page);
  expect(sent.filter((c) => c.type === "set-settings")).toHaveLength(3);
  expect(sent.at(-1)).toEqual({
    type: "set-settings",
    settings: { meterStyle: "history", restoreSetup: false, keepInTray: false },
  });
  await page.keyboard.press("Escape");
  await expect(page.locator(".source-card .cf")).toHaveAttribute(
    "data-mode",
    "history",
  );
});

test("bot setup saves through native adapter and clears the credential field", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=first-run");
  await page
    .getByRole("button", { name: "Add destination", exact: true })
    .last()
    .click();
  await page.getByRole("tab", { name: "Discord", exact: true }).click();
  await page
    .getByRole("button", { name: "Set up bot", exact: true })
    .last()
    .click();
  await page
    .getByLabel("Bot token", { exact: true })
    .fill("synthetic-test-token");
  await page.getByRole("button", { name: "Test & save", exact: true }).click();
  await expect(page.getByText("is ready.", { exact: false })).toBeVisible();
  await expect(page.getByLabel("Bot token", { exact: true })).toHaveCount(0);
  await page
    .getByRole("button", { name: "Choose a room", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Add destination" }),
  ).toBeVisible();
});

test("bot details do not invent profile fields or permissions and lock replacement while live", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: "Preview bot", exact: false }).click();
  await expect(page.getByText("Token verified", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Replace token", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByText("Room permissions are checked when you connect."),
  ).toBeVisible();
  await expect(page.getByText(/ID 118/)).toHaveCount(0);
  await expect(page.locator(".perm-flags")).toHaveCount(0);
});

test("connection details expose unavailable metrics and true measurement boundary", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: "On air", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: /Connection details/ }),
  ).toBeVisible();
  await expect(page.getByText("Unavailable", { exact: true })).toHaveCount(2);
  await expect(
    page.getByText(/does not confirm what players hear/),
  ).toBeVisible();
  await expect(page.getByText(/AES-256|5 in room|38 ms/)).toHaveCount(0);
});

test("native shell controls are wired and double-clicking chrome never maximizes", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: "Minimize", exact: true }).click();
  await page
    .getByRole("button", { name: "Close to tray", exact: true })
    .click();
  await page
    .locator(".brand")
    .dispatchEvent("mousedown", { button: 0, detail: 1 });
  await page
    .locator(".brand")
    .dispatchEvent("mousedown", { button: 0, detail: 2 });
  const v = await page.evaluate(() => ({
    min: window.uiHarness.transport.minimized,
    close: window.uiHarness.transport.closed,
    drag: window.uiHarness.transport.dragged,
  }));
  expect(v).toEqual({ min: 1, close: 1, drag: 1 });
  await expect(page.getByRole("button", { name: "Maximize" })).toHaveCount(0);
});

for (const [w, h] of [
  [440, 640],
  [455, 455],
  [420, 420],
  [760, 440],
  [1000, 640],
] as const)
  test(`Stack fits ${w}x${h} with independently scrolling lists`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: w, height: h });
    await open(page, "many");
    const fit = await page.evaluate(() => {
      const first = document
        .querySelector(".source-card")
        ?.getBoundingClientRect();
      const src = document.querySelector(".src-list")?.getBoundingClientRect();
      const dst = document.querySelector(".dest-card")?.getBoundingClientRect();
      return {
        sw: document.documentElement.scrollWidth,
        sh: document.documentElement.scrollHeight,
        first: !!first && !!src && first.bottom <= src.bottom + 1,
        dst: !!dst && dst.bottom <= innerHeight,
      };
    });
    expect(fit).toEqual({ sw: w, sh: h, first: true, dst: true });
    await expect(
      page.getByRole("button", { name: "Show Preview recording input" }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Show Preview recording input" })
      .click();
    await expect(
      page.getByRole("button", { name: "Change Preview recording input" }),
    ).toBeFocused();
  });

test("telemetry bursts update refs while control React commits stay constant", async ({
  page,
}) => {
  await open(page);
  await page.waitForTimeout(180);
  const before = await page.evaluate(() => window.uiHarness.commits);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    for (let i = 0; i < 300; i++)
      t.emit({
        telemetry: {
          ...t.current.telemetry,
          inputs: [
            {
              stripId: "strip:1",
              levelDbfs: -8,
              preClip: true,
              queuedFrames: 0,
              droppedFrames: 0,
              underrunFrames: 0,
            },
          ],
          outputLevelDbfs: -12,
          queuedFrames: 960,
          droppedFrames: i,
        },
      });
  });
  await expect
    .poll(() =>
      page
        .locator(".source-card")
        .evaluate((e) => e.style.getPropertyValue("--level")),
    )
    .toBe("0.867");
  expect(await page.evaluate(() => window.uiHarness.commits)).toBe(before);
  await expect(page.locator(".source-card")).toHaveAttribute("data-clip", "");
  await expect(page.locator(".dest-card .stats")).toContainText("20");
});

test("muting a source hatches a moving postgain meter, destination mute observes actual silence", async ({
  page,
}) => {
  await open(page);
  await page
    .locator(".source-card")
    .getByRole("button", { name: "Mute Preview player in the mix" })
    .click();
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      telemetry: {
        ...t.current.telemetry,
        inputs: [
          {
            stripId: "strip:1",
            levelDbfs: -14,
            preClip: false,
            queuedFrames: 0,
            droppedFrames: 0,
            underrunFrames: 0,
          },
        ],
        outputLevelDbfs: null,
      },
    });
  });
  await expect(page.locator(".source-card .cf")).toHaveAttribute(
    "data-muted",
    "true",
  );
  await expect
    .poll(() =>
      page
        .locator(".source-card")
        .evaluate((e) => e.style.getPropertyValue("--level")),
    )
    .toBe("0.767");
  await expect
    .poll(() =>
      page
        .locator(".dest-card")
        .evaluate((e) => e.style.getPropertyValue("--level")),
    )
    .toBe("0.000");
});

test("native hidden state pauses paint and catches up on visibility return", async ({
  page,
}) => {
  await open(page);
  await page.waitForTimeout(100);
  const before = await page
    .locator(".source-card")
    .evaluate((e) => e.style.getPropertyValue("--level"));
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.visible(false);
    t.emit({
      telemetry: {
        ...t.current.telemetry,
        inputs: [
          {
            stripId: "strip:1",
            levelDbfs: -4,
            preClip: false,
            queuedFrames: 0,
            droppedFrames: 0,
            underrunFrames: 0,
          },
        ],
      },
    });
  });
  await page.waitForTimeout(150);
  expect(
    await page
      .locator(".source-card")
      .evaluate((e) => e.style.getPropertyValue("--level")),
  ).toBe(before);
  await page.evaluate(() => window.uiHarness.transport.visible(true));
  await expect
    .poll(() =>
      page
        .locator(".source-card")
        .evaluate((e) => e.style.getPropertyValue("--level")),
    )
    .toBe("0.933");
});

test("reduced motion suppresses badge and card animations", async ({
  page,
}) => {
  await open(page, "connecting");
  expect(
    await page
      .locator(".badge-dot")
      .evaluate((e) => getComputedStyle(e).animationName),
  ).toBe("none");
  expect(
    await page
      .locator(".card")
      .first()
      .evaluate((e) => getComputedStyle(e).transitionDuration),
  ).toBe("0s");
});

test("native summed overload remains visible after destination attenuation", async ({
  page,
}) => {
  await open(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      telemetry: {
        ...t.current.telemetry,
        mixPreClip: true,
        outputLevelDbfs: -36,
      },
    });
  });
  await expect
    .poll(() => page.locator(".dest-card").getAttribute("data-clip"))
    .toBe("");
  await expect(page.locator(".dest-card .meter-clip")).toHaveAttribute(
    "title",
    "Mix clipped before destination volume",
  );
  await expect
    .poll(() =>
      page
        .locator(".dest-card")
        .evaluate((e) => e.style.getPropertyValue("--level")),
    )
    .toBe("0.400");
});

test("History meters keep complete source and destination controls at minimum window", async ({
  page,
}) => {
  await page.setViewportSize({ width: 420, height: 420 });
  await open(page, "many");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({ settings: { ...t.current.settings, meterStyle: "history" } });
  });
  await expect(page.locator(".source-card .cf").first()).toHaveAttribute(
    "data-mode",
    "history",
  );
  const fit = await page.evaluate(() => {
    const src = document.querySelector(".source-card")?.getBoundingClientRect();
    const list = document.querySelector(".src-list")?.getBoundingClientRect();
    const dst = document.querySelector(".dest-card")?.getBoundingClientRect();
    return {
      source: !!src && !!list && src.bottom <= list.bottom + 1,
      destination: !!dst && dst.bottom <= innerHeight,
      scroll: document.documentElement.scrollHeight,
    };
  });
  expect(fit).toEqual({ source: true, destination: true, scroll: 420 });
});

test("rejected persisted settings stay readable inside their sheet", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.evaluate(
    () =>
      (window.uiHarness.transport.reject = {
        code: "configuration",
        message: "Could not save settings. Try again.",
      }),
  );
  await page.getByRole("radio", { name: "History", exact: true }).click();
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
    "Could not save settings",
  );
  await expect(
    page.getByRole("radio", { name: "Simple", exact: true }),
  ).toBeChecked();
});

test("targeted source operation errors preserve whole cards and live native phases at420", async ({
  page,
}) => {
  await page.setViewportSize({ width: 420, height: 420 });
  await open(page, "many");
  await page.evaluate(
    () =>
      (window.uiHarness.transport.reject = {
        code: "capture-failed",
        message:
          "The selected application changed. Refresh sources and pick the running process again.",
      }),
  );
  await page
    .getByRole("button", { name: "Stop capturing Preview player", exact: true })
    .click();
  await expect(
    page.locator(".source-card").first().getByRole("alert"),
  ).toContainText("selected application changed");
  await expect(page.locator(".source-card").first()).toHaveAttribute(
    "data-state",
    "capturing",
  );
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "live",
  );
  expect(
    await page
      .locator(".dest-card")
      .evaluate((e) => e.getBoundingClientRect().bottom <= innerHeight),
  ).toBe(true);
  await expect(
    page.getByRole("button", { name: "Disconnect", exact: true }),
  ).toBeVisible();
  await page
    .locator(".source-card")
    .first()
    .getByRole("button", { name: "Dismiss", exact: true })
    .click();
  await expect(
    page.locator(".source-card").first().getByRole("alert"),
  ).toHaveCount(0);
});

test("a removal that succeeds before a settings save rejection still reports the failure", async ({
  page,
}) => {
  await open(page, "many");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const original = t.dispatch;
    t.dispatch = async (command) => {
      if (command.type === "remove-source") {
        t.emit({
          inputs: t.current.inputs.filter((s) => s.stripId !== command.stripId),
        });
        throw {
          code: "configuration",
          message:
            "Source removed, but the saved setup could not be updated. Try again.",
        };
      }
      await original(command);
    };
  });
  await page
    .getByRole("button", { name: "Remove Preview player", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "saved setup could not be updated",
  );
  await expect(page.locator(".source-card")).toHaveCount(2);
});
