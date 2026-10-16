import { expect, type Page, test } from "@playwright/test";

async function start(page: Page, fixture = "rooms") {
  await page.goto(`/tests/harness.html?fixture=${fixture}`);
  await expect(page.locator(".dest-card")).toHaveCount(4);
}
async function show(page: Page, name: string) {
  await page
    .getByRole("button", {
      name: new RegExp(
        `^Show destination \\d+: ${name}(?: on |, playback output)`,
      ),
    })
    .click();
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
  await second.getByRole("button", { name: /^Connect / }).click();
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
    .getByRole("button", {
      name: "Mute Сессия on Вечерняя группа",
      exact: true,
    })
    .click();
  await second
    .getByRole("slider", { name: "Сессия on Вечерняя группа volume" })
    .press("ArrowLeft");
  await second
    .getByRole("slider", { name: "Сессия on Вечерняя группа volume" })
    .blur();
  await second.getByRole("button", { name: /^Disconnect / }).click();
  await page.evaluate(() => {
    window.uiHarness.transport.holdConnect = true;
  });
  await second.getByRole("button", { name: /^Connect / }).click();
  await second.getByRole("button", { name: /^Cancel / }).click();
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
  await expect(denied.getByRole("note")).toContainText("cannot speak");
  const cleanup = await show(page, "Listening room");
  await expect(
    cleanup.getByRole("button", { name: /^Disconnect / }),
  ).toBeVisible();
  const local = await show(page, "Preview headphones");
  await local.getByRole("button", { name: /^Stop / }).click();
  await local.getByRole("button", { name: /^Play / }).click();
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

async function addPicker(page: Page) {
  await page
    .getByRole("button", { name: "Add destination", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Add destination" }),
  ).toBeVisible();
}

test("another server can be added idle without moving the existing live room", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=connected");
  await addPicker(page);
  await expect(
    page.getByRole("option", { name: "Сессия", exact: true }),
  ).toHaveAttribute("aria-disabled", "false");
  await page
    .getByRole("option", { name: "Сессия", exact: true })
    .press("Enter");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator('.dest-card[data-kind="discord"]')).toHaveCount(2);
  await expect(page.locator('.dest-card[data-state="live"]')).toHaveCount(1);
  await expect(page.locator('.dest-card[data-state="idle"]')).toHaveCount(1);
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([
    { type: "add-destination", serverId: "guild:2", channelId: "room:3" },
  ]);
});

test("idle configured rooms reserve their server and Current remains a no-op", async ({
  page,
}) => {
  await start(page, "rooms-restored");
  const first = await show(page, "Session room");
  await first.getByRole("button", { name: /^Change room / }).click();
  const current = page.getByRole("option", { name: /Session room/ });
  await expect(current).toContainText("Current");
  await current.press("Enter");
  await current.dblclick({ force: true });
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([]);
  await expect(
    page.getByRole("option", { name: "Lounge", exact: true }),
  ).toHaveAttribute("aria-disabled", "false");
  await page
    .getByRole("button", { name: "Вечерняя группа", exact: true })
    .click();
  await expect(page.getByRole("option", { name: /Сессия/ })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expect(page.getByText(/Another card reserves Сессия/)).toBeVisible();
  await page.getByRole("button", { name: "Studio group", exact: true }).click();
  await page
    .getByRole("option", { name: "Lounge", exact: true })
    .press("Enter");
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([
    {
      type: "change-destination",
      destinationId: "dest:1",
      serverId: "guild:1",
      channelId: "room:2",
    },
  ]);
  await expect(page.locator('.dest-card[data-state="idle"]')).toHaveCount(4);
});

test("retiring reservation identifies its old room and releases only after native cleanup", async ({
  page,
}) => {
  await start(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const ds = structuredClone(t.current.destinations).filter(
      (d) => d.kind === "discord",
    );
    const d = ds[0];
    if (!d) throw Error("Missing room");
    d.serverId = "guild:4";
    d.channelId = "room:5";
    d.connection = {
      phase: "cleanup-incomplete",
      message: "Old room cleanup has not finished.",
    };
    t.emit({
      destinations: ds,
      serverReservations: [
        ...t.current.serverReservations.filter(
          (r) => r.destinationId !== "dest:1",
        ),
        {
          destinationId: "dest:1",
          serverId: "guild:4",
          channelId: "room:5",
          sessionId: 2,
          phase: "selected",
        },
        {
          destinationId: "dest:1",
          serverId: "guild:1",
          channelId: "room:1",
          sessionId: 1,
          phase: "blocked",
        },
      ],
    });
  });
  await addPicker(page);
  await page.getByRole("button", { name: "Studio group", exact: true }).click();
  await expect(
    page.getByRole("option", { name: /Session room/ }),
  ).toHaveAttribute("aria-disabled", "true");
  await expect(
    page.getByText(/Finishing cleanup for Session room/),
  ).toBeVisible();
  await expect(page.getByText(/Finishing cleanup for Late room/)).toHaveCount(
    0,
  );
  await page.getByRole("option", { name: /Lounge/ }).press("Enter");
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([]);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      serverReservations: t.current.serverReservations.filter(
        (r) => r.serverId !== "guild:1",
      ),
    });
  });
  await expect(
    page.getByRole("option", { name: "Lounge", exact: true }),
  ).toHaveAttribute("aria-disabled", "false");
  await page
    .getByRole("option", { name: "Lounge", exact: true })
    .press("Enter");
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)),
  ).toEqual({
    type: "add-destination",
    serverId: "guild:1",
    channelId: "room:2",
  });
});

