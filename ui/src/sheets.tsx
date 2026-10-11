import {
  ArrowLeft,
  Bot,
  Check,
  ChevronRight,
  Eye,
  EyeOff,
  KeyRound,
  LoaderCircle,
  RotateCw,
  Search,
  Volume2,
} from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { NetChart, OnAirTime, StatTiles } from "./cards";
import { GATEWAY_STATUS, STATUS, statusKey } from "./copy";
import type {
  AppSettings,
  BotProfile,
  Discovery,
  GatewaySnapshot,
  PlaybackDevice,
  ServerReservation,
} from "./generated";
import { HelpLink } from "./helpLinks";
import { ServerTile, SourceGlyph } from "./icons";
import type {
  BotState,
  CatalogSource,
  DestCard,
  MeterMode,
  Server,
  SourceCard,
} from "./model";
import { paint, useTelemetryStore } from "./telemetry";

function Sheet({
  title,
  onClose,
  footer,
  children,
  label,
  initialFocus,
}: {
  title: string;
  onClose: () => void;
  footer?: ReactNode | undefined;
  children: ReactNode;
  label?: string | undefined;
  initialFocus?: string | undefined;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    if (!root.contains(document.activeElement))
      root.querySelector<HTMLElement>(initialFocus ?? "input,button")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close.current();
      }
      if (e.key !== "Tab") return;
      const items = Array.from(
        root.querySelectorAll<HTMLElement>(
          'button:not(:disabled),input:not(:disabled),a[href],summary,[tabindex="0"]',
        ),
      ).filter(
        (el) =>
          el.getClientRects().length > 0 &&
          el.getAttribute("aria-disabled") !== "true",
      );
      const first = items[0];
      const last = items[items.length - 1];
      if (!first) return;
      if (
        e.shiftKey &&
        (!root.contains(document.activeElement) ||
          document.activeElement === first)
      ) {
        e.preventDefault();
        last?.focus();
      } else if (
        !e.shiftKey &&
        (!root.contains(document.activeElement) ||
          document.activeElement === last)
      ) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [initialFocus]);
  return (
    <div
      ref={ref}
      className="sheet"
      role="dialog"
      aria-modal="true"
      aria-label={label ?? title}
    >
      <header className="sheet-head">
        <button
          type="button"
          className="icon-btn"
          onClick={onClose}
          aria-label="Back"
        >
          <ArrowLeft size={18} />
        </button>
        <h2 className="sheet-title">{title}</h2>
      </header>
      <div className="sheet-body">{children}</div>
      {footer && <footer className="sheet-foot">{footer}</footer>}
    </div>
  );
}

function SearchField({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => ref.current?.focus(), []);
  return (
    <label className="search">
      <Search size={16} aria-hidden />
      <input
        ref={ref}
        type="search"
        value={value}
        placeholder={placeholder}
        aria-label={placeholder}
        onChange={(e) => onChange(e.currentTarget.value)}
      />
    </label>
  );
}

function Refresh({
  label,
  onRefresh,
  busy,
}: {
  label: string;
  onRefresh: () => void;
  busy: boolean;
}) {
  return (
    <button
      type="button"
      className="btn btn-quiet"
      onClick={onRefresh}
      disabled={busy}
    >
      {busy ? (
        <LoaderCircle size={15} className="spin" />
      ) : (
        <RotateCw size={15} />
      )}{" "}
      {label}
    </button>
  );
}

