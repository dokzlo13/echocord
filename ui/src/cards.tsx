import {
  ArrowRightLeft,
  Check,
  Circle,
  LoaderCircle,
  Plus,
  Square,
  Timer,
  TriangleAlert,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { destinationStatusLabel, STAGES, stageLine, statusKey } from "./copy";
import { HelpLink } from "./helpLinks";
import { ServerTile, SourceGlyph } from "./icons";
import type {
  DestCard as Dest,
  MeterMode,
  SourceCard as Source,
  Stage,
} from "./model";
import { fraction, paint, useTelemetryStore } from "./telemetry";

const MINUS = "−";
const fmtDb = (db: number) => {
  const v = Math.abs(db).toFixed(0);
  return `${db < 0 && v !== "0" ? MINUS : ""}${v}`;
};

/* ------------------------------------------------------------------ levels */

/** Scalar telemetry paints through refs; control state never receives meter updates. */
function useLevel(key: string) {
  const store = useTelemetryStore();
  const host = useRef<HTMLElement>(null);
  const readout = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    let clipUntil = 0;
    return paint(store, (now) => {
      const el = host.current;
      if (!el) return;
      const r = store.read(key);
      el.style.setProperty("--level", fraction(r.db).toFixed(3));
      el.style.setProperty("--peak", fraction(r.peak).toFixed(3));
      el.dataset.signal = r.db === null ? "none" : r.db > -3 ? "hot" : "ok";
      el.dataset.stale = String(r.stale);
      if (r.stale) clipUntil = 0;
      else if (r.preClip) clipUntil = now + 1500;
      if (now < clipUntil) el.dataset.clip = "";
      else delete el.dataset.clip;
      if (readout.current)
        readout.current.textContent = r.db === null ? "" : fmtDb(r.db);
    });
  }, [key, store]);
  return { host, readout };
}

const GUIDES = [-6, -12, -24, -48];

/** Level history: x = time (newest at the right), y = level. Drawn from the shared ring. */
function LevelWell({ levelKey }: { levelKey: string }) {
  const store = useTelemetryStore();
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const cv = ref.current;
    const ctx = cv?.getContext("2d");
    if (!cv || !ctx) return;
    let colour = "#888";
    let guide = "rgba(255,255,255,.07)";
    let styled = 0;
    const fit = () => {
      const dpr = window.devicePixelRatio || 1;
      cv.width = Math.max(1, Math.round(cv.clientWidth * dpr));
      cv.height = Math.max(1, Math.round(cv.clientHeight * dpr));
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(cv);
    const stop = paint(
      store,
      (now) => {
        if (now - styled > 1000) {
          const cs = getComputedStyle(cv);
          colour = cs.getPropertyValue("--meter-fill").trim() || colour;
          guide = cs.getPropertyValue("--well-guide").trim() || guide;
          styled = now;
        }
        const ring = store.history(levelKey);
        const w = cv.width;
        const h = cv.height;
        ctx.clearRect(0, 0, w, h);
        const dpr = window.devicePixelRatio || 1;
        const col = Math.max(1, Math.round(1.5 * dpr));
        const cols = ring?.buf.length ?? 0;
        const duration = Math.max(1000, Math.floor(w / col) * 100);
        // History and the current level share one well: older samples fade towards the left,
        // the newest few are drawn brightest, and the live bar runs along the floor.
        ctx.fillStyle = colour;
        for (let k = 0; ring && k < cols; k++) {
          const f = ring.at(k);
          if (Number.isNaN(f) || f <= 0) continue;
          const age = (now - ring.timeAt(k)) / duration;
          if (age < 0 || age > 1) continue;
          ctx.globalAlpha = age < 0.05 ? 0.9 : 0.62 - 0.5 * age;
          ctx.fillRect(w * (1 - age) - col, h - f * h, col, f * h);
        }
        ctx.globalAlpha = 1;
        ctx.fillStyle = guide;
        for (const g of GUIDES)
          ctx.fillRect(0, Math.round(h - fraction(g) * h), w, 1);
      },
      10,
    );
    return () => {
      stop();
      ro.disconnect();
    };
  }, [levelKey, store]);
  return <canvas ref={ref} className="well" aria-hidden />;
}

/** A lone source in a roomy window grows into its level over the last minute. */
function SourceHistory({ levelKey }: { levelKey: string }) {
  const wrap = useRef<HTMLDivElement>(null);
  const store = useTelemetryStore();
  const ref = useRef<HTMLCanvasElement>(null);
  const peakRef = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    const cv = ref.current;
    const ctx = cv?.getContext("2d");
    if (!cv || !ctx) return;
    const stop = paint(
      store,
      (now) => {
        const dpr = window.devicePixelRatio || 1;
        const w = Math.round(cv.clientWidth * dpr);
        const h = Math.round(cv.clientHeight * dpr);
        if (cv.width !== w || cv.height !== h) {
          cv.width = w;
          cv.height = h;
        }
        const cs = getComputedStyle(cv);
        const colour = cs.getPropertyValue("--meter-fill").trim();
        const right = 30 * dpr;
        const plotW = w - right;
        const padT = 6 * dpr;
        const padB = 16 * dpr;
        const plotH = h - padT - padB;
        const ring = store.history(levelKey);
        ctx.clearRect(0, 0, w, h);
        ctx.fillStyle = cs.getPropertyValue("--grid").trim();
        ctx.font = `${11 * dpr}px ${cs.fontFamily}`;
        for (const g of [0, -12, -24, -48]) {
          const y = padT + plotH - fraction(g) * plotH;
          ctx.fillStyle = cs.getPropertyValue("--grid").trim();
          ctx.fillRect(0, Math.round(y), plotW, Math.max(1, Math.round(dpr)));
          ctx.fillStyle = cs.getPropertyValue("--axis").trim();
          ctx.textBaseline = "middle";
          ctx.fillText(g === 0 ? "0" : `${MINUS}${-g}`, plotW + 6 * dpr, y);
        }
        ctx.textBaseline = "alphabetic";
        ctx.fillText(`${MINUS}60 s`, 0, h - 3 * dpr);
        ctx.fillText("now", plotW - ctx.measureText("now").width, h - 3 * dpr);
        if (!ring) {
          if (peakRef.current) peakRef.current.textContent = "";
          return;
        }
        let peak = 0;
        let observed = false;
        let previousTime = -Infinity;
        const y = (f: number) => padT + plotH - f * plotH;
        ctx.beginPath();
        for (let k = ring.buf.length - 1; k >= 0; k--) {
          const value = ring.at(k);
          const time = ring.timeAt(k);
          const age = now - time;
          if (!Number.isFinite(value) || age < 0 || age > 60_000) continue;
          const x = plotW * (1 - age / 60_000);
          observed = true;
          peak = Math.max(peak, value);
          if (time - previousTime > 250) ctx.moveTo(x, y(value));
          else ctx.lineTo(x, y(value));
          previousTime = time;
          ctx.globalAlpha = 0.16;
          ctx.fillStyle = colour;
          ctx.fillRect(
            x,
            y(value),
            Math.max(dpr, plotW / 600),
            y(0) - y(value),
          );
        }
        ctx.globalAlpha = 1;
        ctx.strokeStyle = colour;
        ctx.lineWidth = 1.5 * dpr;
        ctx.lineJoin = "round";
        ctx.stroke();
        if (peakRef.current) {
          const db = peak * 60 - 60;
          peakRef.current.textContent = !observed
            ? "Level unavailable"
            : peak > 0
              ? `Peak ${fmtDb(db)} dBFS`
              : "Below −60 dBFS";
        }
      },
      10,
    );
    return stop;
  }, [levelKey, store]);
  return (
    <div className="src-history" ref={wrap}>
      <div className="net-legend">
        <span>Level, last 60 s</span>
        <span className="net-unit tabular" ref={peakRef} />
      </div>
      <div className="net-plot">
        <canvas ref={ref} aria-hidden />
      </div>
    </div>
  );
}

