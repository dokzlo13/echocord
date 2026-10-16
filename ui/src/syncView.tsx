/* Sync correction and Limit loud peaks readouts. Values are painted as text at
 * 2 Hz; only a correction stage appearing or disappearing re-renders React. */
import { useEffect, useRef, useSyncExternalStore } from "react";
import {
  adjustmentText,
  agoText,
  bufferedText,
  COUNT_LIMIT,
  LIMITER_LINE,
  lastMinuteText,
  RESTART_REASON,
  SYNC_STATE,
  updatedText,
} from "./copy";
import {
  paint,
  type RecentTiming,
  type TimingView,
  useTelemetryStore,
} from "./telemetry";

export function useSyncStages() {
  const store = useTelemetryStore();
  return useSyncExternalStore(store.subscribeStages, store.stages);
}

type Fact = "adjustment" | "buffered" | "recent" | "restart" | "updated";
const FACTS: readonly Fact[] = [
  "adjustment",
  "buffered",
  "recent",
  "restart",
  "updated",
];
const FACT_LABEL: Record<Fact, string> = {
  adjustment: "Rate adjustment",
  buffered: "Buffered",
  recent: "Last minute",
  restart: "Last restart",
  updated: "Updated",
};

const recentText = (recent: RecentTiming) =>
  recent === "limit" ? COUNT_LIMIT : lastMinuteText(recent);
export const recentProblem = (recent: RecentTiming | null) =>
  !!recent &&
  recent !== "limit" &&
  (recent.restarts > 0 || recent.gapMs > 0 || recent.droppedMs > 0);

function factText(view: TimingView, fact: Fact): string | null {
  switch (fact) {
    case "adjustment":
      return view.adjustmentPpm === null
        ? null
        : adjustmentText(view.adjustmentPpm);
    case "buffered":
      return view.queuedMs === null
        ? null
        : bufferedText(view.queuedMs, view.targetMs);
    case "recent":
      return view.recent === null ? null : recentText(view.recent);
    case "restart":
      return view.lastReset === null
        ? null
        : `${agoText(view.lastReset.ageMs)} · ${RESTART_REASON[view.lastReset.reason]}`;
    case "updated":
      return view.ageMs === null ? null : updatedText(view.ageMs);
  }
}

/** One stage's label/value pairs inside a <dl>; a fact without a value hides its term too. */
export function SyncFacts({
  stageKey,
  stateRef,
}: {
  stageKey: string;
  stateRef: React.RefObject<HTMLElement | null>;
}) {
  const store = useTelemetryStore();
  const values = useRef<Partial<Record<Fact, HTMLElement | null>>>({});
  const terms = useRef<Partial<Record<Fact, HTMLElement | null>>>({});
  useEffect(
    () =>
      paint(
        store,
        () => {
          const view = store.timing(stageKey);
          const state = stateRef.current;
          if (state) {
            state.textContent = SYNC_STATE[view?.state ?? "stale"];
            state.dataset.sync = view?.state ?? "stale";
          }
          for (const fact of FACTS) {
            const text = view ? factText(view, fact) : null;
            const value = values.current[fact];
            const term = terms.current[fact];
            if (value) {
              value.textContent = text ?? "";
              value.hidden = text === null;
              if (fact === "recent" && recentProblem(view?.recent ?? null))
                value.dataset.warn = "";
              else delete value.dataset.warn;
            }
            if (term) term.hidden = text === null;
          }
        },
        2,
      ),
    [store, stageKey, stateRef],
  );
  return (
    <>
      {FACTS.map((fact) => (
        <FactPair
          key={fact}
          label={FACT_LABEL[fact]}
          termRef={(el) => {
            terms.current[fact] = el;
          }}
          valueRef={(el) => {
            values.current[fact] = el;
          }}
        />
      ))}
    </>
  );
}

