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
import { STAGES, STATUS, stageLine, statusKey } from "./copy";
import { ServerTile, SourceGlyph } from "./icons";
import type {
  DestCard as Dest,
  MeterMode,
  SourceCard as Source,
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
      if (r.preClip) clipUntil = now + 1500;
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
        const cols = Math.min(ring?.buf.length ?? 0, Math.floor(w / col));
        // History and the current level share one well: older samples fade towards the left,
        // the newest few are drawn brightest, and the live bar runs along the floor.
        ctx.fillStyle = colour;
        for (let k = 0; ring && k < cols; k++) {
          const f = ring.at(k);
          if (Number.isNaN(f) || f <= 0) continue;
          const age = k / Math.max(1, cols - 1);
          ctx.globalAlpha = k < 3 ? 0.9 : 0.62 - 0.5 * age;
          ctx.fillRect(w - (k + 1) * col, h - f * h, col, f * h);
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
      () => {
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
        if (!ring) return;
        const n = ring.buf.length;
        let peak = 0;
        const x = (k: number) => plotW - (k / (n - 1)) * plotW;
        const y = (f: number) => padT + plotH - f * plotH;
        ctx.beginPath();
        ctx.moveTo(x(n - 1), y(0));
        for (let k = n - 1; k >= 0; k--) {
          const f = ring.at(k);
          const v = Number.isNaN(f) ? 0 : f;
          peak = Math.max(peak, v);
          ctx.lineTo(x(k), y(v));
        }
        ctx.lineTo(x(0), y(0));
        ctx.closePath();
        ctx.globalAlpha = 0.16;
        ctx.fillStyle = colour;
        ctx.fill();
        ctx.globalAlpha = 1;
        ctx.beginPath();
        for (let k = n - 1; k >= 0; k--) {
          const f = ring.at(k);
          const v = Number.isNaN(f) ? 0 : f;
          if (k === n - 1) ctx.moveTo(x(k), y(v));
          else ctx.lineTo(x(k), y(v));
        }
        ctx.strokeStyle = colour;
        ctx.lineWidth = 1.5 * dpr;
        ctx.lineJoin = "round";
        ctx.stroke();
        if (peakRef.current) {
          const db = peak * 60 - 60;
          peakRef.current.textContent =
            peak > 0 ? `Peak ${fmtDb(db)} dBFS` : "Silent";
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
          <span className="meter-clip" title="Input clipped before gain" />
          {mode === "simple" && (
            <span className="meter-scale">
              {SCALE.map((g) => (
                <span key={g} style={{ left: `${fraction(g) * 100}%` }} />
              ))}
            </span>
          )}
          {idle && <span className="mf-idle">{idle}</span>}
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

export function StatTiles({
  compact,
}: {
  uid: string;
  compact?: boolean | undefined;
}) {
  const store = useTelemetryStore();
  const host = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          const n = store.net();
          const values = [n.ping, n.buffer, n.drops];
          host.current
            ?.querySelectorAll<HTMLElement>("[data-value]")
            .forEach((el, i) => {
              el.textContent =
                values[i] === null ? "—" : `${Math.round(values[i] ?? 0)}`;
            });
        },
        2,
      ),
    [store],
  );
  return (
    <span ref={host} className="stats" data-compact={compact || undefined}>
      <span
        className="stat"
        title="Discord gateway round trip, when available; voice-server ping is unavailable"
      >
        <span className="stat-label">Gateway</span>
        <span className="stat-value tabular">
          <span data-value>—</span>
          <small> ms</small>
        </span>
      </span>
      <span
        className="stat"
        title="Audio queued inside Echocord; this is not listener delay"
      >
        <span className="stat-label">Buffer</span>
        <span className="stat-value tabular">
          <span data-value>—</span>
          <small> ms</small>
        </span>
      </span>
      <span
        className="stat"
        title="Frames dropped from the bounded native audio queue"
      >
        <span className="stat-label">Dropped</span>
        <span className="stat-value tabular" data-value>
          —
        </span>
      </span>
    </span>
  );
}

/** Native buffer observations only; unavailable gateway values do not become a series. */
export function NetChart({
  seconds,
  minHeight = 28,
}: {
  uid: string;
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
        () => {
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
          const n = store.net();
          const samples = Math.min(seconds * 2, n.buffer2m.buf.length);
          let max = 0;
          for (let k = 0; k < samples; k++) {
            const v = n.buffer2m.at(k);
            if (Number.isFinite(v)) max = Math.max(max, v);
          }
          const top = Math.max(60, Math.ceil(max / 20) * 20);
          const right = 30 * dpr;
          const padT = 6 * dpr;
          const padB = 16 * dpr;
          const plotW = w - right;
          const plotH = h - padT - padB;
          const x = (k: number) => plotW - (k / (samples - 1)) * plotW;
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
          ctx.strokeStyle = cs.getPropertyValue("--net-b").trim();
          ctx.beginPath();
          let connected = false;
          for (let k = samples - 1; k >= 0; k--) {
            const v = n.buffer2m.at(k);
            if (!Number.isFinite(v)) {
              connected = false;
              continue;
            }
            if (!connected) {
              ctx.moveTo(x(k), y(v));
              connected = true;
            } else ctx.lineTo(x(k), y(v));
          }
          ctx.stroke();
          const at = hover.current;
          const tooltip = tip.current;
          if (tooltip) {
            tooltip.hidden = at === null;
            if (at !== null) {
              const v = n.buffer2m.at(at);
              tooltip.textContent = `${at === 0 ? "now" : `−${Math.round(at / 2)} s`} · Buffer ${Number.isFinite(v) ? `${Math.round(v)} ms` : "unavailable"}`;
              tooltip.style.left = `${Math.max(0, Math.min(cv.clientWidth - 150, x(at) / dpr + 12))}px`;
              ctx.fillStyle = cs.getPropertyValue("--crosshair").trim();
              ctx.fillRect(
                Math.round(x(at)),
                padT,
                Math.max(1, Math.round(dpr)),
                plotH,
              );
            }
          }
        },
        2,
      ),
    [store, seconds, minHeight],
  );
  return (
    <div className="net-chart" ref={box}>
      <div className="net-legend" aria-hidden>
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
                (seconds * 2 - 1),
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
  stage?: string | undefined;
  room: string;
}) {
  const times = useRef<Record<string, number>>({});
  useEvery(200);
  const now = performance.now();
  if (stage && times.current[stage] === undefined) times.current[stage] = now;
  const idx = STAGES.findIndex((x) => x.id === stage);
  const secs = (ms: number) => `${(ms / 1000).toFixed(1)} s`;
  return (
    <ol className="connect-log d-log" aria-label="Connection progress">
      {STAGES.map((st, i) => {
        const status = i < idx ? "done" : i === idx ? "now" : "wait";
        const start = times.current[st.id];
        const next = times.current[STAGES[i + 1]?.id ?? ""];
        let result: ReactNode = "—";
        if (status === "done")
          result = (
            <>
              <span className="log-ok">OK</span>
              {start !== undefined && next !== undefined && (
                <span className="log-time">{secs(next - start)}</span>
              )}
            </>
          );
        else if (status === "now")
          result = (
            <>
              <LoaderCircle size={13} className="spin" aria-hidden />
              {start !== undefined && (
                <span className="log-time">{secs(now - start)}</span>
              )}
            </>
          );
        return (
          <li
            key={st.id}
            data-status={status}
            aria-current={status === "now" ? "step" : undefined}
          >
            <span className="log-mark" aria-hidden>
              {status === "done" ? (
                <Check size={13} />
              ) : status === "now" ? (
                <span className="log-dot" />
              ) : (
                <span className="log-dot" />
              )}
            </span>
            <span className="log-label">{LOG_LABEL[st.id]?.(room)}…</span>
            <span className="log-result tabular">{result}</span>
          </li>
        );
      })}
    </ol>
  );
}

