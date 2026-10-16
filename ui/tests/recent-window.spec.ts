import { expect, test } from "@playwright/test";
import { fixtureSnapshot, freshTelemetry } from "../src/fixtures";
import type { MixTelemetry, TimingObservation } from "../src/generated";
import { TelemetryStore } from "../src/telemetry";

/** Drives the store with an injected clock at a fixed publication rate. */
function feed(
  hz: number,
  seconds: number,
  stage: (index: number, time: number) => Partial<TimingObservation>,
  output: (index: number) => { droppedFrames: number; underrunFrames: number },
) {
  let now = 1000;
  const fixture = fixtureSnapshot("audio-processing");
  const store = new TelemetryStore(() => now);
  store.setInputs(fixture.inputs);
  store.setDestinations(fixture.destinations);
  const period = 1000 / hz;
  for (let index = 0; index <= hz * seconds; index++) {
    now = 1000 + index * period;
    const value: MixTelemetry = {
      ...fixture.telemetry,
      inputs: fixture.telemetry.inputs.map((input) =>
        input.stripId === "strip:1"
          ? {
              ...input,
              timing: {
                epoch: "steady",
                phase: "running",
                appliedAdjustmentPpm: 4,
                queuedFrames: 720,
                targetFrames: 720,
                capacityFrames: 4096,
                gapOutputFrames: 0,
                droppedInputFrames: 0,
                resyncCount: 0,
                lastResetReason: null,
                lastResetAtMs: null,
                stateObservedAtMs: now,
                audioObservedAtMs: now,
                ...stage(index, now),
              },
            }
          : input,
      ),
      destinations: fixture.telemetry.destinations.map((d) =>
        d.destinationId === "local:1" ? { ...d, ...output(index) } : d,
      ),
    };
    store.ingest(freshTelemetry(value, now));
  }
  return store;
}

test("a persistent problem at the 10 Hz publication bound keeps an honest 60 s total", () => {
  // One 1 ms gap and one 1 ms underrun per observation for 70 s: 600 events per window.
  const store = feed(
    10,
    70,
    (index) => ({ gapOutputFrames: index * 48 }),
    (index) => ({ droppedFrames: 0, underrunFrames: index * 48 }),
  );
  expect(store.timing("src:strip:1")?.recent).toEqual({
    restarts: 0,
    gapMs: 600,
    droppedMs: 0,
  });
  expect(store.net("local:1").recentUnderruns).toBe(600 * 48);
  expect(store.net("local:1").underrunsSaturated).toBe(false);
});

test("reporting faster than the publication bound reads as incomplete, never a smaller total", () => {
  const store = feed(
    20,
    70,
    (index) => ({ gapOutputFrames: index * 48 }),
    (index) => ({ droppedFrames: 0, underrunFrames: index * 48 }),
  );
  expect(store.timing("src:strip:1")?.recent).toBeNull();
  expect(store.net("local:1").recentUnderruns).toBeNull();
});