/* ------------------------------------------------- gain lane + level meter */

const DB_FLOOR = -60;
const fmtGain = (db: number | null) =>
  db === null
    ? "Off"
    : `${db < -0.05 ? MINUS : ""}${Math.abs(db).toFixed(1)} dB`;

const SCALE = [-48, -24, -12, -6, 0];

/**
 * Combined channel fader: a gain lane along the top edge of a post-gain level meter, both on
 * one −60…0 dB axis. Because the meter is post-gain, its fill can never pass the thumb, so the
 * thumb reads as the channel's ceiling. Native range input (dB taper): arrows 0.5 dB, Page 6 dB.
 * Scale ticks hang below the meter and never cross the thumb.
 */
function ChannelFader({
  levelKey,
  mode,
  gainDb,
  onChange,
  label,
  onCommit,
  muted,
  idle,
  readout,
}: {
  levelKey: string;
  mode: MeterMode;
  gainDb: number | null;
  onChange: (db: number | null) => Promise<boolean>;
  onCommit: () => void;
  label: string;
  muted?: boolean | undefined;
  idle?: string | undefined;
  readout: React.RefObject<HTMLSpanElement | null>;
}) {
  const [draft, setDraft] = useState<number | null | undefined>(undefined);
  const sequence = useRef(0);
  useEffect(() => {
    if (draft === gainDb) setDraft(undefined);
  }, [gainDb, draft]);
  const shown = draft === undefined ? gainDb : draft;
  const db = shown ?? DB_FLOOR;
  const set = (v: number) => {
    const next = v <= DB_FLOOR ? null : Math.min(0, v);
    setDraft(next);
    const id = ++sequence.current;
    void onChange(next).then((ok) => {
      if (!ok && id === sequence.current) setDraft(undefined);
    });
  };
  return (
    <div
      className="cf"
      data-mode={mode}
      data-muted={muted || undefined}
      data-idle={idle ? true : undefined}
      style={{ "--gain": (db - DB_FLOOR) / -DB_FLOOR } as React.CSSProperties}
    >
      <div className="cf-body">
        <div className="cf-axis" aria-hidden>
          <span className="cf-lane">
            <span className="cf-lane-fill" />
          </span>
          <span className="cf-meter">
            {mode === "history" && !idle && <LevelWell levelKey={levelKey} />}
            <span className="mf-fill" />
            <span className="mf-peak" />
            <span className="cf-ceiling" />
          </span>
          <span
            className="meter-clip"
            title={
              levelKey.startsWith("src:")
                ? "Input clipped before gain"
                : "Mix clipped before destination volume"
            }
          />
          {mode === "simple" && (
            <span className="meter-scale">
              {SCALE.map((g) => (
                <span key={g} style={{ left: `${fraction(g) * 100}%` }} />
              ))}
            </span>
          )}
          {idle ? (
            <span className="mf-idle">{idle}</span>
          ) : (
            <span className="mf-idle mf-stale">No recent data</span>
          )}
        </div>
        <input
          type="range"
          className="cf-input"
          onPointerUp={onCommit}
          onBlur={onCommit}
          min={DB_FLOOR}
          max={0}
          step={0.5}
          value={db}
          aria-label={label}
          aria-valuetext={fmtGain(shown)}
          onChange={(e) => set(Number(e.currentTarget.value))}
          onKeyDown={(e) => {
            if (e.key === "PageUp" || e.key === "PageDown") {
              e.preventDefault();
              set(db + (e.key === "PageUp" ? 6 : -6));
            }
          }}
          title={`${label}: ${fmtGain(shown)}`}
        />
      </div>
      <span className="cf-values tabular" aria-hidden>
        <span className="cf-gain">{fmtGain(shown)}</span>
        <span className="cf-level" ref={readout} />
      </span>
    </div>
  );
}

/* --------------------------------------------------------------- telemetry */

function useEvery(ms: number) {
  const store = useTelemetryStore();
  const [, set] = useState(0);
  useEffect(
    () => paint(store, () => set((n) => n + 1), 1000 / ms),
    [ms, store],
  );
}