function FactPair({
  label,
  termRef,
  valueRef,
}: {
  label: string;
  termRef: (el: HTMLElement | null) => void;
  valueRef: (el: HTMLElement | null) => void;
}) {
  return (
    <>
      <dt ref={termRef} hidden>
        {label}
      </dt>
      <dd ref={valueRef} hidden />
    </>
  );
}

/** Playback output details: the stage state and applied adjustment. The existing
 * Buffer tile already reads this output's queue, so no second buffer value is shown. */
export function PlaybackSyncFacts({ stageKey }: { stageKey: string }) {
  const store = useTelemetryStore();
  const term = useRef<HTMLElement>(null);
  const value = useRef<HTMLElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          const view = store.timing(stageKey);
          const adjustment =
            view?.adjustmentPpm == null
              ? null
              : adjustmentText(view.adjustmentPpm).toLowerCase();
          const text = view
            ? adjustment
              ? `${SYNC_STATE[view.state]} · ${adjustment}`
              : SYNC_STATE[view.state]
            : null;
          if (value.current) {
            value.current.textContent = text ?? "";
            value.current.hidden = text === null;
          }
          if (term.current) term.current.hidden = text === null;
        },
        2,
      ),
    [store, stageKey],
  );
  return (
    <>
      <dt ref={term} hidden>
        Sync correction
      </dt>
      <dd ref={value} hidden />
    </>
  );
}

/** Status under the Limit loud peaks switch; hidden while the limiter is not processing. */
export function LimiterLine({ enabled }: { enabled: boolean }) {
  const store = useTelemetryStore();
  const ref = useRef<HTMLParagraphElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          const el = ref.current;
          if (!el) return;
          const view = store.limiter();
          const text =
            !enabled || view.state === "none" ? null : LIMITER_LINE[view.state];
          el.textContent = text ?? "";
          el.hidden = text === null;
          el.dataset.state = view.state;
        },
        2,
      ),
    [store, enabled],
  );
  return <p ref={ref} className="set-live" hidden />;
}

/** The limiter fact in Sync details; numbers stay in this diagnostic view. */
export function LimiterDetail({ enabled }: { enabled: boolean }) {
  const store = useTelemetryStore();
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          if (!ref.current) return;
          const view = store.limiter();
          ref.current.textContent = !enabled
            ? ""
            : view.state === "reducing" && view.maxDb !== null
              ? ` · lowered peaks by up to ${view.maxDb.toFixed(1)} dB in the last 10 s`
              : view.state === "stale"
                ? " · no recent data"
                : "";
        },
        2,
      ),
    [store, enabled],
  );
  return <span ref={ref} />;
}

/** Live one-line summary on the Sync details row in Settings. */
export function SyncLinkSummary({ enabled }: { enabled: boolean }) {
  const store = useTelemetryStore();
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          const el = ref.current;
          if (!el) return;
          const views = store
            .stages()
            .map((key) => store.timing(key))
            .filter((view): view is TimingView => view !== null);
          const restarts = views.reduce(
            (sum, view) =>
              sum +
              (view.recent && view.recent !== "limit"
                ? view.recent.restarts
                : 0),
            0,
          );
          const problems = views.some((view) => recentProblem(view.recent));
          let text: string;
          if (views.length === 0) text = "Shows details while sound is playing";
          else if (!enabled) text = "Sync correction is off";
          else if (restarts > 0)
            text = `${restarts} restart${restarts === 1 ? "" : "s"} in the last minute`;
          else if (problems) text = "Dropouts in the last minute";
          else if (views.every((view) => view.state === "running"))
            text =
              views.length === 1 ? "Running" : `All ${views.length} running`;
          else if (
            views.some(
              (view) => view.state === "priming" || view.state === "restarting",
            )
          )
            text = SYNC_STATE.priming;
          else text = "Some details are not current";
          el.textContent = text;
          if (problems) el.dataset.warn = "";
          else delete el.dataset.warn;
        },
        2,
      ),
    [store, enabled],
  );
  return <span ref={ref} className="set-sub" />;
}