export function SourcePicker({
  sources,
  catalog,
  error,
  busy,
  discovery,
  onRefresh,
  replacing,
  onPick,
  onClose,
}: {
  sources: SourceCard[];
  catalog: CatalogSource[];
  error: string | null;
  busy: boolean;
  discovery: Discovery;
  onRefresh: () => void;
  replacing?: SourceCard | undefined;
  onPick: (id: string) => void;
  onClose: () => void;
}) {
  const current = replacing?.identity;
  const apps = catalog.filter((s) => s.kind === "application");
  const inputs = catalog.filter((s) => s.kind === "recording");
  const [tab, setTab] = useState<"application" | "recording">(
    current?.kind ?? "application",
  );
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<string | null>(null);
  const inMix = new Set(sources.map((s) => s.sourceId));
  const list = (tab === "application" ? apps : inputs).filter((s) =>
    `${s.name} ${s.detail}`.toLowerCase().includes(q.trim().toLowerCase()),
  );
  const chosen = catalog.find(
    (s) => s.id === sel && s.available && !inMix.has(s.id),
  );
  const unavailable = (s: CatalogSource) =>
    s.disabledReason
      ? s.disabledReason
      : inMix.has(s.id) && s.id !== replacing?.sourceId
        ? "Already in the mix"
        : undefined;

  return (
    <Sheet
      title={current ? `Replace ${current.name}` : "Add source"}
      onClose={onClose}
      footer={
        <>
          <Refresh
            label="Refresh"
            busy={discovery.phase === "loading"}
            onRefresh={onRefresh}
          />
          <button
            type="button"
            className="btn btn-primary"
            disabled={!chosen || busy}
            onClick={() => chosen && onPick(chosen.id)}
          >
            {chosen
              ? current
                ? `Use ${chosen.name}`
                : `Add ${chosen.name}`
              : current
                ? "Choose a source"
                : "Add source"}
          </button>
        </>
      }
    >
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      <div className="tabs" role="tablist" aria-label="Source kind">
        {(
          [
            ["application", "Applications", apps.length],
            ["recording", "Inputs", inputs.length],
          ] as const
        ).map(([id, label, n]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            className="tab"
            onClick={() => {
              setTab(id);
              setSel(null);
            }}
          >
            {label} <span className="tab-count tabular">{n}</span>
          </button>
        ))}
      </div>
      <SearchField
        value={q}
        onChange={setQ}
        placeholder={
          tab === "application" ? "Search applications" : "Search inputs"
        }
      />
      {discovery.phase === "error" && (
        <p className="field-error" role="alert">
          {discovery.message}
        </p>
      )}
      <div
        className="pick-list"
        role="listbox"
        aria-label={tab === "application" ? "Applications" : "Inputs"}
      >
        {list.map((s) => {
          const why = unavailable(s);
          const isCurrent = s.id === replacing?.sourceId;
          return (
            <div key={s.id}>
              <button
                type="button"
                role="option"
                aria-selected={sel === s.id}
                aria-disabled={!!why && !isCurrent}
                className="pick-row"
                data-disabled={(why && !isCurrent) || undefined}
                onClick={() => !why && !isCurrent && setSel(s.id)}
                onDoubleClick={() => !why && !isCurrent && onPick(s.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !why && !isCurrent) {
                    e.preventDefault();
                    onPick(s.id);
                  }
                }}
              >
                <SourceGlyph glyph={s.glyph} tint={s.tint} size={22} />
                <span className="pick-text">
                  <span className="pick-name">{s.name}</span>
                  <span className="pick-detail">
                    {why && !isCurrent
                      ? why
                      : sel === s.id && s.note
                        ? s.note
                        : s.detail}
                  </span>
                </span>
                <span className="pick-meta">
                  {isCurrent ? (
                    <span className="tag">Current</span>
                  ) : inMix.has(s.id) ? (
                    <span className="tag">In mix</span>
                  ) : null}
                  {sel === s.id && <Check size={16} className="pick-check" />}
                </span>
              </button>
            </div>
          );
        })}
        {list.length === 0 && (
          <div className="pick-empty">
            {discovery.phase === "loading"
              ? "Looking for sources…"
              : q
                ? `Nothing matches “${q}”.`
                : "No sources found. Open an application or connect a recording input, then refresh."}
          </div>
        )}
      </div>
    </Sheet>
  );
}

