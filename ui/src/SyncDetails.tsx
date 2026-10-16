/* Read-only diagnostics for Sync correction and Limit loud peaks. */
import { useId, useRef } from "react";
import { AUDIO_PROCESSING, SYNC_COPY, SYNC_STATE } from "./copy";
import type { AppSettings } from "./generated";
import { SourceGlyph } from "./icons";
import type { DestCard, Glyph, SourceCard } from "./model";
import { Sheet } from "./sheets";
import { LimiterDetail, SyncFacts, useSyncStages } from "./syncView";

interface Row {
  key: string;
  name: string;
  ordinal: number | undefined;
  colour: number | undefined;
  glyph: Glyph;
  tint: string;
}

function SyncRow({ row }: { row: Row }) {
  const heading = useId();
  const state = useRef<HTMLSpanElement>(null);
  return (
    <li className="sync-row" aria-labelledby={heading}>
      <div className="sync-row-head">
        {row.ordinal !== undefined && (
          <span
            className={`ch tabular${row.colour === undefined ? " dest-number" : ""}`}
            data-colour={row.colour}
            aria-hidden
          >
            {row.ordinal}
          </span>
        )}
        <SourceGlyph glyph={row.glyph} tint={row.tint} />
        <h4 className="sync-name" id={heading}>
          <span className="sync-title">{row.name}</span>
          <span className="sr-only">, </span>
          <span ref={state} className="tag sync-tag" data-sync="stale">
            {SYNC_STATE.stale}
          </span>
        </h4>
      </div>
      <dl className="facts-list tabular sync-facts">
        <SyncFacts stageKey={row.key} stateRef={state} />
      </dl>
    </li>
  );
}

function Section({ title, rows }: { title: string; rows: Row[] }) {
  if (!rows.length) return null;
  return (
    <section className="sync-section">
      <h3 className="set-title">{title}</h3>
      <ul className="sync-list">
        {rows.map((row) => (
          <SyncRow key={row.key} row={row} />
        ))}
      </ul>
    </section>
  );
}

export function SyncDetails({
  sources,
  dests,
  settings,
  onClose,
}: {
  sources: SourceCard[];
  dests: DestCard[];
  settings: AppSettings;
  onClose: () => void;
}) {
  const stages = new Set(useSyncStages());
  const sourceRows: Row[] = sources.flatMap((card, index) =>
    stages.has(`src:${card.uid}`)
      ? [
          {
            key: `src:${card.uid}`,
            name: card.identity.name,
            ordinal: sources.length > 1 ? index + 1 : undefined,
            colour: card.colour,
            glyph: card.identity.glyph,
            tint: card.identity.tint,
          },
        ]
      : [],
  );
  const outputRows: Row[] = dests.flatMap((card, index) =>
    card.kind === "playback" && stages.has(`dst:${card.uid}`)
      ? [
          {
            key: `dst:${card.uid}`,
            name: card.channel.name,
            ordinal: dests.length > 1 ? index + 1 : undefined,
            colour: undefined,
            glyph: "playback" as const,
            tint: "#6b7f8e",
          },
        ]
      : [],
  );
  const empty = !sourceRows.length && !outputRows.length;
  return (
    <Sheet title={AUDIO_PROCESSING.details} onClose={onClose}>
      <p className="setup-intro sync-intro">{SYNC_COPY.intro}</p>
      <dl className="facts-list sync-summary">
        <dt>{AUDIO_PROCESSING.sync.name}</dt>
        <dd>{settings.driftCorrection ? "On" : "Off"}</dd>
        <dt>{AUDIO_PROCESSING.limiter.name}</dt>
        <dd>
          {settings.mixLimiter ? SYNC_COPY.limiterOn : SYNC_COPY.limiterOff}
          <LimiterDetail enabled={settings.mixLimiter} />
        </dd>
      </dl>
      {!settings.driftCorrection && (
        <p className="sync-banner">{SYNC_COPY.off}</p>
      )}
      {empty ? (
        <p className="sync-empty">{SYNC_COPY.empty}</p>
      ) : (
        <div className="sync-columns">
          <Section title={SYNC_COPY.sources} rows={sourceRows} />
          <Section title={SYNC_COPY.outputs} rows={outputRows} />
        </div>
      )}
      <div className="sync-notes">
        <p className="conn-note">{SYNC_COPY.honesty}</p>
        <p className="conn-note">{SYNC_COPY.buffered}</p>
        <p className="conn-note">{SYNC_COPY.discord}</p>
      </div>
    </Sheet>
  );
}