function Spark({ uid, kind }: { uid: string; kind: "gateway" | "buffer" }) {
  const store = useTelemetryStore();
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(
    () =>
      paint(
        store,
        (now) => {
          const cv = ref.current;
          const ctx = cv?.getContext("2d");
          if (!cv || !ctx) return;
          const dpr = devicePixelRatio || 1;
          const w = Math.round(cv.clientWidth * dpr);
          const h = Math.round(cv.clientHeight * dpr);
          if (!w || !h) return;
          if (cv.width !== w) cv.width = w;
          if (cv.height !== h) cv.height = h;
          const cs = getComputedStyle(cv);
          const n = store.net(uid);
          const ring = kind === "gateway" ? n.ping2m : n.buffer2m;
          const max = kind === "gateway" ? 120 : 60;
          const pad = 3 * dpr;
          const y = (v: number) =>
            h - pad - (Math.min(v, max) / max) * (h - pad * 2);
          ctx.clearRect(0, 0, w, h);
          ctx.lineWidth = 1.5 * dpr;
          ctx.lineJoin = "round";
          ctx.strokeStyle = cs.getPropertyValue("--spark").trim() || "#888";
          ctx.beginPath();
          let connected = false;
          let previousTime = -Infinity;
          for (let k = ring.buf.length - 1; k >= 0; k--) {
            const v = ring.at(k);
            const time = ring.timeAt(k);
            const age = now - time;
            if (!Number.isFinite(v) || age < 0 || age > 12_000) continue;
            if (time - previousTime > (kind === "gateway" ? 2500 : 1500))
              connected = false;
            previousTime = time;
            const x = (1 - age / 12_000) * (w - pad * 2) + pad;
            if (!connected) {
              ctx.moveTo(x, y(v));
              connected = true;
            } else ctx.lineTo(x, y(v));
          }
          ctx.stroke();
          const current = ring.at(0);
          const currentAge = now - ring.timeAt(0);
          const available = Number.isFinite(current) && currentAge <= 12_000;
          cv.dataset.available = available ? "true" : "false";
          if (available) {
            ctx.fillStyle = cs.getPropertyValue("--spark-now").trim() || "#fff";
            ctx.beginPath();
            ctx.arc(
              (1 - currentAge / 12_000) * (w - pad * 2) + pad,
              y(current),
              2.5 * dpr,
              0,
              Math.PI * 2,
            );
            ctx.fill();
          }
        },
        2,
      ),
    [uid, kind, store],
  );
  return (
    <canvas ref={ref} className="spark" data-available="false" aria-hidden />
  );
}
export function StatTiles({
  uid,
  compact,
  local,
}: {
  uid: string;
  compact?: boolean | undefined;
  local?: boolean | undefined;
}) {
  const store = useTelemetryStore();
  const host = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          const n = store.net(uid);
          const values = local
            ? [n.buffer, n.recentDrops === null ? null : n.recentDrops / 48]
            : [
                n.ping,
                n.buffer,
                n.recentDrops === null ? null : n.recentDrops / 48,
              ];
          host.current
            ?.querySelectorAll<HTMLElement>("[data-value]")
            .forEach((el, i) => {
              if (i === values.length - 1) {
                if ((n.recentDrops ?? 0) > 0) el.dataset.warn = "";
                else delete el.dataset.warn;
              }
              el.textContent =
                values[i] == null
                  ? "—"
                  : `${i === values.length - 1 ? Math.ceil(values[i] ?? 0) : Math.round(values[i] ?? 0)}`;
              el.title = values[i] === null ? "No recent data" : "";
            });
        },
        2,
      ),
    [store, uid, local],
  );
  return (
    <span
      ref={host}
      className="stats"
      data-compact={compact || undefined}
      data-local={local || undefined}
    >
      {!local && (
        <span
          className="stat"
          title="Measured Discord gateway round trip; voice-server ping is unavailable"
        >
          <span className="stat-label">Gateway</span>
          <span className="stat-value tabular">
            <span data-value>—</span>
            <small> ms</small>
          </span>
          {!compact && <Spark uid={uid} kind="gateway" />}
        </span>
      )}
      <span
        className="stat"
        title="Audio queued inside Echocord; not hardware or listener delay"
      >
        <span className="stat-label">Buffer</span>
        <span className="stat-value tabular">
          <span data-value>—</span>
          <small> ms</small>
        </span>
        {!compact && <Spark uid={uid} kind="buffer" />}
      </span>
      <span
        className="stat"
        title="Audio discarded in the last 60 seconds to keep this output queue bounded; rounded up to milliseconds"
      >
        <span className="stat-label">Dropped · 60 s</span>
        <span className="stat-value tabular">
          <span data-value>—</span>
          <small> ms</small>
        </span>
      </span>
    </span>
  );
}

