import { expect, test } from "@playwright/test";

test("offline licenses keep selection while filtering and retrying, render text safely and return focus", async ({
  page,
}) => {
  const documents = [
    { title: "Echocord MIT", path: "/licenses/project.txt" },
    { title: "Example Apache", path: "/licenses/example.txt" },
  ];
  let attempts = 0;
  await page.route("**/licenses/catalog.json", (route) =>
    route.fulfill({
      json: {
        version: "fixture",
        sourceUrl: "https://example.test",
        documents,
      },
    }),
  );
  await page.route("**/licenses/project.txt", (route) =>
    route.fulfill({ body: "MIT fixture license" }),
  );
  await page.route("**/licenses/example.txt", (route) =>
    route.fulfill(
      ++attempts === 1
        ? { status: 500 }
        : {
            body: "<script>window.licenseExecuted = true</script>\nApache fixture license",
          },
    ),
  );
  await page.goto("/tests/harness.html?fixture=connected");
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "Licenses", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "Echocord MIT" })).toHaveValue(
    "MIT fixture license",
  );
  await page.getByRole("searchbox", { name: "Find a license" }).fill("Apache");
  await expect(page.getByRole("textbox", { name: "Echocord MIT" })).toHaveValue(
    "MIT fixture license",
  );
  await page
    .getByRole("combobox", { name: "License document" })
    .selectOption({ label: "Example Apache" });
  await expect(page.getByRole("alert")).toContainText(
    "Could not load this license",
  );
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(
    page.getByRole("textbox", { name: "Example Apache" }),
  ).toHaveValue(/<script>/);
  expect(await page.evaluate(() => "licenseExecuted" in window)).toBe(false);
  await page.getByRole("searchbox").fill("no matching package");
  await expect(page.locator(".license-count")).toHaveText(
    "0 matching documents",
  );
  await expect(
    page.getByRole("textbox", { name: "Example Apache" }),
  ).toHaveValue(/Apache fixture/);
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("dialog", { name: "Settings", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Licenses", exact: true }),
  ).toBeFocused();
});
