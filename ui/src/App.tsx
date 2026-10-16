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
import {
  botStatusLabel,
  destinationChipState,
  destinationStatusLabel,
  destinationSummaryText,
  GATEWAY_STATUS,
  sourceSummaryText,
} from "./copy";
import type { Connection, ControlSnapshot } from "./generated";
import { Mark } from "./icons";
import { Licenses } from "./Licenses";
import {
  catalogSource,
  type DestCard as Destination,
  type Sheet,
  type SourceCard as Source,
  serverView,
} from "./model";
import { SaveStatus } from "./SaveStatus";
import { SyncDetails } from "./SyncDetails";
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
      return c.code === "cleanup-incomplete" ? "cleanup-incomplete" : "fault";
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
function useRecoveryAnnouncements(
  ready: boolean,
  sources: Source[],
  destinations: Destination[],
) {
  const previous = useRef<Map<
    string,
    { state: string; problem: string | undefined; info: string | undefined }
  > | null>(null);
  const serial = useRef(0);
  const [notice, setNotice] = useState({ serial: 0, urgent: "", polite: "" });
  useEffect(() => {
    if (!ready) {
      previous.current = null;
      return;
    }
    const observations = [
      ...sources.map((card, index) => ({
        key: `src:${card.uid}`,
        state: card.state,
        label: `Source${sources.length > 1 ? ` ${index + 1}` : ""}, ${card.identity.name}`,
        problem:
          card.operationError?.message ??
          (card.state === "fault" ? card.fault?.title : undefined),
        info: undefined as string | undefined,
        success: "Capturing",
      })),
      ...destinations.map((card, index) => ({
        key: `dst:${card.uid}`,
        state: card.state,
        label: `Destination${destinations.length > 1 ? ` ${index + 1}` : ""}, ${card.channel.name}${card.kind === "discord" ? ` on ${card.server.name}` : ""}`,
        problem: card.operationError?.message ?? card.fault?.title,
        info:
          card.connection.phase === "recovering"
            ? card.connection.message
            : undefined,
        success: destinationStatusLabel(card.state, card.kind),
      })),
    ];
    const current = new Map(
      observations.map((item) => [
        item.key,
        { state: item.state, problem: item.problem, info: item.info },
      ]),
    );
    if (previous.current) {
      const urgent = new Map<string, string[]>();
      const polite: string[] = [];
      for (const item of observations) {
        const before = previous.current.get(item.key);
        if (item.problem && item.problem !== before?.problem) {
          urgent.set(item.problem, [
            ...(urgent.get(item.problem) ?? []),
            item.label,
          ]);
        } else if (
          before &&
          !item.problem &&
          (before.state !== item.state || before.info !== item.info) &&
          ["recovering", "disconnecting", "live", "capturing"].includes(
            item.state,
          )
        ) {
          polite.push(`${item.label}: ${item.info ?? item.success}.`);
        }
      }
      if (urgent.size || polite.length)
        setNotice({
          serial: ++serial.current,
          urgent: [...urgent]
            .map(([message, labels]) => `${labels.join("; ")}: ${message}`)
            .join(" "),
          polite: polite.join(" "),
        });
    }
    previous.current = current;
  }, [ready, sources, destinations]);
  return notice;
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
  const warning = snapshot?.preferencesWarning;
  const hasSaveWarning = Boolean(warning);
  const saving =
    Boolean(snapshot?.preferencesSaving) ||
    view.pending.includes(JSON.stringify({ type: "retry-preferences" }));
  const previousWarning = useRef(false);
  const previousSaving = useRef(false);
  const saveRequested = useRef(false);
  const [saveAnnouncement, setSaveAnnouncement] = useState("");
  useEffect(() => {
    if (
      saving &&
      (hasSaveWarning || previousWarning.current || saveRequested.current)
    )
      setSaveAnnouncement("Saving changes.");
    else if (hasSaveWarning)
      setSaveAnnouncement(
        previousSaving.current || saveRequested.current
          ? `Still not saved. ${warning?.message ?? "Try again."}`
          : "Changes apply now, but are not saved for next time.",
      );
    else if (
      previousWarning.current ||
      (previousSaving.current && saveRequested.current)
    ) {
      setSaveAnnouncement("Changes saved.");
      saveRequested.current = false;
      requestAnimationFrame(() => {
        if (document.activeElement !== document.body) return;
        const target =
          document.querySelector<HTMLElement>(".sheet-head button") ??
          document.querySelector<HTMLElement>(".src-slot .btn-add");
        target?.focus();
      });
    }
    previousWarning.current = hasSaveWarning;
    previousSaving.current = saving;
  }, [hasSaveWarning, saving, warning?.message]);
  const retrySave = async () => {
    saveRequested.current = true;
    setSaveAnnouncement("");
    await adapter.dispatch({ type: "retry-preferences" });
    requestAnimationFrame(() => {
      const state = adapter.getSnapshot().snapshot;
      const remaining = state?.preferencesWarning;
      setSaveAnnouncement(
        state?.preferencesSaving
          ? "Saving changes."
          : remaining
            ? `Still not saved. ${remaining.message}`
            : "Changes saved.",
      );
    });
  };
  const failedCommand = view.commandErrorCommand;
  const operationError = (target: string) => view.targetErrors[target]?.error;
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
      operationError: operationError(`src:${c.stripId}`),
      operationCommand: view.targetErrors[`src:${c.stripId}`]?.command.type,
      fault: c.fault
        ? {
            code: c.fault.code,
            title: c.fault.message,
            actionLabel: "Select again",
          }
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
        operationError: operationError(`dst:${d.destinationId}`),
        operationCommand:
          view.targetErrors[`dst:${d.destinationId}`]?.command.type,
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
  const sourceSummary = sourceSummaryText(sources);
  const bot = snapshot?.bot.status ?? "unconfigured";
  const botName = snapshot?.bot.name ?? "Discord bot";
  const mode = snapshot?.settings.meterStyle ?? "simple";
  const limiterOn = snapshot?.settings.mixLimiter ?? false;
  // A limiter mode change starts a new observation period for its readouts.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset only when the mode changes.
  useEffect(() => adapter.telemetry.resetLimiter(), [adapter, limiterOn]);
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
  const destinationSummary = destinationSummaryText(dests);
  const recoveryNotice = useRecoveryAnnouncements(
    view.phase === "ready",
    sources,
    dests,
  );
  return (
    <TelemetryProvider value={adapter.telemetry}>
      <div
        ref={win}
        className="window"
        data-layout="stack"
        data-native
        data-unsaved={Boolean(warning)}
      >
        <span className="sr-only" role="status">
          {saveAnnouncement}
        </span>
        <span
          className="sr-only"
          id="recovery-alert"
          aria-live="assertive"
          aria-atomic="true"
        >
          <span key={recoveryNotice.serial}>{recoveryNotice.urgent}</span>
        </span>
        <span
          className="sr-only"
          id="recovery-status"
          aria-live="polite"
          aria-atomic="true"
        >
          <span key={recoveryNotice.serial}>{recoveryNotice.polite}</span>
        </span>
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
                : botStatusLabel(bot, botName)
            }
          >
            {bot === "ready" ? (
              <BotAvatar size={20} avatarUrl={snapshot?.bot.avatarUrl} />
            ) : (
              <span className="bot-dot" aria-hidden />
            )}
            <span className="bot-chip-name">
              {botStatusLabel(bot, botName)}
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
          {/* While Settings is open its footer presents rejected changes instead. */}
          {view.commandError &&
            !targetedError &&
            !(
              sheet?.kind === "settings" &&
              failedCommand?.type === "set-settings"
            ) && (
              <div className="app-error" role="alert">
                <span>
                  {failedCommand?.type === "set-settings"
                    ? `Setting change failed. ${view.commandError.message}`
                    : view.commandError.message}
                </span>
                <button
                  type="button"
                  className="icon-btn"
                  aria-label="Dismiss error"
                  onClick={() => adapter.dismissError()}
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
              itemLabel="Source"
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
                    discoveryReady={snapshot?.sourceDiscovery.phase === "ready"}
                    ordinal={sources.length > 1 ? i + 1 : undefined}
                    mode={mode}
                    expand={wide && sources.length <= 2}
                    h={{
                      dismiss: () => adapter.dismissError(`src:${c.uid}`),
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
              itemLabel="Destination"
              summary={destinationSummary}
              chips={dests.map((d) => ({
                uid: d.uid,
                label:
                  d.kind === "playback"
                    ? `${d.channel.name}, playback output`
                    : `${d.channel.name} on ${d.server.name}`,
                state: destinationChipState(d),
                stateLabel:
                  d.state === "cleanup-incomplete"
                    ? "Cleanup unconfirmed"
                    : destinationStatusLabel(d.state, d.kind),
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
                    "--dst-natural": `${dests.length === 1 ? (mode === "history" ? 194 : 182) : dests.length * (mode === "history" ? 146 : 134) + (dests.length - 1) * 8 + dests.filter((d) => d.fault || d.operationError).length * 46}px`,
                  } as React.CSSProperties
                }
              >
                {dests.map((d, i) => (
                  <DestCard
                    key={d.uid}
                    card={d}
                    ordinal={dests.length > 1 ? i + 1 : undefined}
                    mode={mode}
                    expand={dests.length === 1}
                    compact={dests.length > 1}
                    sourcesCapturing={capturing.length}
                    botReady={bot === "ready"}
                    botName={botName}
                    limiterOn={limiterOn}
                    h={{
                      dismiss: () => adapter.dismissError(`dst:${d.uid}`),
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
                      bot: () => open({ kind: "bot" }),
                    }}
                  />
                ))}
                <div className="list-fade" aria-hidden />
              </div>
            )}
          </section>
        </main>
        {warning && (
          <div className="save-strip" inert={sheet ? true : undefined}>
            <SaveStatus
              warning={warning}
              busy={saving}
              retry={() => void retrySave()}
              details={() => open({ kind: "settings", focusSave: true })}
            />
          </div>
        )}
        {sheet?.kind === "source" && snapshot && (
          <SourcePicker
            sources={sources}
            catalog={catalog}
            error={
              replacingSource
                ? (view.targetErrors[`src:${replacingSource.uid}`]?.error
                    .message ?? null)
                : failedCommand &&
                    ["add-source", "refresh-sources"].includes(
                      failedCommand.type,
                    )
                  ? (view.commandError?.message ?? null)
                  : null
            }
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
            error={
              replacingDest
                ? (view.targetErrors[`dst:${replacingDest.uid}`]?.error
                    .message ?? null)
                : failedCommand &&
                    [
                      "add-destination",
                      "add-playback-destination",
                      "refresh-catalog",
                      "refresh-playback-devices",
                    ].includes(failedCommand.type)
                  ? (view.commandError?.message ?? null)
                  : null
            }
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
            error={
              view.configurationError?.message ??
              (view.commandErrorCommand?.type === "recheck-configuration"
                ? (view.commandError?.message ?? null)
                : null)
            }
            statusMessage={snapshot?.bot.message ?? null}
            onRecheck={() => void send({ type: "recheck-configuration" })}
            replacementBlocked={live}
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
            saveWarning={warning}
            saving={saving}
            onRetrySave={() => void retrySave()}
            error={
              failedCommand?.type === "set-settings"
                ? (view.commandError?.message ?? null)
                : null
            }
            onSettings={(s) => void adapter.setSettings(s)}
            meterMode={mode}
            onMeterMode={(meterStyle) =>
              void adapter.setSettings({ ...snapshot.settings, meterStyle })
            }
            onLicenses={() => setSheet({ kind: "licenses" })}
            focusLicenses={Boolean(sheet.focusLicenses)}
            onSyncDetails={() => setSheet({ kind: "sync-details" })}
            focusSyncDetails={Boolean(sheet.focusSyncDetails)}
            focusSave={Boolean(sheet.focusSave)}
            onBot={() =>
              setSheet(bot === "ready" ? { kind: "bot-info" } : { kind: "bot" })
            }
            onClose={close}
          />
        )}
        {netDest && <ConnectionSheet dest={netDest} onClose={close} />}
        {sheet?.kind === "sync-details" && snapshot && (
          <SyncDetails
            sources={sources}
            dests={dests}
            settings={snapshot.settings}
            onClose={() =>
              setSheet({ kind: "settings", focusSyncDetails: true })
            }
          />
        )}
        {sheet?.kind === "licenses" && (
          <Licenses
            onClose={() => setSheet({ kind: "settings", focusLicenses: true })}
          />
        )}
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