/** Native buffer observations only; unavailable gateway values do not become a series. */
export function NetChart({
  uid,
  local,
  seconds,
  minHeight = 28,
}: {
  uid: string;
  local?: boolean | undefined;
  seconds: number;
  minHeight?: number | undefined;
}) {
  const store = useTelemetryStore();
  const box = useRef<HTMLDivElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const cvs = useRef<HTMLCanvasElement>(null);
  const tip = useRef<HTMLDivElement>(null);
  const hover = useRef<number | null>(null);
  useEffect(
    () =>
      paint(
        store,
        (now) => {
          const cv = cvs.current;
          const ctx = cv?.getContext("2d");
          const root = box.current;
          if (!cv || !ctx || !root) return;
          const show =
            root.clientHeight >= minHeight + 24 && cv.clientWidth > 120;
          if (show) delete root.dataset.hidden;
          else root.dataset.hidden = "true";
          if (!show) return;
          const dpr = window.devicePixelRatio || 1;
          const w = Math.round(cv.clientWidth * dpr);
          const h = Math.round(cv.clientHeight * dpr);
          if (cv.width !== w) cv.width = w;
          if (cv.height !== h) cv.height = h;
          const cs = getComputedStyle(cv);
          const n = store.net(uid);
          const samples = n.buffer2m.buf.length;
          const within = (time: number) =>
            now - time >= 0 && now - time <= seconds * 1000;
          let max = 0;
          for (let k = 0; k < samples; k++) {
            const buffer = n.buffer2m.at(k);
            const gateway = n.ping2m.at(k);
            if (Number.isFinite(buffer) && within(n.buffer2m.timeAt(k)))
              max = Math.max(max, buffer);
            if (
              !local &&
              Number.isFinite(gateway) &&
              within(n.ping2m.timeAt(k))
            )
              max = Math.max(max, gateway);
          }
          const top = Math.max(60, Math.ceil(max / 20) * 20);
          const right = 30 * dpr;
          const padT = 6 * dpr;
          const padB = 16 * dpr;
          const plotW = w - right;
          const plotH = h - padT - padB;
          const x = (time: number) =>
            plotW * (1 - (now - time) / (seconds * 1000));
          const y = (v: number) =>
            padT + plotH - (Math.min(v, top) / top) * plotH;
          ctx.clearRect(0, 0, w, h);
          ctx.font = `${11 * dpr}px ${cs.fontFamily}`;
          ctx.textBaseline = "middle";
          for (const t of [0, top / 2, top]) {
            ctx.fillStyle = cs.getPropertyValue("--grid").trim();
            ctx.fillRect(
              0,
              Math.round(y(t)),
              plotW,
              Math.max(1, Math.round(dpr)),
            );
            ctx.fillStyle = cs.getPropertyValue("--axis").trim();
            ctx.fillText(`${t}`, plotW + 6 * dpr, y(t));
          }
          ctx.textBaseline = "alphabetic";
          ctx.fillText(`−${seconds} s`, 0, h - 3 * dpr);
          ctx.fillText(
            "now",
            plotW - ctx.measureText("now").width,
            h - 3 * dpr,
          );
          ctx.lineWidth = 2 * dpr;
          ctx.lineJoin = "round";
          ctx.lineCap = "round";
          for (const series of [
            ...(!local ? [{ ring: n.ping2m, css: "--net-a", gap: 2500 }] : []),
            { ring: n.buffer2m, css: "--net-b", gap: 1500 },
          ]) {
            ctx.strokeStyle = cs.getPropertyValue(series.css).trim();
            ctx.beginPath();
            let previousTime = -Infinity;
            for (let k = samples - 1; k >= 0; k--) {
              const v = series.ring.at(k);
              const time = series.ring.timeAt(k);
              if (!Number.isFinite(v) || !within(time)) continue;
              if (time - previousTime > series.gap) ctx.moveTo(x(time), y(v));
              else ctx.lineTo(x(time), y(v));
              previousTime = time;
            }
            ctx.stroke();
          }
          const at = hover.current;
          const tooltip = tip.current;
          if (tooltip) {
            tooltip.hidden = at === null;
            if (at !== null) {
              const time = now - at;
              const v = n.buffer2m.near(time, 350);
              const gateway = n.ping2m.near(time, 600);
              tooltip.textContent = `${at === 0 ? "now" : `−${Math.round(at / 1000)} s`}${local ? "" : ` · Gateway ${Number.isFinite(gateway) ? `${Math.round(gateway)} ms` : "unavailable"}`} · Buffer ${Number.isFinite(v) ? `${Math.round(v)} ms` : "unavailable"}`;
              tooltip.style.left = `${Math.max(0, Math.min(cv.clientWidth - 150, x(now - at) / dpr + 12))}px`;
              ctx.fillStyle = cs.getPropertyValue("--crosshair").trim();
              ctx.fillRect(
                Math.round(x(now - at)),
                padT,
                Math.max(1, Math.round(dpr)),
                plotH,
              );
            }
          }
        },
        2,
      ),
    [store, uid, local, seconds, minHeight],
  );
  return (
    <div className="net-chart" ref={box}>
      <div className="net-legend" aria-hidden>
        {!local && (
          <span className="net-key" data-series="ping2m">
            Gateway
          </span>
        )}
        <span className="net-key" data-series="buffer2m">
          Buffer
        </span>
        <span className="net-unit">ms, last {seconds} s</span>
      </div>
      <div
        ref={wrap}
        className="net-plot"
        role="img"
        aria-label={`Measured audio queue inside Echocord over the last ${seconds} seconds`}
        onPointerMove={(e) => {
          const r = wrap.current?.getBoundingClientRect();
          if (r)
            hover.current = Math.round(
              (1 -
                Math.max(0, Math.min(r.width - 30, e.clientX - r.left)) /
                  (r.width - 30)) *
                (seconds * 1000),
            );
        }}
        onPointerLeave={() => {
          hover.current = null;
          if (tip.current) tip.current.hidden = true;
        }}
      >
        <canvas ref={cvs} />
        <div ref={tip} className="net-tip tabular" hidden />
      </div>
    </div>
  );
}

const LOG_LABEL: Record<string, (room: string) => string> = {
  checking: () => "Checking the bot",
  gateway: () => "Reaching Discord",
  joining: (room) => `Joining ${room}`,
  starting: () => "Starting audio",
};

/**
 * Connection progress as a short live log, shown only when the card has spare height
 * (CSS falls back to the single stage line). Durations are the observed time between stage
 * changes, not estimates.
 */
function ConnectLog({
  stage,
  room,
}: {
  stage?: Stage | undefined;
  room: string;
}) {
  const observations = useRef<
    { key: number; id: Stage; start: number; end?: number }[]
  >([]);
  const serial = useRef(0);
  useEvery(200);
  const now = performance.now();
  const previous = observations.current.at(-1);
  if (stage && previous?.id !== stage) {
    if (previous) previous.end = now;
    observations.current.push({ key: ++serial.current, id: stage, start: now });
    if (observations.current.length > 8) observations.current.shift();
  }
  const secs = (ms: number) => `${(ms / 1000).toFixed(1)} s`;
  return (
    <>
      <ol className="connect-log d-log" aria-label="Connection progress">
        {observations.current.map((observed) => {
          const done = observed.end !== undefined;
          return (
            <li
              key={observed.key}
              data-status={done ? "done" : "now"}
              aria-current={done ? undefined : "step"}
            >
              <span className="log-mark" aria-hidden>
                {done ? <Check size={13} /> : <span className="log-dot" />}
              </span>
              <span className="log-label">
                {LOG_LABEL[observed.id]?.(room)}…
              </span>
              <span className="log-result tabular">
                {done ? (
                  <span className="log-ok">Done</span>
                ) : (
                  <LoaderCircle size={13} className="spin" aria-hidden />
                )}
                <span className="log-time">
                  {secs((observed.end ?? now) - observed.start)}
                </span>
              </span>
            </li>
          );
        })}
      </ol>
      <div className="stages d-stage-line" aria-live="polite">
        <ol className="stage-steps" aria-label="Connection progress">
          {observations.current.map((observed) => (
            <li
              key={observed.key}
              data-now={observed.end === undefined || undefined}
              aria-current={observed.end === undefined ? "step" : undefined}
            >
              {STAGES.find((s) => s.id === observed.id)?.label}
            </li>
          ))}
        </ol>
        <span className="stage-line">{stageLine(stage, room)}</span>
      </div>
    </>
  );
}