function InlinePing({ uid: _uid }: { uid: string }) {
  const store = useTelemetryStore();
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          const n = store.net();
          if (ref.current)
            ref.current.textContent = `${n.buffer === null ? "—" : Math.round(n.buffer)} ms buffer · ${n.drops ?? "—"} dropped`;
        },
        2,
      ),
    [store],
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
  children,
}: {
  text: string;
  children?: ReactNode | undefined;
}) {
  return (
    <div className="slip" role="alert">
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

const sourceTag = (c: Source) =>
  c.state === "opening"
    ? "Opening…"
    : c.state === "stopped"
      ? "Stopped"
      : c.muted && c.state === "capturing"
        ? "Muted"
        : null;

/* ----------------------------------------------------------------- sources */

export interface SourceHandlers {
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
  index,
  mode,
  expand,
  h,
}: {
  card: Source;
  index: number;
  mode: MeterMode;
  expand?: boolean | undefined;
  h: SourceHandlers;
}) {
  const s = card.identity;
  const { host, readout } = useLevel(`src:${card.uid}`);
  const capturing = card.state === "capturing";
  const opening = card.state === "opening";
  const detail = card.restored
    ? "From last session · press Capture to reattach"
    : s.detail;
  const tag = sourceTag(card);
  const idle =
    card.state === "stopped"
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
        title={`Stop capturing ${s.name}`}
        aria-label={`Stop capturing ${s.name}`}
      >
        {opening ? (
          <LoaderCircle size={14} className="spin" />
        ) : (
          <Square size={11} fill="currentColor" />
        )}
        <span>{opening ? "Opening" : "Stop"}</span>
      </button>
    ) : (
      <button
        type="button"
        className="btn btn-capture btn-primary"
        onClick={h.capture}
        title={`Capture ${s.name}`}
        aria-label={`Capture ${s.name}`}
      >
        <Circle size={11} fill="currentColor" />
        <span>Capture</span>
      </button>
    );

  const identity = (
    <button
      type="button"
      className="identity"
      onClick={h.change}
      title={`Change ${s.name}`}
      aria-label={`Change ${s.name}`}
    >
      <SourceGlyph glyph={s.glyph} tint={s.tint} />
      <span className="id-text">
        <span className="id-name">{s.name}</span>
        <span className="id-detail">{detail}</span>
      </span>
      <ArrowRightLeft className="id-change" size={14} aria-hidden />
    </button>
  );

  const fault = card.state === "fault" && card.fault && (
    <Slip text={card.fault.title}>
      <button type="button" className="btn btn-small" onClick={h.fault}>
        {card.fault.actionLabel}
      </button>
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
      aria-label={`Source ${index + 1}: ${s.name}${tag ? `, ${tag.toLowerCase()}` : ""}`}
    >
      <div className="card-row identity-row">
        <span className="ch tabular" aria-hidden>
          {index + 1}
        </span>
        {identity}
        {tag && <span className="tag state-tag">{tag}</span>}
        <Remove label={`Remove ${s.name}`} onClick={h.remove} />
      </div>
      {fault ? (
        <div className="card-row fault-row">{fault}</div>
      ) : (
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
      )}
      {expand && <SourceHistory levelKey={`src:${card.uid}`} />}
    </article>
  );
}

