import { expect, test } from "@playwright/test";

test("live settings remain applied when saving fails and retry saves without replaying controls", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=connected");
  await page.evaluate(() => {
    window.uiHarness.transport.failPreferences = true;
  });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const tray = page.getByRole("switch", { name: /Keep running/ });
  await tray.uncheck();
  await expect(tray).not.toBeChecked();
  await expect(
    page.getByRole("dialog").getByRole("region", { name: "Save status" }),
  ).toContainText("Changes apply now");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Retry save" })
    .click();
  await expect(page.getByRole("dialog")).toContainText("Changes not saved");
  await expect(page.getByRole("status")).toContainText("Still not saved");
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "Settings", exact: true }),
  ).toBeFocused();
  await expect(page.locator(".save-strip")).toContainText("Changes not saved");
  await page.getByRole("button", { name: "Details", exact: true }).click();
  await expect(
    page.getByRole("dialog").getByRole("button", { name: "Retry save" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "Details", exact: true }),
  ).toBeFocused();
  await page.getByRole("button", { name: "Details", exact: true }).click();
  await page.evaluate(() => {
    window.uiHarness.transport.failPreferences = false;
  });
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Retry save" })
    .click();
  await expect(page.getByRole("region", { name: "Save status" })).toHaveCount(
    0,
  );
  await expect(tray).not.toBeChecked();
  await expect(page.getByRole("status")).toContainText("Changes saved");
  await expect(
    page.getByRole("button", { name: "Back", exact: true }),
  ).toBeFocused();
  const commands = await page.evaluate(
    () => window.uiHarness.transport.commands,
  );
  expect(
    commands.filter((command) => command.type === "set-settings"),
  ).toHaveLength(1);
  expect(
    commands.filter((command) => command.type === "retry-preferences"),
  ).toHaveLength(2);
});

for (const [width, height] of [
  [440, 640],
  [455, 455],
  [420, 420],
  [760, 440],
  [1000, 640],
] as const) {
  test(`unsaved status preserves controls at ${width}x${height}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await page.goto("/tests/harness.html?fixture=connected");
    await page.evaluate(() =>
      window.uiHarness.transport.emit({
        preferencesWarning: {
          code: "configuration",
          message: "Preferences could not be saved.",
        },
      }),
    );
    await expect(
      page.getByRole("button", { name: "Retry save" }),
    ).toBeInViewport();
    await expect(
      page.locator(".source-card").getByRole("button", { name: /^Mute / }),
    ).toBeInViewport();
    await expect(
      page.locator(".dest-card").getByRole("button", { name: /^Disconnect / }),
    ).toBeInViewport();
    expect(
      await page.evaluate(
        () =>
          document.documentElement.scrollHeight <= innerHeight &&
          document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: `../artifacts/browser-review/save-${width}x${height}.png`,
    });
  });
}

test("background save progress stays pending until the native writer finishes", async ({
  page,
}) => {
  await page.goto("/tests/harness.html?fixture=connected");
  await page.evaluate(() =>
    window.uiHarness.transport.emit({
      preferencesSaving: true,
      preferencesWarning: {
        code: "configuration",
        message: "Preferences could not be saved.",
      },
    }),
  );
  const saving = page.getByRole("button", { name: "Saving…", exact: true });
  await expect(saving).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByRole("status")).toHaveText("Saving changes.");
  await page.evaluate(() =>
    window.uiHarness.transport.emit({ preferencesSaving: false }),
  );
  await expect(
    page.getByRole("button", { name: "Retry save", exact: true }),
  ).toBeEnabled();
  await expect(page.getByRole("status")).toContainText("Still not saved");
});
