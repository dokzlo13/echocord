import { expect, type Locator, type Page, test } from "@playwright/test";

async function holdHistoryClock(page: Page) {
  // Freeze before telemetry loads, with room for clock installation to finish.
  // Sparse fixture emissions must not create a real history gap during edits.
  await page.clock.install({ time: new Date("2026-01-01T00:00:00Z") });
  await page.clock.pauseAt(new Date("2026-01-01T01:00:00Z"));
}

async function seedObservedHistory(page: Page) {
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.stopTelemetry();
    const inputs = structuredClone(t.current.inputs);
    const destinations = structuredClone(t.current.destinations);
    t.emit({
      inputs: inputs.map((v) => ({ ...v, phase: "capturing" })),
      destinations: destinations.map((v) => ({
        ...v,
        connection: { phase: "connected" },
      })),
    });
    t.observeTelemetry(
      {
        ...t.current.telemetry,
        inputs: t.current.telemetry.inputs.map((v) => ({
          ...v,
          levelDbfs: -18,
        })),
        destinations: t.current.telemetry.destinations.map((v) => ({
          ...v,
          levelDbfs: -24,
        })),
      },
      t.current.telemetry.clockMs + 1,
    );
    t.emit({ inputs, destinations });
  });
}

async function numbers(section: Locator, expected: string[]) {
  await expect(section.locator(".identity-row > .ch")).toHaveText(expected);
  await expect(section.locator(".chips .chip")).toHaveText(expected);
  await expect(section.getByRole("navigation")).toHaveCount(
    expected.length > 1 ? 1 : 0,
  );
}

test("empty, single and added cards reveal numbering independently per section", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=first-run");
  const sources = page.getByRole("region", { name: "Sources", exact: true });
  const destinations = page.getByRole("region", { name: "Destinations" });
  await numbers(sources, []);
  await numbers(destinations, []);
  await sources.getByRole("button", { name: "Add source" }).first().click();
  await page.getByRole("option", { name: /Preview player/ }).press("Enter");
  await expect(sources.getByRole("article")).toHaveAccessibleName(
    "Source: Preview player",
  );
  await expect(sources.locator(".slot-summary")).toHaveText("on");
  await numbers(sources, []);
  await sources
    .getByRole("button", { name: "Mute Preview player in the mix" })
    .click();
  await expect(sources.locator(".slot-summary")).toHaveText("muted");
  await sources
    .getByRole("button", { name: "Stop capturing Preview player" })
    .click();
  await expect(sources.locator(".slot-summary")).toHaveText("off");

  await destinations
    .getByRole("button", { name: "Add destination" })
    .first()
    .click();
  await page.getByRole("tab", { name: "Playback", exact: true }).click();
  await page.getByRole("option", { name: /Preview headphones/ }).press("Enter");
  await expect(destinations.getByRole("article")).toHaveAccessibleName(
    "Destination: Preview headphones, playback output, Stopped",
  );
  await numbers(destinations, []);
  await destinations.getByRole("button", { name: "Add destination" }).click();
  await page.getByRole("tab", { name: "Playback", exact: true }).click();
  await page.getByRole("option", { name: /Preview speakers/ }).press("Enter");
  await numbers(destinations, ["1", "2"]);
  await numbers(sources, []);

  await sources.getByRole("button", { name: "Add source" }).click();
  await page.getByRole("option", { name: /Preview browser/ }).press("Enter");
  await numbers(sources, ["1", "2"]);
  await expect(sources.locator(".slot-summary")).toHaveText("1 on · 1 off");
  await destinations
    .getByRole("button", { name: "Remove Preview headphones" })
    .click();
  await numbers(destinations, []);
  await expect(destinations.getByRole("article")).toHaveAccessibleName(
    "Destination: Preview speakers, playback output, Stopped",
  );
  await numbers(sources, ["1", "2"]);
  await destinations
    .getByRole("button", { name: "Remove Preview speakers" })
    .click();
  await expect(destinations.getByText("No destinations yet")).toBeVisible();
  await sources.getByRole("button", { name: "Remove Preview player" }).click();
  await numbers(sources, []);
  await expect(sources.getByRole("article")).toHaveAccessibleName(
    "Source: Preview browser",
  );
  await sources.getByRole("button", { name: "Remove Preview browser" }).click();
  await expect(sources.getByText("No sources yet")).toBeVisible();
  expect(
    await page.evaluate(() =>
      window.uiHarness.transport.commands.some(
        (command) => command.type === "connect-destination",
      ),
    ),
  ).toBe(false);
});

