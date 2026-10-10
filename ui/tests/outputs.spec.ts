import { expect, type Page, test } from "@playwright/test";

async function start(page: Page, fixture = "connected") {
  await page.goto(`/tests/harness.html?fixture=${fixture}`);
  await expect(page.locator(".window")).toBeVisible();
}
async function addPlayback(page: Page, name = "Preview headphones") {
  await page
    .getByRole("button", { name: "Add destination", exact: true })
    .last()
    .click();
  await page.getByRole("tab", { name: "Playback", exact: true }).click();
  await page.getByRole("option", { name: new RegExp(name) }).press("Enter");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  return page.locator('.dest-card[data-kind="playback"]').filter({
    has: page.getByRole("button", { name: `Change ${name}`, exact: true }),
  });
}

test("first-run playback needs no bot and is idle until explicit Play", async ({
  page,
}) => {
  await start(page, "first-run");
  const card = await addPlayback(page);
  await expect(card).toHaveAttribute("data-state", "idle");
  await expect(
    card.getByRole("button", { name: "Play", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(() =>
      window.uiHarness.transport.commands.map((c) => c.type),
    ),
  ).toEqual(["add-playback-destination"]);
  await card.getByRole("button", { name: "Play", exact: true }).click();
  await expect(
    card.getByRole("button", { name: "Playing", exact: true }),
  ).toBeVisible();
  await expect(
    card.getByRole("button", { name: "Connect", exact: true }),
  ).toHaveCount(0);
  await expect(card.getByText("On air", { exact: true })).toHaveCount(0);
});

test("playback Stop mute gain remove and endpoint replacement target only its card", async ({
  page,
}) => {
  await start(page);
  const local = await addPlayback(page);
  await local.getByRole("button", { name: "Play", exact: true }).click();
  await local
    .getByRole("button", { name: "Mute Preview headphones", exact: true })
    .click();
  await local
    .getByRole("slider", { name: "Preview headphones volume" })
    .press("ArrowLeft");
  await local.getByRole("slider", { name: "Preview headphones volume" }).blur();
  await local
    .getByRole("button", { name: "Change Preview headphones", exact: true })
    .click();
  await expect(
    page.getByRole("tab", { name: "Discord", exact: true }),
  ).toBeDisabled();
  await page.getByRole("option", { name: /Preview speakers/ }).press("Enter");
  await expect(page.locator('.dest-card[data-kind="playback"]')).toContainText(
    "Preview speakers",
  );
  const current = page.locator('.dest-card[data-kind="playback"]');
  await current.getByRole("button", { name: "Stop", exact: true }).click();
  await current
    .getByRole("button", { name: "Remove Preview speakers", exact: true })
    .click();
  await expect(page.locator('.dest-card[data-kind="discord"]')).toHaveAttribute(
    "data-state",
    "live",
  );
  const commands = await page.evaluate(
    () => window.uiHarness.transport.commands,
  );
  expect(
    commands.some(
      (c) =>
        c.type === "disconnect-destination" && c.destinationId === "dest:1",
    ),
  ).toBe(false);
  expect(commands.some((c) => c.type === "change-playback-destination")).toBe(
    true,
  );
});

test("local playback and discovery remain usable during Discord join and REST failure", async ({
  page,
}) => {
  await start(page, "connecting");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      bot: { status: "error", name: null, message: "Could not reach Discord." },
      discovery: { phase: "error", message: "Room discovery failed." },
    });
  });
  const local = await addPlayback(page);
  await local.getByRole("button", { name: "Play", exact: true }).click();
  await expect(local).toHaveAttribute("data-state", "live");
  await expect(page.locator('.dest-card[data-kind="discord"]')).toHaveAttribute(
    "data-state",
    "connecting",
  );
  await local.getByRole("button", { name: "Stop", exact: true }).click();
  expect(
    await page.evaluate(() =>
      window.uiHarness.transport.commands.some(
        (c) => c.type === "recheck-configuration",
      ),
    ),
  ).toBe(false);
});

test("token replacement is available while only playback runs", async ({
  page,
}) => {
  await start(page, "disconnected");
  const local = await addPlayback(page);
  await local.getByRole("button", { name: "Play", exact: true }).click();
  await page.getByRole("button", { name: "Preview bot", exact: false }).click();
  await expect(
    page.getByRole("button", { name: "Replace token", exact: true }),
  ).toBeEnabled();
});

test("playback picker uses actual availability and duplicate limits with relevant routing notes", async ({
  page,
}) => {
  await start(page, "many");
  await addPlayback(page);
  await page
    .getByRole("button", { name: "Add destination", exact: true })
    .click();
  await page.getByRole("tab", { name: "Playback", exact: true }).click();
  await expect(
    page.getByRole("option", { name: /Preview headphones/ }),
  ).toHaveAttribute("aria-disabled", "true");
  await expect(
    page.getByRole("option", { name: /Unavailable output/ }),
  ).toHaveAttribute("aria-disabled", "true");
  await expect(
    page.getByText(/Original app playback still follows Windows routing/),
  ).toBeVisible();
  await expect(
    page.getByText(/captured cable back into its paired/),
  ).toBeVisible();
  await page.getByRole("button", { name: "Refresh", exact: true }).click();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)?.type),
  ).toBe("refresh-playback-devices");
});

