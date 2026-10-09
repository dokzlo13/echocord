import { memo, useCallback, useEffect, useRef } from "react";
import styles from "./App.module.css";
import type { ControlAdapter } from "./adapter";

type Telemetry = ReturnType<ControlAdapter["getTelemetrySnapshot"]>;

/** Event-driven, one pending timer, no idle animation loop or React state. */
function useTelemetryDom(
  adapter: ControlAdapter,
  paint: (observation: Telemetry) => void,
  interval = 50,
) {
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let lastPaint = -Infinity;
    let disposed = false;
    const flush = () => {
      timer = undefined;
      if (disposed || document.hidden) return;
      const latest = adapter.getTelemetrySnapshot();
      if (!latest.visible) return;
      lastPaint = performance.now();
      paint(latest);
    };
    const schedule = () => {
      if (document.hidden || !adapter.getTelemetrySnapshot().visible) {
        clearTimeout(timer);
        timer = undefined;
        return;
      }
      if (timer !== undefined) return;
      const delay = Math.max(0, interval - (performance.now() - lastPaint));
      if (delay === 0) flush();
      else timer = setTimeout(flush, delay);
    };
    const visibility = () => {
      if (!document.hidden) lastPaint = -Infinity;
      schedule();
    };
    const unsubscribe = adapter.subscribeTelemetry(schedule);
    document.addEventListener("visibilitychange", visibility);
    schedule();
    return () => {
      disposed = true;
      clearTimeout(timer);
      unsubscribe();
      document.removeEventListener("visibilitychange", visibility);
    };
  }, [adapter, paint, interval]);
}

export const SignalMeter = memo(function SignalMeter({
  adapter,
  stage,
}: {
  adapter: ControlAdapter;
  stage: "input" | "mix";
}) {
  const bar = useRef<HTMLSpanElement>(null);
  const meter = useRef<HTMLMeterElement>(null);
  const readout = useRef<HTMLSpanElement>(null);
  const paint = useCallback(
    ({ capture }: Telemetry) => {
      const observed =
        capture?.phase === "running"
          ? stage === "input"
            ? capture.levelDbfs
            : capture.outputLevelDbfs
          : null;
      const level =
        observed !== null && observed !== undefined && Number.isFinite(observed)
          ? observed
          : null;
      const fraction =
        level === null ? 0 : Math.max(0, Math.min(1, (level + 60) / 60));
      const label =
        level === null
          ? "No level"
          : level <= -96
            ? "Silent"
            : `${level.toFixed(1)} dBFS`;
      if (bar.current) bar.current.style.transform = `scaleY(${fraction})`;
      if (readout.current) readout.current.textContent = label;
      if (meter.current) {
        meter.current.setAttribute("aria-valuetext", label);
        meter.current.value =
          level === null ? -60 : Math.max(-60, Math.min(0, level));
      }
    },
    [stage],
  );
  useTelemetryDom(adapter, paint);
  return (
    <div className={styles.signalMeter}>
      <div className={styles.calibration} aria-hidden="true">
        {[0, -12, -24, -36, -48, -60].map((tick) => (
          <span key={tick}>{tick}</span>
        ))}
      </div>
      <meter
        ref={meter}
        className={styles.semanticMeter}
        min={-60}
        max={0}
        value={-60}
        aria-label={
          stage === "input"
            ? "Input peak before gain"
            : "Mix peak after gain and mute, before Discord"
        }
        aria-valuetext="No level"
      />
      <div className={styles.meterTrack} aria-hidden="true">
        <span ref={bar} />
      </div>
      <span
        ref={readout}
        className={styles.meterReadout}
        data-testid={`${stage}-level`}
      >
        No level
      </span>
    </div>
  );
});

export const CaptureQueue = memo(function CaptureQueue({
  adapter,
}: {
  adapter: ControlAdapter;
}) {
  const duration = useRef<HTMLSpanElement>(null);
  const frames = useRef<HTMLSpanElement>(null);
  const bar = useRef<HTMLSpanElement>(null);
  const paint = useCallback(({ capture }: Telemetry) => {
    const available = capture?.phase === "running";
    const queued = available ? capture.queuedFrames : null;
    if (duration.current)
      duration.current.textContent =
        queued === null ? "—" : (queued / 48).toFixed(1);
    if (frames.current)
      frames.current.textContent =
        queued === null
          ? "No active queue"
          : `${queued.toLocaleString()}/1,920 frames`;
    if (bar.current)
      bar.current.style.transform = `scaleX(${queued === null ? 0 : Math.max(0, Math.min(1, queued / 1920))})`;
  }, []);
  useTelemetryDom(adapter, paint, 200);
  return (
    <section className={styles.queue} aria-label="Capture queue">
      <span>Capture queue</span>
      <div>
        <strong ref={duration} data-testid="queue-duration">
          —
        </strong>{" "}
        <span>ms buffered</span>
      </div>
      <div className={styles.queueTrack} aria-hidden="true">
        <span ref={bar} />
      </div>
      <span
        ref={frames}
        className={styles.queueFrames}
        data-testid="queue-frames"
      >
        No active queue
      </span>
    </section>
  );
});

export const CaptureCounters = memo(function CaptureCounters({
  adapter,
}: {
  adapter: ControlAdapter;
}) {
  const dropped = useRef<HTMLElement>(null);
  const underruns = useRef<HTMLElement>(null);
  const paint = useCallback(({ capture }: Telemetry) => {
    if (dropped.current)
      dropped.current.textContent = capture
        ? capture.droppedFrames.toLocaleString()
        : "—";
    if (underruns.current)
      underruns.current.textContent = capture
        ? capture.underrunFrames.toLocaleString()
        : "—";
  }, []);
  useTelemetryDom(adapter, paint, 250);
  return (
    <dl className={styles.diagnostics}>
      <div>
        <dt>Dropped capture frames</dt>
        <dd>
          <strong ref={dropped}>—</strong>
        </dd>
      </div>
      <div>
        <dt>Reader underrun frames</dt>
        <dd>
          <strong ref={underruns}>—</strong>
        </dd>
      </div>
    </dl>
  );
});