test("source ordinals compact after middle and first removal while colour, identity and history survive", async ({
  page,
}) => {
  await holdHistoryClock(page);
  await page.goto("/tests/harness.html?fixture=many");
  const sources = page.getByRole("region", { name: "Sources", exact: true });
  const survivor = page.locator('[id="card-strip:3"]');
  await expect(survivor).toHaveAttribute("data-colour", "2");
  const node = await survivor.elementHandle();
  await seedObservedHistory(page);
  const history = await page.evaluateHandle(() => {
    const h = window.uiHarness;
    h.adapter.telemetry.ingest(h.transport.current.telemetry);
    return h.adapter.telemetry.history("src:strip:3");
  });
  expect(await history.evaluate((ring) => !!ring)).toBe(true);
  await sources.getByRole("button", { name: "Remove Preview browser" }).click();
  await numbers(sources, ["1", "2"]);
  await expect(survivor).toHaveAccessibleName(
    "Source 2: Preview recording input, stopped",
  );
  const jump = page.getByRole("button", {
    name: "Show source 2: Preview recording input",
  });
  await jump.focus();
  await jump.press("Enter");
  await expect(survivor.locator(".identity")).toBeFocused();
  await sources.getByRole("button", { name: "Remove Preview player" }).click();
  await numbers(sources, []);
  await expect(survivor).toHaveAccessibleName(
    "Source: Preview recording input, stopped",
  );
  await expect(survivor).toHaveAttribute("data-colour", "2");
  expect(await survivor.evaluate((el, original) => el === original, node)).toBe(
    true,
  );
  expect(
    await page.evaluate(
      (original) =>
        window.uiHarness.adapter.telemetry.history("src:strip:3") === original,
      history,
    ),
  ).toBe(true);
  await survivor
    .getByRole("button", {
      name: "Capture Preview recording input",
      exact: true,
    })
    .click();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([
    { type: "remove-source", stripId: "strip:2" },
    { type: "remove-source", stripId: "strip:1" },
    { type: "start-capture", stripId: "strip:3" },
  ]);
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "live",
  );
});

test("source card and jump colours reuse a freed slot through churn while peers stay unchanged", async ({
  page,
}) => {
  await holdHistoryClock(page);
  await page.goto("/tests/harness.html?fixture=many");
  const sources = page.getByRole("region", { name: "Sources", exact: true });
  await sources
    .getByRole("button", { name: "Unmute Preview browser in the mix" })
    .click();
  const peer = page.locator('[id="card-strip:2"]');
  const node = await peer.elementHandle();
  await seedObservedHistory(page);
  const history = await page.evaluateHandle(() => {
    const h = window.uiHarness;
    h.adapter.telemetry.ingest(h.transport.current.telemetry);
    return h.adapter.telemetry.history("src:strip:2");
  });
  expect(await history.evaluate((ring) => !!ring)).toBe(true);
  for (let cycle = 0; cycle < 7; cycle++) {
    await sources
      .getByRole("button", { name: "Remove Preview player" })
      .click();
    // Fixture identities use the clock; keep IDs distinct without introducing
    // a telemetry gap while exercising repeated removal/addition.
    await page.clock.fastForward(1);
    await sources.getByRole("button", { name: "Add source" }).click();
    await page.getByRole("option", { name: /Preview player/ }).press("Enter");
    const added = sources.getByRole("article", {
      name: "Source 3: Preview player",
      exact: true,
    });
    await expect(added).toHaveAttribute("data-colour", "0");
    await expect(added.locator(".identity-row > .ch")).toHaveCSS(
      "background-color",
      "rgb(57, 135, 229)",
    );
    await expect(
      sources.getByRole("button", { name: "Show source 3: Preview player" }),
    ).toHaveCSS("background-color", "rgb(57, 135, 229)");
    await expect(peer).toHaveAttribute("data-colour", "1");
    await expect(peer.locator(".identity-row > .ch")).toHaveCSS(
      "background-color",
      "rgb(25, 158, 112)",
    );
    await expect(
      sources.getByRole("button", { name: "Show source 1: Preview browser" }),
    ).toHaveCSS("background-color", "rgb(25, 158, 112)");
  }
  expect(await peer.evaluate((el, original) => el === original, node)).toBe(
    true,
  );
  expect(
    await page.evaluate(
      (original) =>
        window.uiHarness.adapter.telemetry.history("src:strip:2") === original,
      history,
    ),
  ).toBe(true);
  await numbers(sources, ["1", "2", "3"]);
  await expect(page.locator(".dest-card")).toHaveAttribute(
    "data-state",
    "live",
  );
});