test("local faults and unconfirmed Stop preserve other outputs and expose Retry stop", async ({
  page,
}) => {
  await start(page, "outputs");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const ds = structuredClone(t.current.destinations);
    const d = ds.find((d) => d.destinationId === "local:1");
    if (d)
      d.connection = {
        phase: "cleanup-incomplete",
        message: "Playback cleanup has not finished. Retry stop.",
      };
    t.emit({ destinations: ds });
  });
  const first = page.locator('.dest-card[data-kind="playback"]').first();
  await expect(
    first.getByRole("button", { name: "Retry stop", exact: true }),
  ).toBeVisible();
  await expect(page.locator('.dest-card[data-kind="discord"]')).toHaveAttribute(
    "data-state",
    "live",
  );
  await page
    .getByRole("button", { name: "Show Preview headphones", exact: true })
    .click();
  await first.getByRole("button", { name: "Retry stop", exact: true }).click();
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands.at(-1)),
  ).toEqual({ type: "disconnect-destination", destinationId: "local:1" });
});

test("playback details never contain Discord or gateway metrics", async ({
  page,
}) => {
  await start(page);
  const local = await addPlayback(page);
  await local.getByRole("button", { name: "Play", exact: true }).click();
  await local.getByRole("button", { name: "Playing", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: /Playback output details/ });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByText("Gateway", { exact: true })).toHaveCount(0);
  await expect(
    sheet.getByText(/Opus|Voice server|Discord bot|On air/),
  ).toHaveCount(0);
  await expect(
    sheet.getByText(/does not prove physical audibility/),
  ).toBeVisible();
});

test("four destination limit and restored output cards never start automatically", async ({
  page,
}) => {
  await start(page, "outputs");
  await expect(
    page.getByRole("button", { name: "Add destination", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Add destination", exact: true }),
  ).toHaveAttribute("title", "Four destinations for now");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      destinations: t.current.destinations.map((d) => ({
        ...d,
        connection: { phase: "disconnected" },
      })),
    });
  });
  await expect(page.locator('.dest-card[data-state="live"]')).toHaveCount(0);
  expect(
    await page.evaluate(() => window.uiHarness.transport.commands),
  ).toEqual([]);
});

for (const [w, h] of [
  [440, 640],
  [455, 455],
  [420, 420],
  [760, 440],
  [1000, 640],
] as const)
  test(`four outputs preserve whole card controls at ${w}x${h}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: w, height: h });
    await start(page, "outputs");
    const fit = await page.evaluate(() => {
      const src = document
        .querySelector(".source-card")
        ?.getBoundingClientRect();
      const sl = document.querySelector(".src-list")?.getBoundingClientRect();
      const dst = document.querySelector(".dest-card")?.getBoundingClientRect();
      const dl = document.querySelector(".dst-list")?.getBoundingClientRect();
      return {
        source: !!src && !!sl && src.bottom <= sl.bottom + 1,
        destination: !!dst && !!dl && dst.bottom <= dl.bottom + 1,
        doc: [
          document.documentElement.scrollWidth,
          document.documentElement.scrollHeight,
        ],
      };
    });
    expect(fit).toEqual({ source: true, destination: true, doc: [w, h] });
    await page
      .getByRole("button", { name: "Show Preview cable input", exact: true })
      .click();
    await expect(
      page.getByRole("button", {
        name: "Change Preview cable input",
        exact: true,
      }),
    ).toBeFocused();
  });

test("wide two-source histories fill the column like the actual prototype", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 640 });
  await start(page, "restored");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({ inputs: t.current.inputs.slice(0, 2) });
  });
  await expect(page.locator(".source-card[data-expand]")).toHaveCount(2);
  await expect(page.locator(".src-history")).toHaveCount(2);
  const heights = await page
    .locator(".source-card")
    .evaluateAll((cards) =>
      cards.slice(0, 2).map((e) => e.getBoundingClientRect().height),
    );
  expect(heights.every((h) => h > 240)).toBe(true);
});

test("observed identity notices distinguish pending selection from successful capture", async ({
  page,
}) => {
  await start(page);
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const inputs = structuredClone(t.current.inputs);
    const s = inputs[0];
    if (s) {
      s.sourceId = "app:browser:2";
      s.phase = "opening";
      s.displayName = "Preview browser";
      s.detail = "Two windows";
    }
    t.emit({ inputs });
  });
  await expect(
    page.locator(".source-card").first().locator(".id-detail"),
  ).toHaveText("Selected instead of Preview player");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    const inputs = structuredClone(t.current.inputs);
    if (inputs[0]) inputs[0].phase = "capturing";
    t.emit({ inputs });
  });
  await expect(
    page.locator(".source-card").first().locator(".id-detail"),
  ).toHaveText("Switched from Preview player");
});