test("incoming reservations invalidate selected footer, Enter and double-click", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=connected");
  await addPicker(page);
  const option = page.getByRole("option", { name: /Сессия/ });
  await option.click();
  await expect(
    page.getByRole("button", { name: "Add Сессия", exact: true }),
  ).toBeEnabled();
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      serverReservations: [
        ...t.current.serverReservations,
        {
          serverId: "guild:2",
          channelId: "room:3",
          destinationId: "native:other",
          sessionId: 1,
          phase: "leaving",
        },
      ],
    });
  });
  await expect(
    page.getByRole("button", { name: "Add room", exact: true }),
  ).toBeDisabled();
  await expect(option).toHaveAttribute("aria-disabled", "true");
  await option.press("Enter");
  await option.dblclick({ force: true });
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([]);
});

test("incoming capacity and stale native selection errors remain readable in the picker", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=connected");
  await addPicker(page);
  await page.getByRole("option", { name: "Сессия", exact: true }).click();
  await page.evaluate(() => {
    window.uiHarness.transport.reject = {
      code: "invalid-selection",
      message:
        "Another destination now reserves this server. Choose a different server.",
    };
  });
  await page.getByRole("button", { name: "Add Сессия", exact: true }).click();
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
    "now reserves",
  );
  await expect(
    page.getByRole("button", { name: "Add Сессия", exact: true }),
  ).toBeFocused();
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const first = t.current.destinations[0];
    if (!first) throw Error("Missing room");
    t.emit({
      destinations: [
        first,
        ...Array.from({ length: 3 }, (_, i) => ({
          ...first,
          destinationId: `native:slot:${i}`,
          kind: "playback" as const,
          serverId: "",
          channelId: "",
          deviceId: `native:device:${i}`,
        })),
      ],
    });
  });
  await expect(page.getByText(/Four destinations are supported/)).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Add room", exact: true }),
  ).toBeDisabled();
  const option = page.getByRole("option", { name: /Сессия/ });
  await option.press("Enter");
  await option.dblclick({ force: true });
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.length),
  ).toBe(1);
});

test("shared connection errors follow native gateway state and keep source/playback controls reachable", async ({
  page,
}) => {
  await start(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const ds = structuredClone(t.current.destinations);
    const failed = ds.find((d) => d.destinationId === "dest:2");
    if (failed)
      failed.connection = {
        phase: "error",
        code: "permission-denied",
        message: "No Speak permission in this room.",
      };
    t.emit({ destinations: ds });
  });
  await expect(page.locator(".bot-chip")).toHaveAttribute(
    "data-gateway",
    "connected",
  );
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      gateway: {
        phase: "error",
        generation: 2,
        message:
          "Discord connection ended. Reconnect the affected rooms when ready.",
      },
    });
  });
  await expect(page.locator(".bot-chip")).toHaveAttribute(
    "data-gateway",
    "error",
  );
  await page.locator(".bot-chip").click();
  const shared = page.getByRole("region", {
    name: "Shared Discord connection",
  });
  await expect(shared.getByRole("alert")).toContainText(
    "Discord connection ended",
  );
  await expect(shared).toContainText("Connection error");
  await expect(page.getByText(/In Session room now/)).toBeVisible();
  await expect(page.getByText(/Reserved for Сессия/)).toBeVisible();
  await page.keyboard.press("Escape");
  const local = await show(page, "Preview headphones");
  await local.getByRole("button", { name: /^Stop / }).click();
  await local.getByRole("button", { name: /^Play / }).click();
  await page
    .locator(".source-card")
    .getByRole("button", { name: "Stop capturing Preview player" })
    .click();
  await expect(page.locator(".bot-chip")).toHaveAttribute(
    "data-gateway",
    "error",
  );
  await expect(
    page.locator('.dest-card[data-kind="discord"][data-state="live"]'),
  ).toHaveCount(2);
});