function InlinePing({ uid }: { uid: string }) {
  const store = useTelemetryStore();
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          const n = store.net(uid);
          if (ref.current)
            ref.current.textContent = `${n.buffer === null ? "—" : Math.round(n.buffer)} ms queued · ${n.recentDrops === null ? "—" : Math.ceil(n.recentDrops / 48)} ms dropped · 60 s`;
        },
        2,
      ),
    [store, uid],
  );
  return <span ref={ref}>Awaiting audio telemetry</span>;
}
export function OnAirTime({ since }: { since?: number | undefined }) {
  const store = useTelemetryStore();
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          const total = Math.max(
            0,
            Math.floor((Date.now() - (since ?? Date.now())) / 1000),
          );
          const h = Math.floor(total / 3600);
          const m = Math.floor((total % 3600) / 60);
          const sec = String(total % 60).padStart(2, "0");
          if (ref.current)
            ref.current.textContent =
              h > 0
                ? `${h}:${String(m).padStart(2, "0")}:${sec}`
                : `${m}:${sec}`;
        },
        1,
      ),
    [store, since],
  );
  return <span ref={ref}>0:00</span>;
}

/* ------------------------------------------------------------------ pieces */

function Slip({
  text,
  compact,
  children,
  id,
}: {
  text: string;
  id?: string | undefined;
  compact?: boolean;
  children?: ReactNode | undefined;
}) {
  return (
    <div
      id={id}
      className={`slip${compact ? " operation-slip" : ""}`}
      role="note"
    >
      <TriangleAlert size={16} aria-hidden className="slip-icon" />
      <span className="slip-text" title={text}>
        {text}
      </span>
      <span className="slip-actions">{children}</span>
    </div>
  );
}

function Remove({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      className="icon-btn remove"
      aria-label={label}
      title={label}
      onClick={onClick}
    >
      <X size={15} />
    </button>
  );
}

function MuteToggle({
  muted,
  onChange,
  what,
}: {
  muted: boolean;
  onChange: (m: boolean) => void;
  what: string;
}) {
  return (
    <button
      type="button"
      className="btn btn-toggle"
      data-icon
      aria-pressed={muted}
      aria-label={muted ? `Unmute ${what}` : `Mute ${what}`}
      title={muted ? `Unmute ${what}` : `Mute ${what}`}
      onClick={() => onChange(!muted)}
    >
      {muted ? <VolumeX size={15} /> : <Volume2 size={15} />}
    </button>
  );
}

function useIdentityNotice(
  key: string,
  name: string,
  ready: boolean,
  failed = false,
) {
  const previous = useRef({ key, name });
  const pending = useRef<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  useEffect(() => {
    if (previous.current.key !== key) {
      pending.current = previous.current.name;
      previous.current = { key, name };
    } else previous.current.name = name;
    if (failed) pending.current = null;
    const from = pending.current;
    if (!from) return;
    if (timer.current) clearTimeout(timer.current);
    setNotice(ready ? `Switched from ${from}` : `Selected instead of ${from}`);
    if (ready) pending.current = null;
    timer.current = setTimeout(() => {
      setNotice(null);
    }, 2800);
  }, [key, name, ready, failed]);
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );
  return notice;
}

const sourceTag = (c: Source) =>
  c.state === "opening"
    ? "Opening…"
    : c.state === "fault"
      ? "Fault"
      : c.state === "stopped"
        ? "Stopped"
        : c.muted && c.state === "capturing"
          ? "Muted"
          : null;

/* ----------------------------------------------------------------- sources */

export interface SourceHandlers {
  dismiss: () => void;
  change: () => void;
  remove: () => void;
  capture: () => void;
  stop: () => void;
  mute: (m: boolean) => void;
  gain: (g: number | null) => Promise<boolean>;
  commit: () => void;
  fault: () => void;
}

