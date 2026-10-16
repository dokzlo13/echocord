import { expect, type Page, test } from "@playwright/test";
import type { ConnectStage } from "../src/generated";

async function joining(
  page: Page,
  meterStyle: "simple" | "history",
  multiple = true,
) {
  await page.goto("/tests/harness.html?fixture=connected");
  await expect(page.locator(".dest-card")).toBeVisible();
  await page.evaluate(
    ({ meterStyle, multiple }) => {
      const t = window.uiHarness.transport;
      const first = t.current.destinations[0];
      if (!first) throw Error("Missing room fixture");
      const target = {
        ...first,
        destinationId: multiple ? "dest:2" : first.destinationId,
        sessionId: 2,
        serverId: multiple ? "guild:2" : first.serverId,
        channelId: multiple ? "room:3" : first.channelId,
        connection: {
          phase: "connecting" as const,
          stage: "checking" as const,
        },
      };
      t.emit({
        settings: { ...t.current.settings, meterStyle },
        destinations: multiple ? [first, target] : [target],
      });
    },
    { meterStyle, multiple },
  );
  await page.evaluate(async () => {
    await Promise.all([
      document.fonts.load('400 13px "Sofia Sans Variable"'),
      document.fonts.load('700 16px "Sofia Sans Condensed Variable"'),
    ]);
    await document.fonts.ready;
  });
  return page.locator(".dest-card").last();
}
async function stage(page: Page, value: ConnectStage) {
  await page.evaluate((value) => {
    const t = window.uiHarness.transport;
    const ds = structuredClone(t.current.destinations);
    const target = ds.at(-1);
    if (!target) throw Error("Missing joining room");
    target.connection = { phase: "connecting", stage: value };
    t.emit({ destinations: ds });
  }, value);
}
const stages = [
  ["checking", 0, "Checking the bot"],
  ["preflight", 0, "Checking the bot"],
  ["gateway", 1, "Reaching Discord"],
  ["join", 2, "Joining"],
  ["capture", 3, "Starting audio"],
] as const;
for (const [width, height] of [
  [440, 640],
  [455, 455],
  [420, 420],
  [760, 440],
  [1000, 640],
] as const)
  for (const style of ["simple", "history"] as const)
    test(`live peer and joining room show compact segments at ${width}x${height} ${style}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height });
      const target = await joining(page, style);
      await page
        .getByRole("button", {
          name: "Show destination 2: Сессия on Вечерняя группа",
        })
        .click();
      const compact = target.locator(".d-stage-line");
      for (const [native, index, label] of stages) {
        await stage(page, native);
        await expect(compact).toBeVisible();
        await expect(target.locator(".d-log")).toBeHidden();
        await expect(compact.locator(".stage-line")).toContainText(label);
        await expect(compact.locator('[aria-current="step"]')).toHaveCount(1);
        await expect(compact.locator('[aria-current="step"]')).toHaveText(
          ["Bot", "Discord", "Room", "Audio"][index] ?? "",
        );
        const fit = await compact.evaluate((el) => {
          const list = el.querySelector("ol");
          const steps = Array.from(el.querySelectorAll("li"));
          const boxes = steps.map((step) => step.getBoundingClientRect());
          const controls = el
            .closest(".card")
            ?.querySelector(".controls-row")
            ?.getBoundingClientRect();
          const card = el.closest(".card")?.getBoundingClientRect();
          const body = el.getBoundingClientRect();
          return (
            !!list &&
            !!controls &&
            !!card &&
            getComputedStyle(list).listStyleType === "none" &&
            getComputedStyle(list).display === "flex" &&
            boxes.every(
              (box) =>
                Math.abs(box.width - 14) < 1 && box.top === boxes[0]?.top,
            ) &&
            steps.every(
              (step) => getComputedStyle(step).borderTopWidth === "3px",
            ) &&
            controls.bottom <= body.top &&
            body.bottom <= card.bottom &&
            document.documentElement.scrollWidth === innerWidth &&
            document.documentElement.scrollHeight === innerHeight
          );
        });
        expect(fit).toBe(true);
        await expect(page.locator(".dest-card").first()).toHaveAttribute(
          "data-state",
          "live",
        );
        await expect(
          target.getByRole("button", { name: /^Cancel / }),
        ).toBeVisible();
      }
      await target.getByRole("button", { name: /^Cancel / }).click();
      await expect(page.locator(".dest-card").first()).toHaveAttribute(
        "data-state",
        "live",
      );
      expect(
        await page.evaluate(() => window.uiHarness.transport.commands.at(-1)),
      ).toEqual({ type: "disconnect-destination", destinationId: "dest:2" });
    });
for (const [style, threshold] of [
  ["simple", 207],
  ["history", 219],
] as const)
  test(`single joining card switches full log at the ${style} density threshold`, async ({
    page,
  }) => {
    const target = await joining(page, style, false);
    for (const height of [threshold + 1, threshold]) {
      await page.locator(".dst-list").evaluate((el, height) => {
        const box = el as HTMLElement;
        box.style.flex = "none";
        box.style.height = `${height}px`;
      }, height);
      await stage(page, "join");
      await expect(
        target.locator(height > threshold ? ".d-log" : ".d-stage-line"),
      ).toBeVisible();
      await expect(
        target.locator(height > threshold ? ".d-stage-line" : ".d-log"),
      ).toBeHidden();
      const fit = await target.evaluate((el) => {
        const box = el.getBoundingClientRect();
        return Array.from(
          el.querySelectorAll(".controls-row, .d-log, .d-stage-line"),
        )
          .filter((child) => getComputedStyle(child).display !== "none")
          .every(
            (child) => child.getBoundingClientRect().bottom <= box.bottom + 1,
          );
      });
      expect(fit).toBe(true);
    }
  });

test("full log records only observed stage intervals and resets for a new session", async ({
  page,
}) => {
  // Installation alone keeps ticking while navigation/fonts/assertions run.
  // Freeze before the first observed stage so only runFor advances intervals.
  await page.clock.install({ time: new Date("2026-01-01T00:00:00Z") });
  await page.clock.pauseAt(new Date("2026-01-01T01:00:00Z"));
  const target = await joining(page, "simple", false);
  await expect(target.locator(".d-log")).toBeVisible();
  await page.clock.runFor(1000);
  await stage(page, "gateway");
  await expect(
    target.locator(".d-log li").nth(0).locator(".log-time"),
  ).toHaveText("1.0 s");
  await page.clock.runFor(1000);
  await stage(page, "join");
  await expect(
    target.locator(".d-log li").nth(1).locator(".log-time"),
  ).toHaveText("1.0 s");
  await page.evaluate(() => {
    const t = window.uiHarness.transport;
    t.emit({
      destinations: t.current.destinations.map((d) => ({
        ...d,
        sessionId: d.sessionId + 1,
        connection: { phase: "connecting", stage: "join" },
      })),
    });
  });
  await expect(target.locator(".d-log li")).toHaveCount(1);
  await expect(target.locator(".d-log")).not.toContainText("Checking the bot");
  await expect(target.locator(".d-log")).not.toContainText("Reaching Discord");
  await expect(target.locator('.d-log [aria-current="step"]')).toContainText(
    "Joining",
  );
});