test("retained room cleanup locks token replacement even when cards and gateway are faulted", async ({
  page,
}) => {
  await start(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const ds = t.current.destinations.map((d) => ({
      ...d,
      connection: {
        phase: "error" as const,
        code: "connection-failed" as const,
        message: "Room connection ended.",
      },
    }));
    t.emit({
      destinations: ds,
      gateway: {
        phase: "error",
        generation: 2,
        message: "Shared connection ended.",
      },
    });
  });
  await page.locator(".bot-chip").click();
  const replace = page.getByRole("button", {
    name: "Replace token",
    exact: true,
  });
  await expect(replace).toBeEnabled();
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      serverReservations: t.current.serverReservations.map((r) =>
        r.destinationId === "dest:2" ? { ...r, phase: "blocked" } : r,
      ),
    });
  });
  await expect(replace).toBeDisabled();
  await expect(
    page.getByText("Cleanup incomplete for Сессия", { exact: true }),
  ).toBeVisible();
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      serverReservations: t.current.serverReservations.map((r) => ({
        ...r,
        phase: "selected",
      })),
    });
  });
  await expect(replace).toBeEnabled();
});

test("playback selection footer is invalidated when native output capacity fills", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=connected");
  await addPicker(page);
  await page.getByRole("tab", { name: "Playback", exact: true }).click();
  const option = page.getByRole("option", { name: /Preview headphones/ });
  await option.click();
  await expect(
    page.getByRole("button", { name: "Add Preview headphones", exact: true }),
  ).toBeEnabled();
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const first = t.current.destinations[0];
    if (!first) throw Error("Missing room");
    t.emit({
      destinations: [
        first,
        ...Array.from({ length: 3 }, (_, i) => ({
          ...first,
          destinationId: `native:room:${i}`,
          serverId: `native:server:${i}`,
          channelId: `native:channel:${i}`,
        })),
      ],
    });
  });
  await expect(
    page.getByRole("button", { name: "Choose a playback device", exact: true }),
  ).toBeDisabled();
  await expect(option).toHaveAttribute("aria-disabled", "true");
  await option.press("Enter");
  await option.dblclick({ force: true });
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([]);
});

test("changing and removing one live room preserves peer room sessions and playback", async ({
  page,
}) => {
  await start(page);
  const first = await show(page, "Session room");
  await first.getByRole("button", { name: /^Change room / }).click();
  await page.getByRole("option", { name: "Lounge", exact: true }).click();
  await expect(
    page.getByText("Briefly reconnects to change rooms"),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Move to Lounge", exact: true })
    .click();
  const changed = await show(page, "Lounge");
  await changed
    .getByRole("button", { name: "Remove Lounge", exact: true })
    .click();
  await expect(changed.getByRole("alertdialog")).toBeVisible();
  await changed
    .getByRole("button", { name: "Disconnect & remove", exact: true })
    .click();
  await expect(page.locator(".dest-card")).toHaveCount(3);
  const remaining = await page.evaluate(
    () => window.uiHarness.transport.current.destinations,
  );
  expect(
    remaining.every(
      (d) => d.sessionId === 1 && d.connection.phase === "connected",
    ),
  ).toBe(true);
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([
    {
      type: "change-destination",
      destinationId: "dest:1",
      serverId: "guild:1",
      channelId: "room:2",
    },
    { type: "remove-destination", destinationId: "dest:1" },
  ]);
  await expect(page.locator(".source-card")).toHaveAttribute(
    "data-state",
    "capturing",
  );
});

test("a replacement waits for its own old server cleanup before any retargeting", async ({
  page,
}) => {
  await start(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      serverReservations: [
        ...t.current.serverReservations,
        {
          destinationId: "dest:1",
          serverId: "guild:4",
          channelId: "room:5",
          sessionId: 0,
          phase: "leaving",
        },
      ],
    });
  });
  const first = await show(page, "Session room");
  await first.getByRole("button", { name: /^Change room / }).click();
  await expect(
    page.getByText(
      "Finish cleanup for Late room before changing this destination.",
      { exact: true },
    ),
  ).toBeVisible();
  const lounge = page.getByRole("option", { name: /Lounge/ });
  await expect(lounge).toHaveAttribute("aria-disabled", "true");
  await lounge.press("Enter");
  await lounge.dblclick({ force: true });
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([]);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      serverReservations: t.current.serverReservations.filter(
        (r) => r.serverId !== "guild:4",
      ),
    });
  });
  await expect(
    page.getByRole("option", { name: "Lounge", exact: true }),
  ).toHaveAttribute("aria-disabled", "false");
  await page
    .getByRole("option", { name: "Lounge", exact: true })
    .press("Enter");
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)),
  ).toEqual({
    type: "change-destination",
    destinationId: "dest:1",
    serverId: "guild:1",
    channelId: "room:2",
  });
});