export function SourceCard({
  card,
  ordinal,
  mode,
  expand,
  discoveryReady,
  h,
}: {
  card: Source;
  discoveryReady: boolean;
  ordinal: number | undefined;
  mode: MeterMode;
  expand?: boolean | undefined;
  h: SourceHandlers;
}) {
  const s = card.identity;
  const { host, readout } = useLevel(`src:${card.uid}`);
  const capturing = card.state === "capturing";
  const opening = card.state === "opening";
  const notice = useIdentityNotice(
    card.sourceId,
    s.name,
    card.state === "capturing",
    card.state === "fault",
  );
  const detail =
    notice ??
    (card.restored && s.kind === "application" && card.state === "stopped"
      ? "From last session. Press Capture to start."
      : s.detail);
  const tag = sourceTag(card);
  const restartRequired =
    card.fault?.code === "engine-stopped" ||
    card.fault?.code === "cleanup-incomplete" ||
    card.operationError?.code === "engine-stopped";
  const selectRequired =
    !restartRequired &&
    ((card.state === "stopped" &&
      s.kind === "application" &&
      discoveryReady &&
      !s.available) ||
      (card.state === "fault" &&
        (!s.available || card.fault?.code === "invalid-selection")) ||
      (card.operationError?.code === "invalid-selection" &&
        (card.operationCommand === "start-capture" ||
          card.operationCommand === "replace-source")));
  const noteId = `source-note-${card.uid}`;
  const hasNote = Boolean(
    card.operationError || (card.state === "fault" && card.fault),
  );
  const idle =
    card.state === "stopped" || card.state === "fault"
      ? "Not capturing"
      : opening
        ? "Opening…"
        : undefined;

  const capture =
    capturing || opening ? (
      <button
        type="button"
        className="btn btn-capture"
        data-on
        onClick={h.stop}
        aria-describedby={hasNote ? noteId : undefined}
        title={`Stop capturing ${s.name}`}
        aria-label={`Stop capturing ${s.name}`}
      >
        {opening ? (
          <LoaderCircle size={14} className="spin" />
        ) : (
          <Square size={11} fill="currentColor" />
        )}
        <span>Stop</span>
      </button>
    ) : (
      <button
        type="button"
        className="btn btn-capture btn-primary"
        onClick={() => {
          if (!restartRequired) (selectRequired ? h.change : h.capture)();
        }}
        aria-disabled={restartRequired}
        aria-describedby={hasNote ? noteId : undefined}
        title={
          restartRequired
            ? "Quit and reopen Echocord before capturing again."
            : selectRequired
              ? `Select ${s.name} again`
              : `Capture ${s.name}`
        }
        aria-label={
          selectRequired ? `Select ${s.name} again` : `Capture ${s.name}`
        }
      >
        {selectRequired ? (
          <ArrowRightLeft size={14} />
        ) : (
          <Circle size={11} fill="currentColor" />
        )}
        <span>{selectRequired ? "Select again" : "Capture"}</span>
      </button>
    );

  const identity = (
    <button
      type="button"
      className="identity"
      onClick={() => {
        if (!restartRequired) h.change();
      }}
      aria-disabled={restartRequired}
      aria-describedby={hasNote ? noteId : undefined}
      title={`Change ${s.name}`}
      aria-label={`Change ${s.name}`}
    >
      <SourceGlyph glyph={s.glyph} tint={s.tint} />
      <span className="id-text">
        <span className="id-name">{s.name}</span>
        <span
          className="id-detail"
          title={detail ?? undefined}
          data-notice={notice ? true : undefined}
        >
          {detail}
        </span>
      </span>
      <ArrowRightLeft className="id-change" size={14} aria-hidden />
    </button>
  );

  const fault = card.state === "fault" &&
    card.fault &&
    !card.operationError && (
      <Slip id={noteId} text={card.fault.title}>
        {!restartRequired && !selectRequired && (
          <button
            type="button"
            className="btn btn-small"
            onClick={h.fault}
            aria-label={`Select ${s.name} again`}
          >
            Select again
          </button>
        )}
      </Slip>
    );

  return (
    <article
      ref={host as React.RefObject<HTMLElement>}
      id={`card-${card.uid}`}
      className="card source-card row"
      data-state={card.state}
      data-muted={card.muted || undefined}
      data-colour={card.colour}
      data-expand={expand || undefined}
      data-note={hasNote || undefined}
      aria-label={`Source${ordinal ? ` ${ordinal}` : ""}: ${s.name}${tag ? `, ${tag.toLowerCase()}` : ""}`}
    >
      <div className="card-row identity-row">
        {ordinal && (
          <span className="ch tabular" aria-hidden>
            {ordinal}
          </span>
        )}
        {identity}
        {tag && <span className="tag state-tag">{tag}</span>}
        <Remove label={`Remove ${s.name}`} onClick={h.remove} />
      </div>
      <div className="card-row controls-row">
        {capture}
        <MuteToggle
          muted={card.muted}
          onChange={h.mute}
          what={`${s.name} in the mix`}
        />
        <ChannelFader
          levelKey={`src:${card.uid}`}
          mode={expand ? "simple" : mode}
          gainDb={card.gainDb}
          onChange={h.gain}
          onCommit={h.commit}
          label={`${s.name} gain`}
          muted={card.muted}
          idle={idle}
          readout={readout}
        />
      </div>
      {fault && <div className="card-row fault-row">{fault}</div>}
      {card.operationError && (
        <div className="card-row fault-row">
          <Slip id={noteId} text={card.operationError.message} compact>
            <button
              type="button"
              className="btn btn-small btn-quiet"
              onClick={h.dismiss}
            >
              Dismiss
            </button>
          </Slip>
        </div>
      )}
      {expand && <SourceHistory levelKey={`src:${card.uid}`} />}
    </article>
  );
}

/* ------------------------------------------------------------ destinations */

export interface DestHandlers {
  dismiss: () => void;
  change: () => void;
  remove: () => void;
  confirmRemove: (on: boolean) => void;
  connect: () => void;
  disconnect: () => void;
  volume: (v: number | null) => Promise<boolean>;
  commit: () => void;
  mute: (m: boolean) => void;
  fault: () => void;
  details: () => void;
  bot: () => void;
}

function Preflight({
  bot,
  botName,
  capturing,
}: {
  bot: boolean;
  botName: string;
  capturing: number;
}) {
  const items: [boolean, string][] = [
    [bot, bot ? `${botName} is ready` : "Set up the Discord bot"],
    [
      capturing > 0,
      capturing > 0
        ? `${capturing} source${capturing > 1 ? "s" : ""} capturing`
        : "No source is capturing — the room would hear silence",
    ],
  ];
  return (
    <ul className="preflight" aria-label="Before you connect">
      {items.map(([ok, text]) => (
        <li key={text} data-ok={ok || undefined}>
          {ok ? (
            <Check size={15} aria-hidden />
          ) : (
            <TriangleAlert size={15} aria-hidden />
          )}
          {text}
        </li>
      ))}
    </ul>
  );
}