function DiscordRooms({
  tabs,
  reservations,
  full,
  servers,
  error,
  busy,
  discovery,
  onRefresh,
  replacing,
  onPick,
  onClose,
}: {
  tabs: ReactNode;
  reservations: ServerReservation[];
  full: boolean;
  servers: Server[];
  error: string | null;
  busy: boolean;
  discovery: Discovery;
  onRefresh: () => void;
  replacing?: DestCard | undefined;
  onPick: (serverId: string, channelId: string) => void;
  onClose: () => void;
}) {
  const [server, setServer] = useState(
    replacing?.serverId ??
      servers.find((s) => !reservations.some((r) => r.serverId === s.id))?.id ??
      servers[0]?.id ??
      "",
  );
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<{ s: string; c: string } | null>(null);
  const live = replacing && !["idle", "fault"].includes(replacing.state);
  const query = q.trim().toLowerCase();

  const groups = useMemo(() => {
    const filtered = query ? servers : servers.filter((s) => s.id === server);
    return filtered
      .map((s) => ({
        server: s,
        cats: Object.entries(
          s.channels
            .filter(
              (c) =>
                !query ||
                `${c.name} ${c.category} ${s.name}`
                  .toLowerCase()
                  .includes(query),
            )
            .reduce<Record<string, typeof s.channels>>((acc, c) => {
              acc[c.category] ??= [];
              acc[c.category]?.push(c);
              return acc;
            }, {}),
        ),
      }))
      .filter((g) => g.cats.length > 0);
  }, [query, server, servers]);

  const blockedBy = (serverId: string) =>
    reservations.find(
      (r) =>
        r.serverId === serverId &&
        (r.destinationId !== replacing?.uid ||
          r.phase === "leaving" ||
          r.phase === "blocked"),
    );
  const retiring = replacing
    ? reservations.find(
        (r) =>
          r.destinationId === replacing.uid &&
          (r.phase === "leaving" || r.phase === "blocked"),
      )
    : undefined;
  const retiringRoom = retiring
    ? (servers
        .find((s) => s.id === retiring.serverId)
        ?.channels.find((c) => c.id === retiring.channelId)?.name ??
      "the previous room")
    : null;
  const isCurrent = (s: string, c: string) =>
    replacing?.serverId === s && replacing.channelId === c;
  const canPick = (s: string, c: string) =>
    !full && !busy && !retiring && !blockedBy(s) && !isCurrent(s, c);
  useEffect(() => {
    if (servers.some((s) => s.id === server)) return;
    setServer(
      servers.find((s) => !reservations.some((r) => r.serverId === s.id))?.id ??
        servers[0]?.id ??
        "",
    );
  }, [server, servers, reservations]);

  const chosen =
    sel && canPick(sel.s, sel.c)
      ? servers
          .find((s) => s.id === sel.s)
          ?.channels.find((c) => c.id === sel.c)
      : undefined;
  let primary = replacing ? "Choose a room" : "Add room";
  if (chosen) {
    if (!replacing) primary = `Add ${chosen.name}`;
    else if (live && sel?.s === replacing.serverId)
      primary = `Move to ${chosen.name}`;
    else if (live) primary = `Switch to ${chosen.name}`;
    else primary = `Use ${chosen.name}`;
  }

  return (
    <Sheet
      title={replacing ? "Change room" : "Add destination"}
      onClose={onClose}
      footer={
        <>
          <Refresh
            label="Refresh"
            busy={discovery.phase === "loading"}
            onRefresh={onRefresh}
          />
          {live && sel && (
            <span className="foot-note">
              Briefly reconnects to change rooms
            </span>
          )}
          <button
            type="button"
            className="btn btn-primary"
            disabled={!chosen || busy}
            onClick={() => sel && canPick(sel.s, sel.c) && onPick(sel.s, sel.c)}
          >
            {primary}
          </button>
        </>
      }
    >
      {tabs}
      {retiring && (
        <p className="room-note">
          Finish cleanup for {retiringRoom} before changing this destination.
        </p>
      )}
      {full && (
        <p className="room-note">
          Four destinations are supported. Remove one to add another.
        </p>
      )}
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      <SearchField
        value={q}
        onChange={setQ}
        placeholder="Search rooms and servers"
      />
      {discovery.phase === "error" && (
        <p className="field-error" role="alert">
          {discovery.message}
        </p>
      )}
      <div className="rooms">
        <nav className="server-rail" aria-label="Servers">
          {servers.map((s) => (
            <button
              key={s.id}
              type="button"
              className="server-btn"
              aria-current={!query && server === s.id ? "true" : undefined}
              onClick={() => {
                setServer(s.id);
                setQ("");
              }}
              title={s.name}
              aria-label={s.name}
            >
              <ServerTile
                initials={s.initials}
                tint={s.tint}
                iconUrl={s.iconUrl}
              />
            </button>
          ))}
        </nav>
        <div className="room-list" role="listbox" aria-label="Voice rooms">
          {groups.map((g) => {
            const blocker = blockedBy(g.server.id);
            return (
              <div key={g.server.id} className="room-group">
                <div className="room-server">{g.server.name}</div>
                {g.cats.map(([cat, chans]) => (
                  <div key={cat} className="room-cat">
                    <div className="room-cat-name">{cat}</div>
                    {chans.map((c) => {
                      const current = isCurrent(g.server.id, c.id);
                      const disabled =
                        !!blocker || !!retiring || current || full;
                      return (
                        <button
                          key={c.id}
                          type="button"
                          role="option"
                          className="room-row"
                          aria-selected={sel?.c === c.id}
                          aria-disabled={disabled}
                          data-disabled={disabled || undefined}
                          onClick={() =>
                            !disabled &&
                            !busy &&
                            setSel({ s: g.server.id, c: c.id })
                          }
                          onDoubleClick={() =>
                            !disabled && !busy && onPick(g.server.id, c.id)
                          }
                          onKeyDown={(e) => {
                            if (e.key === "Enter" && !disabled && !busy) {
                              e.preventDefault();
                              onPick(g.server.id, c.id);
                            }
                          }}
                        >
                          <Volume2
                            size={16}
                            aria-hidden
                            className="room-icon"
                          />
                          <span className="room-name">{c.name}</span>
                          <span className="room-meta tabular">
                            {current ? (
                              <span className="tag">Current</span>
                            ) : blocker ? (
                              <span className="tag">
                                {blocker.phase === "leaving" ||
                                blocker.phase === "blocked"
                                  ? "Cleaning up"
                                  : "Reserved"}
                              </span>
                            ) : retiring ? (
                              <span className="tag">Cleaning up</span>
                            ) : null}
                            {sel?.c === c.id && (
                              <Check size={16} className="pick-check" />
                            )}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                ))}
                {blocker && (
                  <p className="room-note">
                    {blocker.phase === "leaving" || blocker.phase === "blocked"
                      ? "Finishing cleanup for"
                      : "Another card reserves"}{" "}
                    {g.server.channels.find((c) => c.id === blocker.channelId)
                      ?.name ?? "a saved voice room"}
                    .
                    {blocker.phase === "leaving" || blocker.phase === "blocked"
                      ? " Choose another server."
                      : " One room per server."}
                  </p>
                )}
              </div>
            );
          })}
          {groups.length === 0 && (
            <div className="pick-empty">
              {discovery.phase === "loading"
                ? "Loading voice rooms…"
                : q
                  ? `No room matches “${q}”.`
                  : "No voice rooms found. Invite your bot to a server, then refresh."}
            </div>
          )}
        </div>
      </div>
    </Sheet>
  );
}

/** The approved full-window picker gains a second native destination type. */
export function RoomPicker({
  dests,
  reservations,
  sources,
  servers,
  devices,
  discovery,
  playbackDiscovery,
  error,
  busy,
  botReady,
  onBot,
  onRefresh,
  onRefreshPlayback,
  replacing,
  onPick,
  onPlayback,
  onClose,
}: {
  dests: DestCard[];
  reservations: ServerReservation[];
  sources: SourceCard[];
  servers: Server[];
  devices: PlaybackDevice[];
  discovery: Discovery;
  playbackDiscovery: Discovery;
  error: string | null;
  busy: boolean;
  botReady: boolean;
  onBot: () => void;
  onRefresh: () => void;
  onRefreshPlayback: () => void;
  replacing?: DestCard | undefined;
  onPick: (serverId: string, channelId: string) => void;
  onPlayback: (deviceId: string) => void;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<"discord" | "playback">(
    replacing?.kind ?? (botReady ? "discord" : "playback"),
  );
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const visibleDevices = devices.filter((d) =>
    `${d.name} ${d.detail ?? ""}`
      .toLowerCase()
      .includes(q.trim().toLowerCase()),
  );
  const keyboardTab = useRef(false);
  useEffect(() => {
    if (!keyboardTab.current) return;
    document
      .querySelector<HTMLButtonElement>(
        `.destination-tabs .tab[value="${tab}"]`,
      )
      ?.focus();
    keyboardTab.current = false;
  }, [tab]);
  const inUse = new Set(
    dests.filter((d) => d.kind === "playback").map((d) => d.deviceId),
  );
  const full = dests.length >= 4 && !replacing;
  const limit =
    (full || dests.filter((d) => d.kind === "playback").length >= 3) &&
    !replacing;
  const chosen = !limit
    ? devices.find((d) => d.id === selected && d.available && !inUse.has(d.id))
    : undefined;
  const tabs = (
    <div
      className="tabs destination-tabs"
      role="tablist"
      aria-label="Destination kind"
    >
      {(["discord", "playback"] as const).map((kind) => (
        <button
          key={kind}
          type="button"
          role="tab"
          value={kind}
          className="tab"
          aria-selected={tab === kind}
          tabIndex={tab === kind ? 0 : -1}
          onKeyDown={(e) => {
            if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key))
              return;
            e.preventDefault();
            keyboardTab.current = true;
            const items = Array.from(
              e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>(
                "button:not(:disabled)",
              ) ?? [],
            );
            const current = items.indexOf(e.currentTarget);
            const next =
              e.key === "Home"
                ? 0
                : e.key === "End"
                  ? items.length - 1
                  : (current +
                      (e.key === "ArrowRight" ? 1 : -1) +
                      items.length) %
                    items.length;
            items[next]?.focus();
            items[next]?.click();
          }}
          disabled={!!replacing && replacing.kind !== kind}
          onClick={() => {
            setTab(kind);
            setSelected(null);
            setQ("");
          }}
        >
          {kind === "discord" ? "Discord" : "Playback"}
        </button>
      ))}
    </div>
  );
  if (tab === "discord" && botReady)
    return (
      <DiscordRooms
        reservations={reservations}
        full={full}
        servers={servers}
        discovery={discovery}
        error={error}
        busy={busy}
        replacing={replacing}
        onPick={onPick}
        onRefresh={onRefresh}
        onClose={onClose}
        tabs={tabs}
      />
    );
  return (
    <Sheet
      title={
        replacing
          ? `Change ${replacing.kind === "playback" ? "playback device" : "room"}`
          : "Add destination"
      }
      onClose={onClose}
      footer={
        tab === "playback" ? (
          <>
            <Refresh
              label="Refresh"
              onRefresh={onRefreshPlayback}
              busy={playbackDiscovery.phase === "loading"}
            />
            <button
              type="button"
              className="btn btn-primary"
              disabled={!chosen || busy || limit}
              onClick={() => chosen && !limit && !busy && onPlayback(chosen.id)}
            >
              {chosen
                ? `${replacing ? "Use" : "Add"} ${chosen.name}`
                : "Choose a playback device"}
            </button>
          </>
        ) : !botReady ? (
          <button type="button" className="btn btn-primary" onClick={onBot}>
            Set up bot
          </button>
        ) : undefined
      }
    >
      {tabs}
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      {tab === "discord" ? (
        <div className="pick-empty">
          Set up a Discord bot to choose a voice room. Playback works without a
          bot.
        </div>
      ) : (
        <>
          <SearchField
            value={q}
            onChange={setQ}
            placeholder="Search playback devices"
          />
          {playbackDiscovery.phase === "error" && (
            <p className="field-error" role="alert">
              {playbackDiscovery.message}
            </p>
          )}
          {limit && (
            <p className="room-note">
              {full
                ? "Four destinations are supported. Remove one to add another."
                : "Three playback outputs are supported."}
            </p>
          )}
          <div
            className="pick-list"
            role="listbox"
            aria-label="Playback devices"
          >
            {visibleDevices.map((device) => {
              const current = device.id === replacing?.deviceId;
              const used = inUse.has(device.id);
              const why = !device.available
                ? (device.detail ?? "This playback device is unavailable.")
                : used && !current
                  ? "Already an output"
                  : limit
                    ? full
                      ? "Four destinations are supported."
                      : "Three playback outputs are supported."
                    : undefined;
              const disabled = current || !!why;
              return (
                <button
                  key={device.id}
                  type="button"
                  role="option"
                  className="pick-row"
                  aria-selected={selected === device.id}
                  aria-disabled={disabled}
                  data-disabled={disabled || undefined}
                  onClick={() => !disabled && setSelected(device.id)}
                  onDoubleClick={() => !disabled && onPlayback(device.id)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !disabled) {
                      e.preventDefault();
                      onPlayback(device.id);
                    }
                  }}
                >
                  <SourceGlyph glyph="playback" tint="#6b7f8e" size={22} />
                  <span className="pick-text">
                    <span className="pick-name">{device.name}</span>
                    <span className="pick-detail">
                      {why ?? device.detail ?? "Windows playback endpoint"}
                    </span>
                  </span>
                  <span className="pick-meta">
                    {current ? (
                      <span className="tag">Current</span>
                    ) : used ? (
                      <span className="tag">In use</span>
                    ) : null}
                    {selected === device.id && (
                      <Check size={16} className="pick-check" />
                    )}
                  </span>
                </button>
              );
            })}
            {visibleDevices.length === 0 && (
              <div className="pick-empty">
                {playbackDiscovery.phase === "loading"
                  ? "Looking for playback devices…"
                  : q
                    ? `No playback device matches “${q}”.`
                    : "No playback devices found. Connect a device and refresh."}
              </div>
            )}
          </div>
          <p className="room-note">
            Added outputs stay stopped until you press Play.
          </p>
          {sources.some((s) => s.identity.kind === "recording") && (
            <p className="room-note">
              Don't send a captured cable back into its paired playback
              endpoint.
            </p>
          )}
          {sources.some((s) => s.identity.kind === "application") && (
            <p className="room-note">
              Original app playback still follows Windows routing. Playing the
              mix on the same audible device can double the sound.
            </p>
          )}
          {replacing?.state === "live" && chosen && (
            <p className="foot-note">
              Changing devices briefly stops and restarts this output.
            </p>
          )}
        </>
      )}
    </Sheet>
  );
}

export function BotSetup({
  bot,
  configure,
  error,
  busy,
  botName,
  onDone,
  onClose,
  thenRoom,
}: {
  bot: BotState;
  configure: (token: string) => Promise<boolean>;
  error: string | null;
  busy: boolean;
  botName: string;
  onDone: () => void;
  onClose: () => void;
  thenRoom: boolean;
}) {
  const [token, setToken] = useState("");
  const [show, setShow] = useState(false);
  const [saved, setSaved] = useState(false);
  const [replacing] = useState(bot === "ready");
  const phase = busy
    ? "checking"
    : saved && bot === "ready"
      ? "ok"
      : error
        ? "bad"
        : "edit";
  const test = async () => {
    const secret = token;
    setToken("");
    const ok = await configure(secret);
    setSaved(ok);
  };
  return (
    <Sheet
      title={replacing ? "Replace bot token" : "Set up Discord bot"}
      initialFocus={phase === "ok" ? ".sheet-foot .btn-primary" : "#bot-token"}
      onClose={onClose}
      footer={
        phase === "ok" ? (
          <button type="button" className="btn btn-primary" onClick={onDone}>
            {thenRoom ? "Choose a room" : "Done"}
          </button>
        ) : (
          <button
            type="submit"
            form="bot-token-form"
            className="btn btn-primary"
            disabled={phase === "checking" || token.trim() === ""}
          >
            {phase === "checking" ? (
              <LoaderCircle size={15} className="spin" />
            ) : null}
            {phase === "checking" ? "Checking with Discord" : "Test & save"}
          </button>
        )
      }
    >
      {phase === "ok" ? (
        <div className="bot-ok" role="status">
          <BotAvatar />
          <span>
            <strong>{botName}</strong> is ready. The token is stored encrypted
            for your Windows account. Choose a Discord destination, then press
            Connect when you want to stream.
          </span>
        </div>
      ) : (
        <>
          <p className="setup-intro">
            Use your own Discord bot to send the mix to a voice room.
          </p>
          <form
            id="bot-token-form"
            className="token-form"
            onSubmit={(event) => {
              event.preventDefault();
              if (!busy && token.trim()) void test();
            }}
          >
            <label className="field">
              <span className="field-label">Bot token</span>
              <span className="field-input">
                <input
                  id="bot-token"
                  type={show ? "text" : "password"}
                  value={token}
                  autoComplete="off"
                  spellCheck={false}
                  aria-describedby="token-care"
                  onChange={(e) => {
                    setToken(e.currentTarget.value);
                    setSaved(false);
                  }}
                  placeholder="Paste the token"
                />
                <button
                  type="button"
                  className="icon-btn"
                  aria-label={show ? "Hide token" : "Show token"}
                  onClick={() => setShow(!show)}
                >
                  {show ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </span>
              {phase === "bad" && (
                <span className="field-error" role="alert">
                  {error ?? "Check the token and try again."}
                </span>
              )}
            </label>
            <p className="conn-note" id="token-care">
              Stored encrypted for your Windows account. Treat this token like a
              password and never share it.
            </p>
          </form>
          <details className="setup-guide">
            <summary>Create and invite a bot</summary>
            <ol className="steps">
              <li>
                Open the{" "}
                <HelpLink to="portal">Discord Developer Portal</HelpLink>.
                Create an application and give your bot a name.
              </li>
              <li>
                On its <strong>Bot</strong> page, use{" "}
                <strong>Reset Token</strong> and paste the token above. Reset it
                again if it is ever shared.
              </li>
              <li>
                Under <strong>OAuth2 → URL Generator</strong>, choose the{" "}
                <strong>bot</strong> scope. Select{" "}
                <strong>View Channels</strong>, <strong>Connect</strong> and{" "}
                <strong>Speak</strong>. Open the generated URL to add the bot to
                a server where you have <strong>Manage Server</strong>{" "}
                permission.
              </li>
            </ol>
            <p className="conn-note">
              Administrator and privileged gateway intents are not needed.
              Channel permissions can override server permissions; allow View
              Channel, Connect and Speak in the voice room you choose.
            </p>
          </details>
          <div className="setup-help">
            <HelpLink to="guide">Full setup guide</HelpLink>
          </div>
        </>
      )}
    </Sheet>
  );
}

export function BotAvatar({
  size = 40,
  avatarUrl,
}: {
  size?: number | undefined;
  avatarUrl?: string | undefined;
}) {
  // Use a neutral bot glyph when the profile has no avatar.
  return (
    <span
      className="bot-avatar"
      style={{ width: size, height: size }}
      aria-hidden
    >
      {avatarUrl ? (
        <img src={avatarUrl} alt="" />
      ) : (
        <Bot size={Math.round(size * 0.55)} />
      )}
    </span>
  );
}

export function Settings({
  bot,
  settings,
  error,
  onSettings,
  botName,
  onBot,
  meterMode,
  onMeterMode,
  onClose,
}: {
  bot: BotState;
  settings: AppSettings;
  error: string | null;
  onSettings: (s: AppSettings) => void;
  botName: string;
  onBot: () => void;
  meterMode: MeterMode;
  onMeterMode: (m: MeterMode) => void;
  onClose: () => void;
}) {
  return (
    <Sheet title="Settings" onClose={onClose}>
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      <section className="set-group">
        <h3 className="set-title">Meters</h3>
        <div className="set-row">
          <span className="set-text">
            <span className="set-name">Meter style</span>
            <span className="set-sub">
              History draws the last few seconds of level inside every meter.
            </span>
          </span>
          <span className="tabs seg" role="radiogroup" aria-label="Meter style">
            {(["simple", "history"] as const).map((m) => (
              <label key={m} className="tab">
                <input
                  className="segment-radio"
                  type="radio"
                  name="meter-style"
                  value={m}
                  checked={meterMode === m}
                  onChange={() => onMeterMode(m)}
                />
                {m === "simple" ? "Simple" : "History"}
              </label>
            ))}
          </span>
        </div>
      </section>
      <section className="set-group">
        <h3 className="set-title">Startup and tray</h3>
        <label className="switch-row">
          <span className="set-text">
            <span className="set-name">Restore my setup</span>
            <span className="set-sub">
              Reopen sources and rooms from last time. Nothing joins until you
              press Connect.
            </span>
          </span>
          <input
            type="checkbox"
            role="switch"
            className="switch"
            checked={settings.restoreSetup}
            aria-checked={settings.restoreSetup}
            onChange={(e) =>
              onSettings({ ...settings, restoreSetup: e.currentTarget.checked })
            }
          />
        </label>
        <label className="switch-row">
          <span className="set-text">
            <span className="set-name">Keep running in the tray</span>
            <span className="set-sub">
              When enabled, Close keeps capture and streaming running. Quit from
              the tray icon.
            </span>
          </span>
          <input
            type="checkbox"
            role="switch"
            className="switch"
            checked={settings.keepInTray}
            aria-checked={settings.keepInTray}
            onChange={(e) =>
              onSettings({ ...settings, keepInTray: e.currentTarget.checked })
            }
          />
        </label>
      </section>
      <section className="set-group">
        <h3 className="set-title">Discord</h3>
        <button type="button" className="set-row set-link" onClick={onBot}>
          {bot === "ready" ? (
            <BotAvatar />
          ) : (
            <span className="bot-avatar off">?</span>
          )}
          <span className="set-text">
            <span className="set-name">
              {bot === "ready" ? botName : "No bot yet"}
            </span>
            <span className="set-sub">
              {bot === "ready"
                ? "Bot details and token"
                : "Add a bot token to join rooms"}
            </span>
          </span>
          <ChevronRight size={18} aria-hidden />
        </button>
      </section>
      <p className="about tabular">Echocord {import.meta.env.APP_VERSION}</p>
    </Sheet>
  );
}

export function BotInfo({
  servers,
  bot,
  gateway,
  reservations,
  live,
  busy,
  onRecheck,
  onReplaceToken,
  onClose,
}: {
  servers: Server[];
  bot: BotProfile;
  gateway: GatewaySnapshot;
  reservations: ServerReservation[];
  live: boolean;
  busy: boolean;
  onRecheck: () => void;
  onReplaceToken: () => void;
  onClose: () => void;
}) {
  return (
    <Sheet
      title="Discord bot"
      onClose={onClose}
      footer={
        <>
          <Refresh label="Check again" onRefresh={onRecheck} busy={busy} />
          <button
            type="button"
            className="btn"
            onClick={onReplaceToken}
            disabled={live}
            title={live ? "Disconnect first" : undefined}
          >
            <KeyRound size={15} />
            Replace token
          </button>
        </>
      }
    >
      <div className="bot-head">
        <BotAvatar size={56} avatarUrl={bot.avatarUrl} />
        <span className="set-text">
          <span className="bot-name">{bot.name ?? "Discord bot"}</span>
          {bot.id && <span className="set-sub tabular">ID {bot.id}</span>}
          <span className="bot-status">
            <span className="bot-dot" aria-hidden />
            {bot.status === "ready"
              ? "Token verified"
              : bot.status === "checking"
                ? "Checking with Discord"
                : (bot.message ?? "Not configured")}
          </span>
        </span>
      </div>
      <section className="set-group" aria-label="Shared Discord connection">
        <h3 className="set-title">Discord connection</h3>
        <div className="set-row">
          <span className="bot-status" data-gateway={gateway.phase}>
            <span className="bot-dot" aria-hidden />
            {GATEWAY_STATUS[gateway.phase]}
          </span>
        </div>
        {gateway.message && (
          <p
            className="conn-note"
            role={
              gateway.phase === "error" ||
              gateway.phase === "cleanup-incomplete"
                ? "alert"
                : "status"
            }
          >
            {gateway.message}
          </p>
        )}
        <p className="conn-note">
          This connection serves all Discord rooms. Sources and playback stay
          independent.
        </p>
      </section>
      <section className="set-group">
        <h3 className="set-title">Servers</h3>
        <ul className="perm-list">
          {servers.map((s) => {
            const reservation =
              reservations.find(
                (r) =>
                  r.serverId === s.id &&
                  (r.phase === "leaving" || r.phase === "blocked"),
              ) ?? reservations.find((r) => r.serverId === s.id);
            const reservedRoom = reservation
              ? (s.channels.find((c) => c.id === reservation.channelId)?.name ??
                "a saved voice room")
              : null;
            const reservationLabel =
              reservation &&
              {
                selected: "Reserved for",
                joining: "Joining",
                connected: "In",
                leaving: "Leaving",
                blocked: "Cleanup incomplete for",
              }[reservation.phase];
            return (
              <li key={s.id} className="perm-row">
                <ServerTile
                  initials={s.initials}
                  tint={s.tint}
                  iconUrl={s.iconUrl}
                />
                <span className="set-text">
                  <span className="set-name">{s.name}</span>
                  <span className="server-facts">
                    <span className="server-fact">
                      {s.channels.length} voice
                      {s.channels.length === 1 ? " room" : " rooms"} found
                    </span>
                    {reservation && (
                      <span
                        className="server-fact"
                        data-reservation={reservation.phase}
                      >
                        {
                          {
                            selected: "Room selected",
                            joining: "Joining",
                            connected: "In voice room",
                            leaving: "Leaving",
                            blocked: "Cleanup incomplete",
                          }[reservation.phase]
                        }
                      </span>
                    )}
                  </span>
                  {reservation && (
                    <span className="set-sub">
                      {`${reservationLabel} ${reservedRoom}${reservation.phase === "connected" ? " now" : ""}`}
                    </span>
                  )}
                </span>
              </li>
            );
          })}
        </ul>
        {servers.length === 0 && (
          <p className="conn-note">
            No servers loaded. Invite the bot, then check again.
          </p>
        )}
        <p className="conn-note">
          Room permissions are checked when you connect. The bot needs View
          Channel, Connect and Speak in that room. One voice room per server.
        </p>
        <div className="setup-help">
          <HelpLink to="guide">Invite a bot or fix room permissions</HelpLink>
        </div>
      </section>
      <p className="conn-note">
        The token is stored encrypted for your Windows account and is never
        shown again.
      </p>
    </Sheet>
  );
}

export function ConnectionSheet({
  dest,
  onClose,
}: {
  dest: DestCard;
  onClose: () => void;
}) {
  const server = dest.server;
  const channel = dest.channel;
  const store = useTelemetryStore();
  const underruns = useRef<HTMLElement>(null);
  useEffect(
    () =>
      paint(
        store,
        () => {
          if (underruns.current)
            underruns.current.textContent = `${store.net(dest.uid).underruns ?? "Unavailable"}`;
        },
        2,
      ),
    [store, dest.uid],
  );
  const local = dest.kind === "playback";
  const sk = local && dest.state === "live" ? "playing" : statusKey(dest.state);
  const statusLabel = local
    ? dest.state === "live"
      ? "Playing"
      : "Stopped"
    : STATUS[statusKey(dest.state)];
  return (
    <Sheet
      title={dest.kind === "playback" ? "Playback output" : "Connection"}
      onClose={onClose}
      label={`${dest.kind === "playback" ? "Playback output" : "Connection"} details for ${channel?.name}`}
    >
      <div className="conn-head">
        <span className="badge" data-status={sk}>
          <span className="badge-dot" aria-hidden />
          {statusLabel}
        </span>
        <span className="conn-room">
          {channel?.name} <span className="conn-server">{server?.name}</span>
        </span>
        <span className="conn-time tabular">
          <OnAirTime since={dest.liveAt} />
        </span>
      </div>
      <div className="conn-chart">
        <NetChart uid={dest.uid} seconds={120} minHeight={40} local={local} />
      </div>
      <StatTiles uid={dest.uid} local={local} />
      <dl className="facts-list tabular">
        <dt>Underrun frames</dt>
        <dd ref={underruns}>Unavailable</dd>
        <dt>Audio</dt>
        <dd>{local ? "Native mix · 48 kHz stereo" : "Opus · 48 kHz stereo"}</dd>
      </dl>
      <p className="conn-note">
        {local
          ? "The meter observes samples handed to this selected device after output volume and mute; it does not prove physical audibility. Buffer is audio queued inside Echocord, not device or listener delay."
          : "Gateway is Discord's control connection; it is unavailable until measured. Buffer is audio waiting inside Echocord. Neither is listener delay. The meter observes native output before Discord transport; it does not confirm what listeners hear."}
      </p>
    </Sheet>
  );
}
