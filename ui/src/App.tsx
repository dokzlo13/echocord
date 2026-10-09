import { ActionIcon, Button, Slider } from "@mantine/core";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import styles from "./App.module.css";
import type { ControlAdapter } from "./adapter";
import {
  AudioDetails,
  BotSetup,
  type DrawerName,
  RoomPicker,
  ServerIcon,
  Sheet,
  SourcePicker,
} from "./Drawers";
import type { Selection } from "./generated";
import { Icon } from "./Icons";
import {
  captureLabel,
  connectionLabel,
  connectStageLabels,
  locksRoute,
  outputLabel,
  readiness,
} from "./presentation";
import { CaptureQueue, SignalMeter } from "./Telemetry";

function sameRoute(first: Selection, second: Selection) {
  return (
    first.sourceId === second.sourceId &&
    first.serverId === second.serverId &&
    first.channelId === second.channelId
  );
}

export function App({ adapter }: { adapter: ControlAdapter }) {
  const view = useSyncExternalStore(adapter.subscribe, adapter.getSnapshot);
  const [drawer, setDrawer] = useState<DrawerName | null>(null);
  const [draft, setDraft] = useState<Selection | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const roomButton = useRef<HTMLButtonElement>(null);
  const snapshot = view.snapshot;
  const ready = view.phase === "ready" && snapshot !== null;
  const selection = snapshot?.selection ?? {
    sourceId: null,
    serverId: null,
    channelId: null,
  };
  const queued = draft ?? selection;
  const dirty = draft !== null && !sameRoute(draft, selection);
  const source = snapshot?.sources.find(
    (item) => item.id === selection.sourceId,
  );
  const server = snapshot?.servers.find(
    (item) => item.id === selection.serverId,
  );
  const room = server?.channels.find((item) => item.id === selection.channelId);
  const nextSource = snapshot?.sources.find(
    (item) => item.id === queued.sourceId,
  );
  const nextServer = snapshot?.servers.find(
    (item) => item.id === queued.serverId,
  );
  const nextRoom = nextServer?.channels.find(
    (item) => item.id === queued.channelId,
  );
  const operation = view.routeOperation;
  const phase = snapshot?.connection.phase;
  const active =
    phase === "connected" || phase === "connecting" || phase === "recovering";
  const cleanup = phase === "cleanup-incomplete";
  const stopping = phase === "disconnecting";
  const volume = view.volumeDraft ?? snapshot?.output.volumePercent ?? 80;
  const muted = snapshot?.output.muted ?? false;
  const locked =
    !ready || Boolean(operation) || view.configuringBot || cleanup || stopping;
  const catalogBusy =
    snapshot?.discovery.phase === "loading" ||
    view.pending.includes("refresh-catalog");
  const reason = snapshot ? readiness(snapshot) : "Waiting for app status.";
  const routeValid = Boolean(
    nextSource?.available &&
      nextRoom &&
      nextServer &&
      snapshot?.bot.status === "ready" &&
      snapshot.discovery.phase === "ready",
  );

  const open = (name: DrawerName) => {
    opener.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    setDrawer(name);
  };
  const close = () => {
    setDrawer(null);
    requestAnimationFrame(() => opener.current?.focus());
  };
  useEffect(() => {
    void adapter.start();
    return adapter.dispose;
  }, [adapter]);
  useEffect(() => {
    const shortcut = (event: KeyboardEvent) => {
      if (
        event.ctrlKey &&
        event.key.toLowerCase() === "k" &&
        ready &&
        !locked
      ) {
        event.preventDefault();
        opener.current = roomButton.current;
        setDrawer("target");
      }
    };
    window.addEventListener("keydown", shortcut);
    return () => window.removeEventListener("keydown", shortcut);
  }, [ready, locked]);

  const messages = [
    view.error?.message,
    view.commandError?.message,
    snapshot?.capture.message,
    snapshot?.connection.phase === "error" ||
    snapshot?.connection.phase === "recovering" ||
    snapshot?.connection.phase === "cleanup-incomplete"
      ? snapshot.connection.message
      : null,
    snapshot?.discovery.phase === "error" ? snapshot.discovery.message : null,
    snapshot?.bot.message,
  ].filter((message): message is string => Boolean(message));
  let notice: string | null = null;
  if (view.error) notice = view.error.message;
  else if (view.commandError)
    notice = dirty
      ? "Route change incomplete. Confirmed settings are shown; remaining changes are queued."
      : "Control not confirmed. Check the current status before retrying.";
  else if (cleanup)
    notice =
      "The session has not exited. Retry stop before applying or connecting.";
  else if (snapshot?.capture.phase === "missing")
    notice =
      "Recording input missing. Restore it or stop and choose another input.";
  else if (snapshot?.capture.phase === "error")
    notice = "Recording input failed. Check its Windows sound settings.";
  else if (phase === "error")
    notice = "Connection failed. Review the bot's voice room permissions.";
  else if (snapshot?.discovery.phase === "error")
    notice = "Could not load inputs or rooms. Open the picker to refresh.";
  else if (
    snapshot?.bot.status === "invalid" ||
    snapshot?.bot.status === "error"
  )
    notice = "Bot configuration needs attention. Open bot settings to test it.";

  const status = snapshot
    ? connectionLabel(snapshot)
    : view.phase === "error"
      ? "App unavailable"
      : "Loading app";
  const progress =
    snapshot?.connection.phase === "connecting"
      ? connectStageLabels[snapshot.connection.stage]
      : snapshot?.connection.phase === "recovering"
        ? `Restoring the same room · attempt ${snapshot.connection.attempt}`
        : operation
          ? "Applying the queued route…"
          : (reason ??
            (muted
              ? "Output muted · input still active"
              : room
                ? `${phase === "connected" ? "In" : "Selected:"} ${room.name}`
                : "Choose an input and room"));
  const transportLabel = cleanup
    ? "Retry stop"
    : operation || phase === "connecting" || phase === "recovering"
      ? "Cancel"
      : active
        ? "Stop"
        : stopping
          ? "Stopping…"
          : "Connect";
  const stopAction = active || cleanup || Boolean(operation);
  const transportDisabled =
    !ready ||
    stopping ||
    view.pending.includes("disconnect") ||
    (!stopAction &&
      (dirty ||
        Boolean(reason) ||
        view.configuringBot ||
        view.pending.includes("connect")));

  return (
    <main
      className={styles.app}
      data-crowded={Boolean(notice && (dirty || operation))}
    >
      <header className={styles.header} inert={drawer !== null}>
        <h1>
          <Icon name="bridge" />
          Echocord
        </h1>
        <div className={styles.headerActions}>
          <Button
            variant="subtle"
            size="compact-sm"
            leftSection={<Icon name="details" />}
            onClick={() => open("details")}
          >
            Audio details
          </Button>
          <ActionIcon
            variant="subtle"
            size="lg"
            aria-label="Bot settings"
            onClick={() => open("bot")}
          >
            <Icon name="settings" />
          </ActionIcon>
        </div>
      </header>
      <div
        className={styles.surface}
        inert={drawer !== null}
        data-testid="route-surface"
      >
        <section
          className={styles.inputSection}
          aria-labelledby="input-heading"
        >
          <div className={styles.sectionHeading}>
            <h2 id="input-heading">Recording input</h2>
            <span data-testid="capture-status">
              {snapshot ? captureLabel(snapshot) : "Waiting for status"}
            </span>
          </div>
          <div className={styles.inputBody}>
            <div className={styles.routeIdentity}>
              <strong title={source?.name}>
                {source?.name ??
                  (selection.sourceId
                    ? "Saved input unavailable"
                    : "Choose an input")}
              </strong>
              <span className={styles.secondary} title={source?.detail ?? ""}>
                {source?.detail ?? "Windows recording endpoint"}
              </span>
              <Button
                variant="subtle"
                size="compact-sm"
                disabled={locked}
                onClick={() => open("source")}
              >
                {source ? "Change input" : "Choose input"}
                <Icon name="chevron" />
              </Button>
            </div>
            <SignalMeter adapter={adapter} stage="input" />
            <CaptureQueue adapter={adapter} />
          </div>
          <span className={styles.boundary}>
            Input peak before gain · Engine PCM 48 kHz / 2 ch
          </span>
        </section>
        <section className={styles.mixSection} aria-labelledby="mix-heading">
          <div className={styles.mixIdentity}>
            <h2 id="mix-heading">Shared mix</h2>
            <span className={styles.secondary} data-testid="output-status">
              {snapshot ? outputLabel(snapshot) : "Output unavailable"}
            </span>
            <span id="volume-label" className={styles.volumeLabel}>
              Master volume{" "}
              <strong>
                {volume}
                <small>%</small>
              </strong>
            </span>
          </div>
          <div className={styles.volumeControl}>
            <Slider
              thumbLabel="Master volume"
              thumbValueText={(value) => `${value}%`}
              label={(value) => `${value}%`}
              min={0}
              max={100}
              step={1}
              value={volume}
              disabled={!ready}
              onChange={adapter.setVolume}
              onChangeEnd={() => void adapter.flushVolume()}
              marks={[
                { value: 0, label: "0" },
                { value: 50, label: "50" },
                { value: 100, label: "100%" },
              ]}
              classNames={{ root: styles.fader, thumb: styles.faderThumb }}
            />
          </div>
          <div className={styles.mixMeter}>
            <span>Mix peak</span>
            <SignalMeter adapter={adapter} stage="mix" />
          </div>
          <span className={styles.mixBoundary}>
            After gain & mute · before Discord
          </span>
        </section>
        <section
          className={styles.outputSection}
          aria-labelledby="output-heading"
          data-testid="destination"
        >
          <div className={styles.sectionHeading}>
            <h2 id="output-heading">Discord</h2>
            <span>
              {snapshot?.bot.status === "ready"
                ? `Bot · ${snapshot.bot.name ?? "Ready"}`
                : "Bot setup needed"}
            </span>
          </div>
          <div className={styles.outputBody}>
            <ServerIcon server={server} />
            <div className={styles.routeIdentity}>
              <strong title={room?.name}>
                {room?.name ??
                  (selection.channelId
                    ? "Saved room unavailable"
                    : "Choose a voice room")}
              </strong>
              <span className={styles.secondary} title={server?.name}>
                {server?.name ?? "No server selected"}
              </span>
            </div>
            <Button
              ref={roomButton}
              variant="subtle"
              size="compact-sm"
              disabled={locked}
              onClick={() =>
                open(snapshot?.bot.status === "ready" ? "target" : "bot")
              }
            >
              {snapshot?.bot.status === "ready"
                ? room
                  ? "Change room"
                  : "Choose room"
                : "Set up bot"}
              <Icon name="chevron" />
            </Button>
          </div>
        </section>
      </div>
      {notice && (
        <div className={styles.notice} role="alert" inert={drawer !== null}>
          <p>{notice}</p>
          <Button
            variant="subtle"
            size="compact-sm"
            onClick={() =>
              view.error ? void adapter.start() : open("details")
            }
          >
            {view.error ? "Retry status" : "Details"}
          </Button>
          {view.commandError && (
            <ActionIcon
              variant="subtle"
              aria-label="Dismiss error"
              onClick={adapter.dismissError}
            >
              <Icon name="close" />
            </ActionIcon>
          )}
        </div>
      )}
      {(dirty || operation) && (
        <section
          className={styles.pendingRoute}
          aria-label="Queued route"
          inert={drawer !== null}
        >
          <div className={styles.queuedIdentity}>
            <strong>{operation ? "Applying route…" : "Queued changes"}</strong>
            <span
              title={`${nextSource?.name ?? "Choose input"} → ${nextServer?.name ?? "Choose server"} / ${nextRoom?.name ?? "Choose room"}`}
            >
              {queued.sourceId !== selection.sourceId
                ? `${nextSource?.name ?? "Choose input"} · `
                : ""}
              {queued.serverId !== selection.serverId ||
              queued.channelId !== selection.channelId
                ? `${nextServer?.name ?? "Choose server"} / ${nextRoom?.name ?? "Choose room"}`
                : "Input selection"}
            </span>
          </div>
          <div className={styles.pendingActions}>
            <Button
              size="compact-sm"
              disabled={!routeValid || locked}
              onClick={() => {
                if (!draft || !snapshot) return;
                void adapter
                  .applyRoute(draft, locksRoute(snapshot))
                  .then((applied) => {
                    if (applied) setDraft(null);
                  });
              }}
            >
              {active ? "Apply & reconnect" : "Apply selection"}
            </Button>
            <Button
              variant="subtle"
              size="compact-sm"
              disabled={Boolean(operation)}
              onClick={() => {
                setDraft(null);
                adapter.dismissError();
              }}
            >
              Discard
            </Button>
          </div>
        </section>
      )}
      <section className={styles.transport} aria-label="Session controls">
        <div className={styles.transportStatus}>
          <strong
            data-testid="connection-status"
            data-phase={phase ?? "loading"}
          >
            {status}
          </strong>
          <span
            title={progress}
            data-testid="connection-progress"
            role="status"
          >
            {progress}
          </span>
        </div>
        <Button
          variant="default"
          className={muted ? styles.mutedButton : undefined}
          aria-label="Mute output"
          aria-pressed={muted}
          disabled={!ready || view.pending.includes("set-muted")}
          leftSection={<Icon name={muted ? "mute" : "speaker"} />}
          onClick={() =>
            void adapter.dispatch({ type: "set-muted", muted: !muted })
          }
        >
          {muted ? "Unmute" : "Mute"}
        </Button>
        <Button
          variant={stopAction || stopping ? "default" : "filled"}
          disabled={transportDisabled}
          onClick={() =>
            void adapter.dispatch({
              type: stopAction ? "disconnect" : "connect",
            })
          }
        >
          {transportLabel}
        </Button>
      </section>
      {drawer && (
        <Sheet
          name={drawer}
          title={
            {
              source: "Recording inputs",
              target: "Discord rooms",
              bot: "Bot settings",
              details: "Audio details",
            }[drawer]
          }
          close={close}
        >
          {drawer === "source" && snapshot && (
            <SourcePicker
              snapshot={snapshot}
              draft={queued}
              busy={locked || catalogBusy}
              refresh={() => void adapter.dispatch({ type: "refresh-catalog" })}
              choose={(sourceId) => {
                setDraft({ ...queued, sourceId });
                close();
              }}
            />
          )}
          {drawer === "target" && snapshot && (
            <RoomPicker
              snapshot={snapshot}
              draft={queued}
              busy={locked || catalogBusy}
              refresh={() => void adapter.dispatch({ type: "refresh-catalog" })}
              setup={() => setDrawer("bot")}
              choose={(serverId, channelId) => {
                setDraft({ ...queued, serverId, channelId });
                close();
              }}
            />
          )}
          {drawer === "bot" && (
            <BotSetup adapter={adapter} view={view} close={close} />
          )}
          {drawer === "details" && (
            <AudioDetails
              adapter={adapter}
              snapshot={snapshot}
              messages={messages}
            />
          )}
        </Sheet>
      )}
    </main>
  );
}