export function DestCard({
  card,
  ordinal,
  mode,
  expand,
  compact,
  sourcesCapturing,
  botReady,
  botName,
  h,
}: {
  card: Dest;
  ordinal: number | undefined;
  mode: MeterMode;
  expand: boolean;
  compact?: boolean | undefined;
  sourcesCapturing: number;
  botReady: boolean;
  botName: string;
  h: DestHandlers;
}) {
  const server = card.server;
  const channel = card.channel;
  const { host, readout } = useLevel(`dst:${card.uid}`);
  const local = card.kind === "playback";
  const sk = local && card.state === "live" ? "playing" : statusKey(card.state);
  const statusLabel =
    card.connection.phase === "error" &&
    card.connection.code === "cleanup-incomplete"
      ? "Unconfirmed"
      : destinationStatusLabel(card.state, card.kind);
  const identityName = local
    ? channel.name
    : `${channel.name} on ${server.name}`;
  const detailsLabel = `${local ? "Playback output" : "Connection"} details for ${identityName}`;
  const errorCode =
    card.operationCommand === "connect-destination" && card.operationError
      ? card.operationError.code
      : card.connection.phase === "error"
        ? card.connection.code
        : undefined;
  const cleanupUnconfirmed =
    card.state === "cleanup-incomplete" || errorCode === "cleanup-incomplete";
  const notice = useIdentityNotice(
    `${card.kind}:${card.deviceId ?? `${card.serverId}:${card.channelId}`}`,
    channel.name,
    card.state === "live",
    card.state === "fault" || card.state === "cleanup-incomplete",
  );
  const live = card.state === "live";
  const busy = card.state === "connecting" || card.state === "recovering";
  const faulted = !!(
    (card.state === "fault" || card.state === "cleanup-incomplete") &&
    card.fault &&
    !card.confirmRemove
  );
  const hasNet = live || card.state === "recovering";
  const noteId = `destination-note-${card.uid}`;
  const hasNote = Boolean(
    card.operationError || card.fault || card.state === "recovering",
  );

  let action: ReactNode;
  if (cleanupUnconfirmed) {
    const label = local ? "Stop" : "Disconnect";
    action = (
      <button
        type="button"
        className="btn btn-connect"
        onClick={h.disconnect}
        aria-describedby={hasNote ? noteId : undefined}
        aria-label={`${label} ${identityName}`}
      >
        {label}
      </button>
    );
  } else if (card.state === "disconnecting") {
    action = (
      <button
        type="button"
        className="btn btn-connect"
        aria-disabled="true"
        aria-describedby={hasNote ? noteId : undefined}
        aria-label={`${local ? "Stopping" : "Leaving"} ${identityName}`}
      >
        <LoaderCircle size={14} className="spin" />
        {local ? "Stopping" : "Leaving"}
      </button>
    );
  } else if (card.state === "idle" || card.state === "fault") {
    const setup =
      !local &&
      (!botReady ||
        errorCode === "authentication" ||
        errorCode === "configuration");
    const change = errorCode === "invalid-selection";
    const label = setup
      ? "Set up bot"
      : change
        ? local
          ? "Change device"
          : "Change room"
        : local
          ? "Play"
          : "Connect";
    action = (
      <button
        type="button"
        className="btn btn-primary btn-connect"
        onClick={setup ? h.bot : change ? h.change : h.connect}
        aria-describedby={hasNote ? noteId : undefined}
        aria-label={`${label} ${setup ? `for ${identityName}` : identityName}`}
      >
        {label}
      </button>
    );
  } else {
    const label =
      card.state === "connecting" ? "Cancel" : local ? "Stop" : "Disconnect";
    action = (
      <button
        type="button"
        className="btn btn-connect"
        data-on
        onClick={h.disconnect}
        aria-describedby={hasNote ? noteId : undefined}
        aria-label={`${label} ${identityName}`}
      >
        {label}
      </button>
    );
  }

  const badge = (
    <button
      type="button"
      className="badge"
      data-status={sk}
      onClick={() => {
        if (hasNet) h.details();
      }}
      disabled={!hasNet}
      title={
        cleanupUnconfirmed
          ? "Cleanup unconfirmed"
          : hasNet
            ? detailsLabel
            : statusLabel
      }
      aria-label={
        cleanupUnconfirmed
          ? "Cleanup unconfirmed"
          : hasNet
            ? `${statusLabel}. ${detailsLabel}`
            : statusLabel
      }
    >
      <span className="badge-dot" aria-hidden />
      {statusLabel}
    </button>
  );

  const identity = (
    <button
      type="button"
      className="identity"
      onClick={() => {
        if (!cleanupUnconfirmed && card.state !== "disconnecting") h.change();
      }}
      aria-disabled={cleanupUnconfirmed || card.state === "disconnecting"}
      aria-describedby={hasNote ? noteId : undefined}
      title={`${local ? "Change device" : "Change room"}: ${identityName}`}
      aria-label={`${local ? "Change device" : "Change room"} ${identityName}`}
    >
      {local ? (
        <SourceGlyph glyph="playback" tint="#6b7f8e" />
      ) : (
        <ServerTile
          initials={server.initials}
          tint={server.tint}
          iconUrl={server.iconUrl}
        />
      )}
      <span className="id-text">
        <span className="id-name">
          {!local && (
            <Volume2 size={14} className="id-channel-icon" aria-hidden />
          )}
          {channel.name}
        </span>
        <span className="id-detail" data-notice={notice ? true : undefined}>
          {notice ?? server.name}
        </span>
      </span>
      <ArrowRightLeft className="id-change" size={14} aria-hidden />
    </button>
  );

  const remove = (
    <Remove
      label={`Remove ${channel.name}`}
      onClick={() => (live || busy ? h.confirmRemove(true) : h.remove())}
    />
  );

  let body: ReactNode = null;
  if (card.confirmRemove) {
    body = (
      <div
        className="confirm"
        role="alertdialog"
        aria-label={local ? "Stop and remove" : "Disconnect and remove"}
      >
        <span className="confirm-text">
          {local ? "Stop" : "Disconnect from"} {channel.name} and remove it?
        </span>
        <span className="confirm-actions">
          <button type="button" className="btn btn-danger" onClick={h.remove}>
            {local ? "Stop & remove" : "Disconnect & remove"}
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => h.confirmRemove(false)}
          >
            Keep
          </button>
        </span>
      </div>
    );
  } else if (faulted && card.fault && !card.operationError) {
    body = (
      <Slip id={noteId} text={card.fault.title}>
        {errorCode === "permission-denied" && (
          <HelpLink to="guide">Permissions guide</HelpLink>
        )}
      </Slip>
    );
  } else if (card.operationError) {
    body = (
      <Slip id={noteId} text={card.operationError.message} compact>
        <button type="button" className="btn btn-small" onClick={h.dismiss}>
          Dismiss
        </button>
      </Slip>
    );
  } else if (card.state === "connecting" && local) {
    body = (
      <span className="net-line" aria-live="polite">
        Opening {channel.name}…
      </span>
    );
  } else if (card.state === "connecting") {
    body = <ConnectLog key={card.op} stage={card.stage} room={channel.name} />;
  } else if (hasNet) {
    body = (
      <>
        {card.state === "recovering" && (
          <span id={noteId} className="net-line warn">
            <TriangleAlert size={13} aria-hidden />
            {card.connection.phase === "recovering"
              ? card.connection.message
              : "Reconnecting…"}
            {card.attempt !== undefined &&
              card.attempt > 1 &&
              ` Attempt ${card.attempt}.`}
          </span>
        )}
        {/* Density is chosen by CSS from the height the card actually gets:
            tiles + graph → tiles → one status line. */}
        <button
          type="button"
          className="stats-btn d-tiles"
          onClick={h.details}
          title={detailsLabel}
          aria-label={detailsLabel}
        >
          <StatTiles uid={card.uid} local={local} />
        </button>
        <div className="chart-slot d-chart">
          <NetChart uid={card.uid} seconds={60} local={local} />
        </div>
        {live && (
          <span className="net-line tabular d-meta">
            <Timer size={13} aria-hidden /> <OnAirTime since={card.liveAt} />{" "}
            {local ? "playing" : "on air"}
          </span>
        )}
        <button
          type="button"
          className="net-line tabular d-line"
          onClick={h.details}
          title={detailsLabel}
          aria-label={detailsLabel}
        >
          <InlinePing uid={card.uid} />
          {live && (
            <>
              <span className="sep" />
              <Timer size={13} aria-hidden /> <OnAirTime since={card.liveAt} />
            </>
          )}
        </button>
      </>
    );
  } else if (card.state === "idle") {
    body = (
      <>
        <div className="d-preflight">
          {local ? (
            <ul className="preflight">
              <li data-ok>
                <Check size={15} aria-hidden />
                Explicit playback device selected
              </li>
              <li data-ok={sourcesCapturing > 0 || undefined}>
                {sourcesCapturing > 0 ? (
                  <Check size={15} aria-hidden />
                ) : (
                  <TriangleAlert size={15} aria-hidden />
                )}
                {sourcesCapturing > 0
                  ? `${sourcesCapturing} source${sourcesCapturing > 1 ? "s" : ""} capturing`
                  : "No source is capturing — playback would be silent"}
              </li>
            </ul>
          ) : (
            <Preflight
              bot={botReady}
              botName={botName}
              capturing={sourcesCapturing}
            />
          )}
        </div>
        <span className="net-line quiet d-hint">
          {sourcesCapturing === 0
            ? "No source is capturing yet"
            : local
              ? "Press Play to start this output"
              : `Joins as ${botName}`}
        </span>
      </>
    );
  }

  return (
    <article
      ref={host as React.RefObject<HTMLElement>}
      id={`card-${card.uid}`}
      className="card dest-card panel"
      data-state={card.state}
      data-kind={card.kind}
      data-status={sk}
      data-muted={card.muted || undefined}
      data-expand={expand || undefined}
      data-compact={compact || undefined}
      aria-label={`Destination${ordinal ? ` ${ordinal}` : ""}: ${channel.name}${local ? ", playback output" : ` on ${server.name}`}, ${statusLabel}`}
    >
      <div className="card-row identity-row">
        {ordinal && (
          <span className="ch dest-number tabular" aria-hidden>
            {ordinal}
          </span>
        )}
        {identity}
        {badge}
        {remove}
      </div>
      {!card.confirmRemove && (
        <div className="card-row controls-row">
          {action}
          <MuteToggle
            muted={card.muted}
            onChange={h.mute}
            what={identityName}
          />
          <ChannelFader
            levelKey={`dst:${card.uid}`}
            mode={mode}
            gainDb={card.volumeDb}
            onChange={h.volume}
            onCommit={h.commit}
            label={`${identityName} volume`}
            muted={card.muted}
            idle={
              live || card.state === "recovering"
                ? undefined
                : local
                  ? "Not playing"
                  : "Not sending"
            }
            readout={readout}
          />
        </div>
      )}
      <div className="dest-body">{body}</div>
    </article>
  );
}

