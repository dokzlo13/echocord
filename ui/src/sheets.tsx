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
import { STATUS, statusKey } from "./copy";
import type { AppSettings, BotProfile, Discovery } from "./generated";
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
}: {
  title: string;
  onClose: () => void;
  footer?: ReactNode | undefined;
  children: ReactNode;
  label?: string | undefined;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    if (!root.contains(document.activeElement))
      root.querySelector<HTMLElement>("input,button")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close.current();
      }
      if (e.key !== "Tab") return;
      const items = Array.from(
        root.querySelectorAll<HTMLElement>(
          'button:not(:disabled),input:not(:disabled),a[href],[tabindex="0"]',
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
  }, []);
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

export function RoomPicker({
  dests,
  servers,
  error,
  busy,
  discovery,
  onRefresh,
  replacing,
  onPick,
  onClose,
}: {
  dests: DestCard[];
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
    replacing?.serverId ?? servers[0]?.id ?? "",
  );
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<{ s: string; c: string } | null>(null);
  const live = replacing && replacing.state === "live";
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
    dests.find((d) => d.serverId === serverId && d.uid !== replacing?.uid);

  const chosen = sel
    ? servers.find((s) => s.id === sel.s)?.channels.find((c) => c.id === sel.c)
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
            onClick={() => sel && onPick(sel.s, sel.c)}
          >
            {primary}
          </button>
        </>
      }
    >
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
                      const isCurrent =
                        replacing?.serverId === g.server.id &&
                        replacing.channelId === c.id;
                      const disabled = !!blocker || isCurrent;
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
                            !disabled && setSel({ s: g.server.id, c: c.id })
                          }
                          onDoubleClick={() =>
                            !disabled && onPick(g.server.id, c.id)
                          }
                          onKeyDown={(e) => {
                            if (e.key === "Enter" && !disabled) {
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
                            {isCurrent ? (
                              <span className="tag">Current</span>
                            ) : blocker ? (
                              <span className="tag">Bot busy here</span>
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
                    The bot can be in one room per server. It's already used for{" "}
                    {blocker.channel.name}.
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
      title="Discord bot"
      onClose={onClose}
      footer={
        phase === "ok" ? (
          <button type="button" className="btn btn-primary" onClick={onDone}>
            {thenRoom ? "Choose a room" : "Done"}
          </button>
        ) : (
          <button
            type="button"
            className="btn btn-primary"
            disabled={phase === "checking" || token.trim() === ""}
            onClick={test}
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
            for your Windows account.
          </span>
        </div>
      ) : (
        <>
          <ol className="steps">
            <li>Create an application in the Discord Developer Portal.</li>
            <li>Under Bot, reset the token and copy it.</li>
            <li>Invite the bot to your server with Connect and Speak.</li>
          </ol>
          <label className="field">
            <span className="field-label">Bot token</span>
            <span className="field-input">
              <input
                type={show ? "text" : "password"}
                value={token}
                autoComplete="off"
                spellCheck={false}
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
  // The native profile currently has no avatar; use a neutral bot glyph.
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
      <p className="about tabular">Echocord 0.1.0</p>
    </Sheet>
  );
}

export function BotInfo({
  dests,
  servers,
  bot,
  live,
  busy,
  onRecheck,
  onReplaceToken,
  onClose,
}: {
  dests: DestCard[];
  servers: Server[];
  bot: BotProfile;
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
      <section className="set-group">
        <h3 className="set-title">Servers</h3>
        <ul className="perm-list">
          {servers.map((s) => {
            const inRoom = dests.find(
              (d) => d.serverId === s.id && d.state === "live",
            );
            return (
              <li key={s.id} className="perm-row">
                <ServerTile
                  initials={s.initials}
                  tint={s.tint}
                  iconUrl={s.iconUrl}
                />
                <span className="set-text">
                  <span className="set-name">{s.name}</span>
                  <span className="set-sub">
                    {inRoom
                      ? `In ${inRoom.channel.name} now`
                      : `${s.channels.length} voice rooms`}
                  </span>
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
          Room permissions are checked when you connect.
        </p>
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
            underruns.current.textContent = `${store.net().underruns ?? "Unavailable"}`;
        },
        2,
      ),
    [store],
  );
  const sk = statusKey(dest.state);
  return (
    <Sheet
      title="Connection"
      onClose={onClose}
      label={`Connection details for ${channel?.name}`}
    >
      <div className="conn-head">
        <span className="badge" data-status={sk}>
          <span className="badge-dot" aria-hidden />
          {STATUS[sk]}
        </span>
        <span className="conn-room">
          {channel?.name} <span className="conn-server">{server?.name}</span>
        </span>
        <span className="conn-time tabular">
          <OnAirTime since={dest.liveAt} />
        </span>
      </div>
      <div className="conn-chart">
        <NetChart uid={dest.uid} seconds={120} minHeight={40} />
      </div>
      <StatTiles uid={dest.uid} />
      <dl className="facts-list tabular">
        <dt>Underruns</dt>
        <dd ref={underruns}>Unavailable</dd>
        <dt>Reconnects this session</dt>
        <dd>Unavailable</dd>
        <dt>Voice server</dt>
        <dd>Unavailable</dd>
        <dt>Audio</dt>
        <dd>Opus · 48 kHz stereo</dd>
        <dt>Bot</dt>
        <dd>Discord bot</dd>
      </dl>
      <p className="conn-note">
        Gateway is Discord's control connection; it is unavailable until
        measured. Buffer is audio waiting inside Echocord. Neither is listener
        delay. The meter observes the native output before Discord transport; it
        does not confirm what players hear.
      </p>
    </Sheet>
  );
}