test("destination ordinals compact without remounting or resetting a surviving room", async ({
  page,
}) => {
  await holdHistoryClock(page);
  await page.goto("/tests/harness.html?fixture=rooms");
  const destinations = page.getByRole("region", { name: "Destinations" });
  const survivor = page.locator('[id="card-dest:3"]');
  await expect(survivor).toHaveAttribute("data-state", "live");
  const node = await survivor.elementHandle();
  await seedObservedHistory(page);
  const history = await page.evaluateHandle(() => {
    const h = window.uiHarness;
    h.adapter.telemetry.ingest(h.transport.current.telemetry);
    return h.adapter.telemetry.history("dst:dest:3");
  });
  expect(await history.evaluate((ring) => !!ring)).toBe(true);
  const network = await page.evaluateHandle(() =>
    window.uiHarness.adapter.telemetry.net("dest:3"),
  );
  for (const name of ["Сессия", "Session room", "Preview headphones"]) {
    await destinations
      .getByRole("button", { name: `Remove ${name}`, exact: true })
      .click();
    await page
      .getByRole("button", {
        name:
          name === "Preview headphones"
            ? "Stop & remove"
            : "Disconnect & remove",
        exact: true,
      })
      .click();
    const remaining =
      3 - ["Сессия", "Session room", "Preview headphones"].indexOf(name);
    await numbers(
      destinations,
      remaining > 1
        ? Array.from({ length: remaining }, (_, i) => `${i + 1}`)
        : [],
    );
  }
  await expect(survivor).toHaveAccessibleName(
    "Destination: Listening room on Listening group, On air",
  );
  await expect(survivor).toHaveAttribute("data-state", "live");
  expect(await survivor.evaluate((el, original) => el === original, node)).toBe(
    true,
  );
  expect(
    await page.evaluate(
      (original) =>
        window.uiHarness.adapter.telemetry.history("dst:dest:3") === original,
      history,
    ),
  ).toBe(true);
  expect(
    await page.evaluate(
      (original) =>
        window.uiHarness.adapter.telemetry.net("dest:3") === original,
      network,
    ),
  ).toBe(true);
  await survivor
    .getByRole("button", {
      name: "Mute Listening room on Listening group",
      exact: true,
    })
    .click();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([
    { type: "remove-destination", destinationId: "dest:2" },
    { type: "remove-destination", destinationId: "dest:1" },
    { type: "remove-destination", destinationId: "local:1" },
    { type: "set-destination-muted", destinationId: "dest:3", muted: true },
  ]);
});

test("long duplicate identities retain distinct section, ordinal and server names for keyboard jumps", async ({
  page,
}) => {
  await page.setViewportSize({ width: 420, height: 420 });
  await page.goto("/tests/harness.html?fixture=many");
  const name =
    "Очень длинное одинаковое название источника и голосовой комнаты";
  await page.evaluate((name) => {
    const t = window.uiHarness.transport;
    const first = t.current.destinations[0];
    if (!first) throw Error("Missing fixture room");
    t.emit({
      sources: t.current.sources.map((source) => ({ ...source, name })),
      servers: t.current.servers.map((server) => ({
        ...server,
        channels: server.channels.map((channel) => ({ ...channel, name })),
      })),
      destinations: [
        first,
        {
          ...first,
          destinationId: "dest:2",
          serverId: "guild:2",
          channelId: "room:3",
        },
      ],
    });
  }, name);
  for (const [label, id] of [
    [`Show source 3: ${name}`, "strip:3"],
    [`Show destination 2: ${name} on Вечерняя группа`, "dest:2"],
  ] as const) {
    const button = page.getByRole("button", { name: label });
    await button.focus();
    await button.press("Enter");
    const card = page.locator(`[id="card-${id}"]`);
    await expect(card.locator(".identity")).toBeFocused();
    await expect(card).toHaveAccessibleName(
      new RegExp(
        id === "strip:3" ? "^Source 3:" : "^Destination 2:.*Вечерняя группа",
      ),
    );
    expect(
      await card.evaluate((el) => {
        const row = el.querySelector(".identity-row");
        if (!row) return false;
        const bounds = el.getBoundingClientRect();
        return [...row.children].every((child) => {
          const rect = child.getBoundingClientRect();
          return rect.left >= bounds.left && rect.right <= bounds.right + 1;
        });
      }),
    ).toBe(true);
  }
  await expect(page.locator(".dest-card .server-tile")).toHaveCount(2);
  expect(
    await page.evaluate(() => [
      document.documentElement.scrollWidth,
      document.documentElement.scrollHeight,
    ]),
  ).toEqual([420, 420]);
});