test("fractional destination gain text stays on one row at every supported size", async ({
  page,
}) => {
  await start(page);
  await page.evaluate(async () => {
    await document.fonts.load(
      '400 12px "Sofia Sans Variable"',
      "−12.0 −60.0 dB",
    );
    await document.fonts.ready;
  });
  for (const [width, height] of [
    [440, 640],
    [455, 455],
    [420, 420],
    [760, 440],
    [1000, 640],
  ] as const) {
    await page.setViewportSize({ width, height });
    for (const volumeDb of [-12, -60]) {
      await page.evaluate((value) => {
        const t = window.uiHarness.transport;
        t.emit({
          destinations: t.current.destinations.map((d) => ({
            ...d,
            volumeDb: value,
          })),
        });
      }, volumeDb);
      await expect(page.locator(".dest-card .cf-gain").first()).toHaveText(
        volumeDb === -12 ? "−12.0 dB" : "−60.0 dB",
      );
      const rects = await page
        .locator(".dest-card .cf-values")
        .evaluateAll((values) =>
          values.map((el) => {
            const gain = el.querySelector(".cf-gain");
            const level = el
              .querySelector(".cf-level")
              ?.getBoundingClientRect();
            if (!gain || !level) return false;
            const range = document.createRange();
            range.selectNodeContents(gain);
            const text = range.getClientRects();
            const first = text[0];
            const box = el.getBoundingClientRect();
            return (
              !!first &&
              text.length === 1 &&
              first.left >= box.left - 1 &&
              first.right <= box.right + 1 &&
              first.bottom <= level.top + 1
            );
          }),
        );
      expect(rects).toEqual([true, true, true, true]);
    }
  }
});

test("the minimum picker shows the complete reserved room reason above its footer", async ({
  page,
}) => {
  await page.setViewportSize({ width: 420, height: 420 });
  await page.goto("/tests/harness.html?fixture=connected");
  await addPicker(page);
  await page.getByRole("button", { name: "Studio group", exact: true }).click();
  const note = page.getByText(
    "Another card reserves Session room. One room per server.",
    { exact: true },
  );
  await expect(note).toBeVisible();
  const fit = await note.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const list = el.closest(".room-list")?.getBoundingClientRect();
    return !!list && r.top >= list.top && r.bottom <= list.bottom;
  });
  expect(fit).toBe(true);
});

test("source and playback commands preserve blocked server lifetimes until explicit cleanup receipt", async ({
  page,
}) => {
  await start(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const ds = t.current.destinations.map((d) =>
      d.kind === "discord"
        ? {
            ...d,
            connection: {
              phase: "error" as const,
              code: "connection-failed" as const,
              message: "Room stopped after shared connection ended.",
            },
          }
        : d,
    );
    t.emit({
      destinations: ds,
      gateway: {
        phase: "error",
        generation: 2,
        message: "Shared connection ended.",
      },
      serverReservations: [
        ...t.current.serverReservations.map((r) => ({
          ...r,
          phase: "selected" as const,
        })),
        {
          destinationId: "dest:1",
          serverId: "guild:4",
          channelId: "room:5",
          sessionId: 0,
          phase: "blocked",
        },
      ],
    });
  });
  const local = await show(page, "Preview headphones");
  await local.getByRole("button", { name: /^Stop / }).click();
  await local.getByRole("button", { name: /^Play / }).click();
  const gain = page.getByRole("slider", { name: "Preview player gain" });
  await gain.press("ArrowLeft");
  await gain.blur();
  await expect
    .poll(() =>
      page.evaluate(() =>
        window.uiHarness.transport.commands.some(
          (c) => c.type === "set-source-gain",
        ),
      ),
    )
    .toBe(true);
  await page.locator(".bot-chip").click();
  const replace = page.getByRole("button", {
    name: "Replace token",
    exact: true,
  });
  await expect(replace).toBeDisabled();
  await expect(
    page.getByText("Cleanup incomplete for Late room", { exact: true }),
  ).toBeVisible();
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      serverReservations: t.current.serverReservations.filter(
        (r) => r.serverId !== "guild:4",
      ),
    });
  });
  await expect(replace).toBeEnabled();
});
