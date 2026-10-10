import { Settings as Gear, Minus, X } from "lucide-react";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { ControlAdapter } from "./adapter";
import { DestCard, Empty, SlotHead, SourceCard } from "./cards";
import { GATEWAY_STATUS } from "./copy";
import type { Connection, ControlSnapshot } from "./generated";
import { Mark } from "./icons";
import {
  catalogSource,
  type DestCard as Destination,
  type Sheet,
  type SourceCard as Source,
  serverView,
} from "./model";
import {
  BotAvatar,
  BotInfo,
  BotSetup,
  ConnectionSheet,
  RoomPicker,
  Settings,
  SourcePicker,
} from "./sheets";
import { TelemetryProvider } from "./telemetry";

function phase(c: Connection): Destination["state"] {
  switch (c.phase) {
    case "disconnected":
      return "idle";
    case "connected":
      return "live";
    case "error":
      return "fault";
    default:
      return c.phase;
  }
}
function destination(
  s: ControlSnapshot,
  d: ControlSnapshot["destinations"][number],
  confirmRemove: boolean,
  liveAt?: number,
): Destination {
  const device =
    d.kind === "playback"
      ? (s.playbackDevices.find((x) => x.id === d.deviceId) ?? null)
      : null;
  const server =
    d.kind === "playback"
      ? {
          id: "",
          name: device?.detail ?? "Windows playback",
          initials: "",
          tint: "#6b7f8e",
          iconUrl: null,
          channels: [],
        }
      : serverView(
          s.servers.find((x) => x.id === d.serverId) ?? {
            id: d.serverId,
            name: "Saved server",
            iconUrl: null,
            channels: [],
          },
        );
  const channel =
    d.kind === "playback"
      ? {
          id: d.deviceId ?? "",
          name: device?.name ?? d.displayName ?? "Saved playback device",
          category: "Playback",
        }
      : (server.channels.find((x) => x.id === d.channelId) ?? {
          id: d.channelId,
          name: "Saved voice room",
          category: "Voice rooms",
        });
  const c = d.connection;
  const stage =
    c.phase === "connecting"
      ? c.stage === "join"
        ? "joining"
        : c.stage === "capture"
          ? "starting"
          : c.stage === "preflight"
            ? "checking"
            : c.stage
      : undefined;
  return {
    uid: d.destinationId,
    kind: d.kind,
    deviceId: d.deviceId,
    device,
    sessionId: d.sessionId,
    serverId: d.serverId,
    channelId: d.channelId,
    server,
    channel,
    state: phase(c),
    connection: c,
    stage,
    attempt: c.phase === "recovering" ? c.attempt : undefined,
    volumeDb: d.volumeDb,
    muted: d.muted,
    confirmRemove,
    op: d.sessionId,
    liveAt,
    fault:
      c.phase === "error" || c.phase === "cleanup-incomplete"
        ? {
            title: c.message,
            actionLabel:
              c.phase === "cleanup-incomplete"
                ? d.kind === "playback"
                  ? "Retry stop"
                  : "Retry disconnect"
                : d.kind === "playback"
                  ? "Change device"
                  : "Change room",
          }
        : undefined,
  };
}
export function App({ adapter }: { adapter: ControlAdapter }) {
  const view = useSyncExternalStore(
    adapter.subscribe,
    adapter.getSnapshot,
    adapter.getSnapshot,
  );
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const win = useRef<HTMLDivElement>(null);
  const [wide, setWide] = useState(false);
  const liveTimes = useRef(
    new Map<string, { session: number; since: number }>(),
  );
  useEffect(() => {
    void adapter.start();
    return adapter.dispose;
  }, [adapter]);
  useEffect(() => {
    const el = win.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) =>
      setWide((e?.contentRect.width ?? 0) >= 700),
    );
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const snapshot = view.snapshot;
  const failedCommand = view.commandErrorCommand;
  const operationError = (uid: string) =>
    failedCommand &&
    (("stripId" in failedCommand && failedCommand.stripId === uid) ||
      ("destinationId" in failedCommand && failedCommand.destinationId === uid))
      ? view.commandError?.message
      : undefined;
  const targetedError =
    !!failedCommand &&
    (("stripId" in failedCommand &&
      snapshot?.inputs.some((s) => s.stripId === failedCommand.stripId)) ||
      ("destinationId" in failedCommand &&
        snapshot?.destinations.some(
          (d) => d.destinationId === failedCommand.destinationId,
        )));
  const catalog = useMemo(
    () => snapshot?.sources.map(catalogSource) ?? [],
    [snapshot?.sources],
  );
  const servers = useMemo(
    () => snapshot?.servers.map(serverView) ?? [],
    [snapshot?.servers],
  );
  const sources: Source[] =
    snapshot?.inputs.map((c) => ({
      uid: c.stripId,
      sourceId: c.sourceId,
      identity:
        catalog.find((s) => s.id === c.sourceId) ??
        catalogSource({
          id: c.sourceId,
          name: c.displayName,
          detail: c.detail,
          kind: c.kind,
          available: false,
        }),
      colour: c.colourSlot,
      state: c.phase,
      muted: c.muted,
      gainDb: c.gainDb,
      restored: c.restored,
      operationError: operationError(c.stripId),
      fault: c.fault
        ? { title: c.fault.message, actionLabel: "Pick again" }
        : undefined,
    })) ?? [];
  const destinationIds = new Set(
    snapshot?.destinations.map((d) => d.destinationId) ?? [],
  );
  for (const id of liveTimes.current.keys())
    if (!destinationIds.has(id)) liveTimes.current.delete(id);
  const dests: Destination[] =
    snapshot?.destinations.map((d) => {
      if (d.connection.phase === "connected") {
        if (liveTimes.current.get(d.destinationId)?.session !== d.sessionId)
          liveTimes.current.set(d.destinationId, {
            session: d.sessionId,
            since: Date.now(),
          });
      } else if (d.connection.phase !== "recovering")
        liveTimes.current.delete(d.destinationId);
      return {
        ...destination(
          snapshot,
          d,
          confirm === d.destinationId,
          liveTimes.current.get(d.destinationId)?.since,
        ),
        operationError: operationError(d.destinationId),
      };
    }) ?? [];
  const open = (next: Sheet) => {
    if (view.phase !== "ready") return;
    if (!sheet)
      opener.current =
        document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null;
    setSheet(next);
  };
  const close = () => {
    setSheet(null);
    requestAnimationFrame(() => {
      const el = opener.current;
      if (el?.isConnected) el.focus({ preventScroll: true });
      else win.current?.querySelector<HTMLElement>(".btn-add")?.focus();
    });
  };
  const send = adapter.dispatch;
  const capturing = sources.filter((s) => s.state === "capturing");
  const on = capturing.filter((s) => !s.muted).length;
  const muted = capturing.filter((s) => s.muted).length;
  const stopped = sources.length - capturing.length;
  const sourceSummary = sources.length
    ? [on && `${on} on`, muted && `${muted} muted`, stopped && `${stopped} off`]
        .filter(Boolean)
        .join(" · ")
    : undefined;
  const bot = snapshot?.bot.status ?? "unconfigured";
  const botName = snapshot?.bot.name ?? "Discord bot";
  const mode = snapshot?.settings.meterStyle ?? "simple";
  const addDestination = () => open({ kind: "room" });
  const replacingSource =
    sheet?.kind === "source"
      ? sources.find((s) => s.uid === sheet.replaceUid)
      : undefined;
  const replacingDest =
    sheet?.kind === "room"
      ? dests.find((d) => d.uid === sheet.replaceUid)
      : undefined;
  const netDest =
    sheet?.kind === "net" ? dests.find((d) => d.uid === sheet.uid) : undefined;
  const live =
    dests.some(
      (d) => d.kind === "discord" && !["idle", "fault"].includes(d.state),
    ) ||
    (!!snapshot &&
      ([
        "connecting",
        "connected",
        "recovering",
        "disconnecting",
        "cleanup-incomplete",
      ].includes(snapshot.gateway.phase) ||
        snapshot.serverReservations.some((r) => r.phase !== "selected")));
  const playing = dests.filter(
    (d) => d.kind === "playback" && d.state === "live",
  ).length;
  const onAir = dests.filter(
    (d) => d.kind === "discord" && d.state === "live",
  ).length;
  const destinationSummary =
    dests.length > 1
      ? [onAir && `${onAir} on air`, playing && `${playing} playing`]
          .filter(Boolean)
          .join(" · ") || "All stopped"
      : undefined;
  return (
    <TelemetryProvider value={adapter.telemetry}>
      <div ref={win} className="window" data-layout="stack" data-native>
        <header
          className="titlebar"
          role="toolbar"
          aria-label="Window controls"
          onMouseDown={(e) => {
            if (
              e.button === 0 &&
              e.detail === 1 &&
              !(e.target as HTMLElement).closest("button,input,a")
            )
              void adapter.startDragging();
          }}
        >
          <span className="brand">
            <Mark />
            <span className="brand-name">Echocord</span>
          </span>
          <span className="flex" />
          <button
            type="button"
            className="bot-chip"
            data-bot={bot}
            data-gateway={snapshot?.gateway.phase}
            aria-label={
              bot === "ready"
                ? `${botName}, Discord connection ${GATEWAY_STATUS[snapshot?.gateway.phase ?? "disconnected"].toLowerCase()}`
                : undefined
            }
            onClick={() =>
              open(bot === "ready" ? { kind: "bot-info" } : { kind: "bot" })
            }
            title={
              bot === "ready"
                ? `Discord bot details · ${GATEWAY_STATUS[snapshot?.gateway.phase ?? "disconnected"]}`
                : "Set up the Discord bot"
            }
          >
            {bot === "ready" ? (
              <BotAvatar size={20} avatarUrl={snapshot?.bot.avatarUrl} />
            ) : (
              <span className="bot-dot" aria-hidden />
            )}
            <span className="bot-chip-name">
              {bot === "ready"
                ? botName
                : bot === "checking"
                  ? "Checking bot"
                  : "Set up bot"}
            </span>
            {bot === "ready" && (
              <span
                className="bot-dot"
                title={`Token verified · ${GATEWAY_STATUS[snapshot?.gateway.phase ?? "disconnected"]}`}
                aria-hidden
              />
            )}
          </button>
          <button
            type="button"
            className="win-btn"
            aria-label="Settings"
            title="Settings"
            onClick={() => open({ kind: "settings" })}
          >
            <Gear size={16} />
          </button>
          <button
            type="button"
            className="win-btn"
            aria-label="Minimize"
            onClick={() => void adapter.minimize()}
          >
            <Minus size={16} />
          </button>
          <button
            type="button"
            className="win-btn win-close"
            aria-label={
              snapshot?.settings.keepInTray === false
                ? "Close Echocord"
                : "Close to tray"
            }
            title={
              snapshot?.settings.keepInTray === false
                ? "Close Echocord"
                : "Close to tray"
            }
            onClick={() => void adapter.close()}
          >
            <X size={16} />
          </button>
        </header>
        <main className="surface" inert={sheet ? true : undefined}>
          {view.phase === "loading" && (
            <div className="app-status" role="status">
              Loading your setup…
            </div>
          )}
          {view.phase === "error" && (
            <div className="app-error" role="alert">
              <span>{view.error?.message}</span>
              <button
                type="button"
                className="btn"
                onClick={() => void adapter.start()}
              >
                Retry
              </button>
            </div>
          )}
          {view.commandError && !targetedError && (
            <div className="app-error" role="alert">
              <span>{view.commandError.message}</span>
              <button
                type="button"
                className="icon-btn"
                aria-label="Dismiss error"
                onClick={adapter.dismissError}
              >
                <X size={16} />
              </button>
            </div>
          )}
          <section
            className="slot src-slot"
            aria-label="Sources"
            data-many={sources.length > 1 || undefined}
          >
            <SlotHead
              title="Sources"
              summary={sourceSummary}
              chips={sources.map((c) => ({
                uid: c.uid,
                label: c.identity.name,
                colour: c.colour,
                state:
                  c.state === "capturing"
                    ? c.muted
                      ? "muted"
                      : "on"
                    : c.state,
              }))}
              addLabel="Add source"
              onAdd={() => open({ kind: "source" })}
              addDisabled={view.phase !== "ready" || sources.length >= 8}
              addHint={
                sources.length >= 8 ? "Eight sources for now" : undefined
              }
            />
            {sources.length === 0 ? (
              <Empty
                text="No sources yet"
                sub="An app like Firefox or Spotify, or an input like VB-Cable."
                action="Add source"
                onAction={() => open({ kind: "source" })}
              />
            ) : (
              <div className="list src-list">
                {sources.map((c, i) => (
                  <SourceCard
                    key={c.uid}
                    card={c}
                    index={i}
                    mode={mode}
                    expand={wide && sources.length <= 2}
                    h={{
                      dismiss: adapter.dismissError,
                      change: () => open({ kind: "source", replaceUid: c.uid }),
                      remove: () =>
                        void send({ type: "remove-source", stripId: c.uid }),
                      capture: () =>
                        void send({ type: "start-capture", stripId: c.uid }),
                      stop: () =>
                        void send({ type: "stop-capture", stripId: c.uid }),
                      mute: (m) =>
                        void send({
                          type: "set-source-muted",
                          stripId: c.uid,
                          muted: m,
                        }),
                      commit: () => void adapter.flushFader(`src:${c.uid}`),
                      gain: (gainDb) =>
                        adapter.setFader(`src:${c.uid}`, {
                          type: "set-source-gain",
                          stripId: c.uid,
                          gainDb,
                        }),
                      fault: () => open({ kind: "source", replaceUid: c.uid }),
                    }}
                  />
                ))}
                <div className="list-fade" aria-hidden />
              </div>
            )}
          </section>
          <section
            className="slot dst-slot"
            aria-label="Destinations"
            data-many={dests.length > 1 || undefined}
          >
            <SlotHead
              title={dests.length > 1 ? "Destinations" : "Destination"}
              summary={destinationSummary}
              chips={dests.map((d) => ({
                uid: d.uid,
                label: d.channel.name,
                state:
                  d.state === "live"
                    ? d.kind === "discord"
                      ? "on"
                      : "playing"
                    : d.state === "fault"
                      ? "fault"
                      : "stopped",
              }))}
              addLabel="Add destination"
              onAdd={addDestination}
              addDisabled={view.phase !== "ready" || dests.length >= 4}
              addHint={
                dests.length >= 4 ? "Four destinations for now" : undefined
              }
            />
            {dests.length === 0 ? (
              <Empty
                text="No destinations yet"
                sub="A Discord voice room or a Windows playback device."
                action="Add destination"
                onAction={addDestination}
              />
            ) : (
              <div
                className="list dst-list"
                data-count={dests.length}
                style={
                  {
                    "--dst-natural": `${dests.length === 1 ? (mode === "history" ? 194 : 182) : dests.length * (mode === "history" ? 146 : 134) + (dests.length - 1) * 8}px`,
                  } as React.CSSProperties
                }
              >
                {dests.map((d) => (
                  <DestCard
                    key={d.uid}
                    card={d}
                    mode={mode}
                    expand={dests.length === 1}
                    compact={dests.length > 1}
                    sourcesCapturing={capturing.length}
                    botReady={bot === "ready"}
                    botName={botName}
                    h={{
                      dismiss: adapter.dismissError,
                      change: () => open({ kind: "room", replaceUid: d.uid }),
                      remove: () => {
                        setConfirm(null);
                        void send({
                          type: "remove-destination",
                          destinationId: d.uid,
                        });
                      },
                      confirmRemove: (v) => setConfirm(v ? d.uid : null),
                      connect: () =>
                        void send({
                          type: "connect-destination",
                          destinationId: d.uid,
                        }),
                      disconnect: () =>
                        void send({
                          type: "disconnect-destination",
                          destinationId: d.uid,
                        }),
                      commit: () => void adapter.flushFader(`dst:${d.uid}`),
                      volume: (volumeDb) =>
                        adapter.setFader(`dst:${d.uid}`, {
                          type: "set-destination-volume",
                          destinationId: d.uid,
                          volumeDb,
                        }),
                      mute: (m) =>
                        void send({
                          type: "set-destination-muted",
                          destinationId: d.uid,
                          muted: m,
                        }),
                      fault: () =>
                        d.state === "cleanup-incomplete"
                          ? void send({
                              type: "disconnect-destination",
                              destinationId: d.uid,
                            })
                          : open({ kind: "room", replaceUid: d.uid }),
                      details: () => open({ kind: "net", uid: d.uid }),
                    }}
                  />
                ))}
                <div className="list-fade" aria-hidden />
              </div>
            )}
          </section>
        </main>
        {sheet?.kind === "source" && snapshot && (
          <SourcePicker
            sources={sources}
            catalog={catalog}
            error={view.commandError?.message ?? null}
            busy={view.pending.length > 0}
            discovery={snapshot.sourceDiscovery}
            onRefresh={() => void send({ type: "refresh-sources" })}
            replacing={replacingSource}
            onClose={close}
            onPick={async (sourceId) => {
              if (
                await send(
                  replacingSource
                    ? {
                        type: "replace-source",
                        stripId: replacingSource.uid,
                        sourceId,
                      }
                    : { type: "add-source", sourceId },
                )
              )
                close();
            }}
          />
        )}
        {sheet?.kind === "room" && snapshot && (
          <RoomPicker
            dests={dests}
            reservations={snapshot.serverReservations}
            servers={servers}
            devices={snapshot.playbackDevices}
            sources={sources}
            playbackDiscovery={snapshot.playbackDiscovery}
            botReady={bot === "ready"}
            onBot={() => setSheet({ kind: "bot", next: "room" })}
            onRefreshPlayback={() =>
              void send({ type: "refresh-playback-devices" })
            }
            error={view.commandError?.message ?? null}
            busy={view.pending.length > 0}
            discovery={snapshot.discovery}
            onRefresh={() => void send({ type: "refresh-catalog" })}
            replacing={replacingDest}
            onClose={close}
            onPlayback={async (deviceId) => {
              if (
                await send(
                  replacingDest
                    ? {
                        type: "change-playback-destination",
                        destinationId: replacingDest.uid,
                        deviceId,
                      }
                    : { type: "add-playback-destination", deviceId },
                )
              )
                close();
            }}
            onPick={async (serverId, channelId) => {
              if (
                await send(
                  replacingDest
                    ? {
                        type: "change-destination",
                        destinationId: replacingDest.uid,
                        serverId,
                        channelId,
                      }
                    : { type: "add-destination", serverId, channelId },
                )
              )
                close();
            }}
          />
        )}
        {sheet?.kind === "bot" && (
          <BotSetup
            bot={bot}
            botName={botName}
            busy={view.configuringBot}
            error={view.configurationError?.message ?? null}
            configure={adapter.configureBot}
            thenRoom={sheet.next === "room"}
            onDone={() =>
              sheet.next === "room" ? setSheet({ kind: "room" }) : close()
            }
            onClose={close}
          />
        )}
        {sheet?.kind === "settings" && snapshot && (
          <Settings
            bot={bot}
            botName={botName}
            settings={snapshot.settings}
            error={view.commandError?.message ?? null}
            onSettings={(s) => void adapter.setSettings(s)}
            meterMode={mode}
            onMeterMode={(meterStyle) =>
              void adapter.setSettings({ ...snapshot.settings, meterStyle })
            }
            onBot={() =>
              setSheet(bot === "ready" ? { kind: "bot-info" } : { kind: "bot" })
            }
            onClose={close}
          />
        )}
        {netDest && <ConnectionSheet dest={netDest} onClose={close} />}
        {sheet?.kind === "bot-info" && snapshot && (
          <BotInfo
            servers={servers}
            bot={snapshot.bot}
            gateway={snapshot.gateway}
            reservations={snapshot.serverReservations}
            live={live}
            busy={
              snapshot.bot.status === "checking" ||
              snapshot.discovery.phase === "loading"
            }
            onRecheck={() => void send({ type: "recheck-configuration" })}
            onReplaceToken={() => setSheet({ kind: "bot" })}
            onClose={close}
          />
        )}
      </div>
    </TelemetryProvider>
  );
}