/* -------------------------------------------------------------------- slots */

export interface Chip {
  uid: string;
  label: string;
  colour?: number | undefined;
  state: string;
  stateLabel?: string;
}

function jump(uid: string) {
  const el = document.getElementById(`card-${uid}`);
  if (!el) return;
  el.scrollIntoView({
    behavior: matchMedia("(prefers-reduced-motion: reduce)").matches
      ? "instant"
      : "smooth",
    block: "nearest",
    inline: "nearest",
  });
  el.dataset.flash = "";
  window.setTimeout(() => delete el.dataset.flash, 900);
  el.querySelector<HTMLElement>(".identity")?.focus({ preventScroll: true });
}

export function SlotHead({
  title,
  itemLabel,
  summary,
  chips,
  addLabel,
  onAdd,
  addDisabled,
  addHint,
}: {
  title: string;
  itemLabel: "Source" | "Destination";
  summary?: string | undefined;
  chips: Chip[];
  addLabel: string;
  onAdd: () => void;
  addDisabled?: boolean | undefined;
  addHint?: string | undefined;
}) {
  return (
    <header className="slot-head">
      <h2 className="slot-title">{title}</h2>
      {summary && (
        <span className="slot-summary" title={summary}>
          {summary}
        </span>
      )}
      {chips.length > 1 && (
        <nav className="chips" aria-label={`Jump to ${title.toLowerCase()}`}>
          {chips.map((c, i) => (
            <button
              key={c.uid}
              type="button"
              className="ch chip tabular"
              data-colour={c.colour}
              data-state={c.state}
              aria-label={`Show ${itemLabel.toLowerCase()} ${i + 1}: ${c.label}, ${c.stateLabel ?? c.state}`}
              title={`${itemLabel} ${i + 1}: ${c.label}, ${c.stateLabel ?? c.state}`}
              onClick={() => jump(c.uid)}
            >
              {i + 1}
            </button>
          ))}
        </nav>
      )}
      <span className="flex" />
      <button
        type="button"
        className="btn btn-add"
        onClick={onAdd}
        disabled={addDisabled}
        title={addHint ?? addLabel}
        aria-label={addLabel}
      >
        <Plus size={15} />
        <span>Add</span>
      </button>
    </header>
  );
}

export function Empty({
  text,
  sub,
  action,
  onAction,
}: {
  text: string;
  sub: string;
  action: string;
  onAction: () => void;
}) {
  return (
    <div className="empty">
      <div className="empty-copy">
        <span className="empty-text">{text}</span>
        <span className="empty-sub">{sub}</span>
      </div>
      <button type="button" className="btn btn-primary" onClick={onAction}>
        <Plus size={15} />
        {action}
      </button>
    </div>
  );
}