/* ------------------------------------------------------------ destinations */

export interface DestHandlers {
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
}

function Preflight({ bot, capturing }: { bot: boolean; capturing: number }) {
  const items: [boolean, string][] = [
    [bot, bot ? "The bot is ready" : "Set up the Discord bot"],
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
  mode,
  expand,
  compact,
  sourcesCapturing,
  botReady,
  h,
}: {
  card: Dest;
  mode: MeterMode;
  expand: boolean;
  compact?: boolean | undefined;
  sourcesCapturing: number;
  botReady: boolean;
  h: DestHandlers;
}) {
  const server = card.server;
  const channel = card.channel;
  const { host, readout } = useLevel(`dst:${card.uid}`);
  const sk = statusKey(card.state);
  const live = card.state === "live";
  const busy = card.state === "connecting" || card.state === "recovering";
  const faulted = !!(
    (card.state === "fault" || card.state === "cleanup-incomplete") &&
    card.fault &&
    !card.confirmRemove
  );
  const hasNet = live || card.state === "recovering";

  let action: ReactNode;
  if (card.state === "idle") {
    action = (
      <button
        type="button"
        className="btn btn-primary btn-connect"
        onClick={h.connect}
      >
        Connect
      </button>
    );
  } else if (card.state === "cleanup-incomplete") {
    action = (
      <button type="button" className="btn btn-connect" onClick={h.disconnect}>
        Retry disconnect
      </button>
    );
  } else if (card.state === "fault") {
    action = (
      <button type="button" className="btn btn-connect" onClick={h.connect}>
        Retry
      </button>
    );
  } else if (card.state === "disconnecting") {
    action = (
      <button type="button" className="btn btn-connect" disabled>
        <LoaderCircle size={14} className="spin" />
        Leaving
      </button>
    );
  } else {
    action = (
      <button
        type="button"
        className="btn btn-connect"
        data-on
        onClick={h.disconnect}
      >
        {busy ? "Cancel" : "Disconnect"}
      </button>
    );
  }

  const badge = (
    <button
      type="button"
      className="badge"
      data-status={sk}
      onClick={h.details}
      disabled={!hasNet}
      title={hasNet ? "Connection details" : undefined}
    >
      <span className="badge-dot" aria-hidden />
      {STATUS[sk]}
    </button>
  );

  const identity = (
    <button
      type="button"
      className="identity"
      onClick={h.change}
      title="Change room"
      aria-label="Change room"
    >
      <ServerTile
        initials={server.initials}
        tint={server.tint}
        iconUrl={server.iconUrl}
      />
      <span className="id-text">
        <span className="id-name">
          <Volume2 size={14} className="id-channel-icon" aria-hidden />
          {channel.name}
        </span>
        <span className="id-detail">{server.name}</span>
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
        aria-label="Disconnect and remove"
      >
        <span className="confirm-text">
          Disconnect from {channel.name} and remove it?
        </span>
        <span className="confirm-actions">
          <button type="button" className="btn btn-danger" onClick={h.remove}>
            Disconnect &amp; remove
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
  } else if (faulted && card.fault) {
    body = (
      <Slip text={card.fault.title}>
        <button type="button" className="btn btn-small" onClick={h.fault}>
          {card.fault.actionLabel}
        </button>
      </Slip>
    );
  } else if (card.state === "connecting") {
    const idx = STAGES.findIndex((x) => x.id === card.stage);
    body = (
      <>
        <ConnectLog key={card.op} stage={card.stage} room={channel.name} />
        <div className="stages d-stage-line" aria-live="polite">
          <ol className="stage-steps">
            {STAGES.map((st, i) => (
              <li
                key={st.id}
                data-done={i < idx || undefined}
                data-now={i === idx || undefined}
              >
                {st.label}
              </li>
            ))}
          </ol>
          <span className="stage-line">
            {stageLine(card.stage, channel.name)}
          </span>
        </div>
      </>
    );
  } else if (hasNet) {
    body = (
      <>
        {card.state === "recovering" && (
          <span className="net-line warn" aria-live="polite">
            <TriangleAlert size={13} aria-hidden /> Connection dropped ·
            retrying ({card.attempt ?? 1} of 3)
          </span>
        )}
        {/* Density is chosen by CSS from the height the card actually gets:
            tiles + graph → tiles → one status line. */}
        <button
          type="button"
          className="stats-btn d-tiles"
          onClick={h.details}
          title="Connection details"
        >
          <StatTiles uid={card.uid} />
        </button>
        <div className="chart-slot d-chart">
          <NetChart uid={card.uid} seconds={60} />
        </div>
        <span className="net-line tabular d-meta">
          <Timer size={13} aria-hidden /> <OnAirTime since={card.liveAt} /> on
          air
        </span>
        <button
          type="button"
          className="net-line tabular d-line"
          onClick={h.details}
          title="Connection details"
        >
          <InlinePing uid={card.uid} /> <span className="sep" />
          <Timer size={13} aria-hidden /> <OnAirTime since={card.liveAt} />
        </button>
      </>
    );
  } else if (card.state === "idle") {
    body = (
      <>
        <div className="d-preflight">
          <Preflight bot={botReady} capturing={sourcesCapturing} />
        </div>
        <span className="net-line quiet d-hint">
          {sourcesCapturing === 0
            ? "No source is capturing yet"
            : "Ready to join this room"}
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
      data-status={sk}
      data-muted={card.muted || undefined}
      data-expand={expand || undefined}
      data-compact={compact || undefined}
      aria-label={`${channel.name} on ${server.name}, ${STATUS[sk]}`}
    >
      <>
        <div className="card-row identity-row">
          {identity}
          {badge}
          {remove}
        </div>
        {!faulted && !card.confirmRemove && (
          <div className="card-row controls-row">
            {action}
            <MuteToggle
              muted={card.muted}
              onChange={h.mute}
              what={channel.name}
            />
            <ChannelFader
              levelKey={`dst:${card.uid}`}
              mode={mode}
              gainDb={card.volumeDb}
              onChange={h.volume}
              onCommit={h.commit}
              label={`${channel.name} volume`}
              muted={card.muted}
              idle={live ? undefined : "Not sending"}
              readout={readout}
            />
          </div>
        )}
        <div className="dest-body">{body}</div>
      </>
    </article>
  );
}

/* -------------------------------------------------------------------- slots */

export interface Chip {
  uid: string;
  label: string;
  colour?: number | undefined;
  state: string;
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
  summary,
  chips,
  addLabel,
  onAdd,
  addDisabled,
  addHint,
}: {
  title: string;
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
      {summary && <span className="slot-summary">{summary}</span>}
      {chips.length > 1 && (
        <nav className="chips" aria-label={`Jump to ${title.toLowerCase()}`}>
          {chips.map((c, i) => (
            <button
              key={c.uid}
              type="button"
              className="ch chip tabular"
              data-colour={c.colour}
              data-state={c.state}
              aria-label={`Show ${c.label}`}
              title={c.label}
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
