import { expect, type Page, test } from "@playwright/test";
import type { Harness } from "./harness";

async function openSetup(page: Page) {
  await page.goto("/tests/harness.html?fixture=first-run");
  await page.getByRole("button", { name: "Set up bot", exact: true }).click();
}

test("bot setup explains the required permissions and Enter saves without joining", async ({
  page,
}) => {
  await openSetup(page);
  const guide = page.locator(".setup-guide");
  await expect(guide).not.toHaveAttribute("open");
  await expect(page.getByLabel("Bot token", { exact: true })).toBeVisible();
  await page.getByText("Create and invite a bot", { exact: true }).click();
  await expect(guide).toHaveAttribute("open", "");
  await expect(guide).toContainText("View Channels, Connect and Speak");
  await expect(guide).toContainText("Manage Server");
  await expect(guide).toContainText(
    "Administrator and privileged gateway intents are not needed",
  );
  const portal = page.getByRole("link", { name: "Discord Developer Portal" });
  await expect(portal).toHaveAttribute(
    "href",
    "https://discord.com/developers/applications",
  );
  await expect(portal).toHaveAttribute("rel", "noopener noreferrer");
  await expect(
    page.getByRole("link", { name: "Full setup guide" }),
  ).toHaveAttribute(
    "href",
    "https://github.com/dokzlo13/echocord#discord-bot-setup",
  );
  await page
    .getByLabel("Bot token", { exact: true })
    .fill("synthetic-test-token");
  await page.getByLabel("Bot token", { exact: true }).press("Enter");
  await expect(page.getByText("is ready.", { exact: false })).toBeVisible();
  await expect(page.getByLabel("Bot token", { exact: true })).toHaveCount(0);
  expect(
    await page.evaluate(() => (window.uiHarness as Harness).transport.commands),
  ).toEqual([]);
});

test("native help sends only fixed keys and browser failures keep a usable public URL", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const host = window as unknown as {
      isTauri: boolean;
      __TAURI_INTERNALS__: unknown;
      helpTest: {
        calls: { name: string; args: unknown }[];
        fail: boolean;
      };
    };
    host.isTauri = true;
    host.helpTest = { calls: [], fail: false };
    host.__TAURI_INTERNALS__ = {
      invoke: async (name: string, args: unknown) => {
        host.helpTest.calls.push({ name, args });
        if (host.helpTest.fail)
          throw Error("untrusted internal failure detail");
      },
    };
  });
  await openSetup(page);
  await page
    .getByLabel("Bot token", { exact: true })
    .fill("synthetic-test-token");
  await page.getByText("Create and invite a bot", { exact: true }).click();
  await page.getByRole("link", { name: "Discord Developer Portal" }).click();
  await page.evaluate(() => {
    (window as unknown as { helpTest: { fail: boolean } }).helpTest.fail = true;
  });
  await page.getByRole("link", { name: "Full setup guide" }).click();
  await expect(page.getByRole("alert")).toContainText(
    "Could not open your browser. Open https://github.com/dokzlo13/echocord#discord-bot-setup manually.",
  );
  await expect(page.getByRole("alert")).not.toContainText("untrusted");
  expect(
    await page.evaluate(
      () =>
        (window as unknown as { helpTest: { calls: unknown[] } }).helpTest
          .calls,
    ),
  ).toEqual([
    { name: "open_setup_link", args: { link: "portal" } },
    { name: "open_setup_link", args: { link: "guide" } },
  ]);
  await expect(page.getByLabel("Bot token", { exact: true })).toHaveValue(
    "synthetic-test-token",
  );
});

test("bot server facts follow observed reservations without granting permissions", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=rooms");
  await page.locator(".bot-chip").click();
  const first = page.locator(".perm-row").first();
  await expect(first).toContainText("2 voice rooms found");
  await expect(first.locator("[data-reservation=connected]")).toHaveText(
    "In voice room",
  );
  await expect(page.locator(".perm-flags")).toHaveCount(0);
  await expect(
    page.getByText(/Room permissions are checked when you connect/),
  ).toBeVisible();
  await page.evaluate(() => {
    const t = (window.uiHarness as Harness).transport;
    t.emit({
      serverReservations: t.current.serverReservations.map((r, i) =>
        i === 0 ? { ...r, phase: "blocked" } : r,
      ),
    });
  });
  await expect(first.locator("[data-reservation=blocked]")).toHaveText(
    "Cleanup incomplete",
  );
  await expect(first).toContainText("Cleanup incomplete for Session room");
  await expect(
    page.getByRole("button", { name: "Replace token" }),
  ).toBeDisabled();
});

for (const [width, height] of [
  [440, 640],
  [455, 455],
  [420, 420],
  [760, 440],
  [1000, 640],
] as const) {
  test(`bot setup and server details preserve compact geometry and focus at ${width}x${height}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await openSetup(page);
    const token = page.getByLabel("Bot token", { exact: true });
    await expect(token).toBeInViewport();
    await token.fill("synthetic-test-token");
    const summary = page.getByText("Create and invite a bot", { exact: true });
    await summary.focus();
    await page.keyboard.press("Enter");
    await expect(page.locator(".setup-guide")).toHaveAttribute("open", "");
    await page.keyboard.press("Tab");
    await expect(
      page.getByRole("link", { name: "Discord Developer Portal" }),
    ).toBeFocused();
    const save = page.getByRole("button", { name: "Test & save", exact: true });
    await expect(save).toBeInViewport();
    await page.getByRole("button", { name: "Back", exact: true }).focus();
    await page.keyboard.press("Shift+Tab");
    await expect(save).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(
      page.getByRole("button", { name: "Back", exact: true }),
    ).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(
      page.getByRole("button", { name: "Set up bot", exact: true }),
    ).toBeFocused();

    await page.goto("/tests/harness.html?fixture=rooms");
    await page.evaluate(() => {
      const t = (window.uiHarness as Harness).transport;
      t.emit({
        servers: t.current.servers.map((s, i) =>
          i === 0
            ? {
                ...s,
                name: "Очень длинное имя сервера для проверки переноса строки",
              }
            : s,
        ),
      });
    });
    await page.locator(".bot-chip").click();
    await expect(page.locator(".perm-row").first()).toContainText(
      "Очень длинное имя сервера",
    );
    await expect(
      page.getByRole("button", { name: "Check again" }),
    ).toBeInViewport();
    expect(
      await page.getByRole("dialog").evaluate((dialog) => {
        const box = dialog.getBoundingClientRect();
        const footer = dialog
          .querySelector(".sheet-foot")
          ?.getBoundingClientRect();
        const body = dialog.querySelector(".sheet-body");
        return (
          box.x === 0 &&
          box.y === 36 &&
          box.right <= innerWidth &&
          box.bottom <= innerHeight &&
          !!footer &&
          footer.bottom <= innerHeight &&
          !!body &&
          body.scrollWidth <= body.clientWidth &&
          document.documentElement.scrollHeight <= innerHeight
        );
      }),
    ).toBe(true);
  });
}
