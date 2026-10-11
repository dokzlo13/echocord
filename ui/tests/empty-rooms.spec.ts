import { expect, test } from "@playwright/test";

for (const [width, height] of [
  [440, 640],
  [455, 455],
  [420, 420],
  [760, 440],
  [1000, 640],
] as const)
  for (const meterStyle of ["simple", "history"] as const)
    for (const fault of [false, true])
      test(`empty source and four ${fault ? "faulted" : "idle"} rooms fit ${meterStyle} at ${width}x${height}`, async ({
        page,
      }) => {
        await page.setViewportSize({ width, height });
        await page.goto("/tests/harness.html?fixture=rooms-restored");
        await page.evaluate(
          ({ meterStyle, fault }) => {
            const t = window.uiHarness.transport;
            const rooms = t.current.destinations.filter(
              (d) => d.kind === "discord",
            );
            const first = rooms[0];
            if (!first) throw Error("Missing owned room fixture");
            rooms.push({
              ...first,
              destinationId: "dest:4",
              serverId: "guild:4",
              channelId: "room:5",
            });
            t.emit({
              inputs: [],
              settings: { ...t.current.settings, meterStyle },
              destinations: rooms.map((d) => ({
                ...d,
                connection: fault
                  ? {
                      phase: "error",
                      code: "permission-denied",
                      message:
                        "The bot cannot speak in this room. Allow Connect and Speak.",
                    }
                  : { phase: "disconnected" },
              })),
              telemetry: {
                ...t.current.telemetry,
                inputs: [],
                destinations: [],
              },
            });
          },
          { meterStyle, fault },
        );
        await expect(page.locator(".source-card")).toHaveCount(0);
        await expect(page.locator(".dest-card")).toHaveCount(4);
        await page.evaluate(async () => {
          await document.fonts.load('400 13px "Sofia Sans Variable"');
          await document.fonts.load('700 15px "Sofia Sans Condensed Variable"');
          await document.fonts.ready;
        });
        const fit = await page.evaluate(() => {
          const source = document
            .querySelector(".src-slot")
            ?.getBoundingClientRect();
          const empty = document
            .querySelector(".src-slot .empty")
            ?.getBoundingClientRect();
          const action = document
            .querySelector(".src-slot .empty button")
            ?.getBoundingClientRect();
          const destination = document
            .querySelector(".dst-slot")
            ?.getBoundingClientRect();
          const heading = document
            .querySelector(".dst-slot .slot-head")
            ?.getBoundingClientRect();
          const list = document
            .querySelector(".dst-list")
            ?.getBoundingClientRect();
          const card = document
            .querySelector(".dest-card")
            ?.getBoundingClientRect();
          return {
            emptyContained:
              !!source &&
              !!empty &&
              !!action &&
              empty.top >= source.top &&
              empty.bottom <= source.bottom + 1 &&
              action.bottom <= empty.bottom &&
              action.top >= empty.top,
            sectionsSeparate:
              !!source &&
              !!destination &&
              (innerWidth >= 700
                ? source.right <= destination.left
                : source.bottom <= destination.top &&
                  !!empty &&
                  !!heading &&
                  empty.bottom <= heading.top),
            firstCardContained:
              !!list &&
              !!card &&
              card.top >= list.top &&
              card.bottom <= list.bottom + 1,
            headerAboveList: !!heading && !!list && heading.bottom <= list.top,
            document: [
              document.documentElement.scrollWidth,
              document.documentElement.scrollHeight,
            ],
          };
        });
        expect(fit).toEqual({
          emptyContained: true,
          sectionsSeparate: true,
          firstCardContained: true,
          headerAboveList: true,
          document: [width, height],
        });
        await page
          .getByRole("button", {
            name: "Show destination 4: Late room on Late session",
            exact: true,
          })
          .click();
        const last = page.locator(".dest-card").last();
        await expect(last.locator(".identity")).toBeFocused();
        const scroll = await last.evaluate((el) => {
          const card = el.getBoundingClientRect();
          const list = el.closest(".dst-list")?.getBoundingClientRect();
          return (
            !!list && card.top >= list.top - 1 && card.bottom <= list.bottom + 1
          );
        });
        expect(scroll).toBe(true);
        const add = page
          .locator(".src-slot .empty")
          .getByRole("button", { name: "Add source", exact: true });
        await add.click();
        await expect(
          page.getByRole("dialog", { name: "Add source", exact: true }),
        ).toBeVisible();
        await page.keyboard.press("Escape");
        await expect(add).toBeFocused();
      });
