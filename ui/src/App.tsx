import {
  Alert,
  Button,
  Modal,
  NativeSelect,
  Slider,
  Stack,
  Text,
  VisuallyHidden,
} from "@mantine/core";
import { useEffect, useState, useSyncExternalStore } from "react";
import styles from "./App.module.css";
import type { ControlAdapter } from "./adapter";
import type { FixtureName, FixtureTransport } from "./fixtures";
import type { ControlSnapshot } from "./generated";
import {
  captureLabel,
  connectionLabel,
  connectStageLabels,
  locksRoute,
  outputLabel,
  readiness,
} from "./presentation";

function InputLevel({ snapshot }: { snapshot: ControlSnapshot | null }) {
  const running = snapshot?.capture.phase === "running";
  const level = running ? snapshot.capture.levelDbfs : null;
  const label =
    level === null
      ? "No level"
      : level <= -60
        ? "Silent"
        : `${Math.round(level)} dBFS`;
  const width =
    level === null ? 0 : Math.max(0, Math.min(100, ((level + 60) / 60) * 100));
  return (
    <div className={styles.inputLevel}>
      <div className={styles.levelLabel}>
        <span id="level-label">
          Input level <span className={styles.secondary}>before output</span>
        </span>
        <span data-testid="input-level">{label}</span>
      </div>
      <VisuallyHidden>
        <meter
          aria-labelledby="level-label"
          aria-valuetext={label}
          min={-60}
          max={0}
          value={level ?? -60}
        />
      </VisuallyHidden>
      <div className={styles.meter} aria-hidden="true">
        <span style={{ width: `${width}%` }} />
      </div>
      <p className={styles.captureStatus} data-testid="capture-status">
        {snapshot ? captureLabel(snapshot) : "Waiting for app status"}
      </p>
    </div>
  );
}

export function App({
  adapter,
  fixtures,
}: {
  adapter: ControlAdapter;
  fixtures: FixtureTransport | undefined;
}) {
  const view = useSyncExternalStore(adapter.subscribe, adapter.getSnapshot);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [preview, setPreview] = useState(fixtures?.name ?? "disconnected");
  const snapshot = view.snapshot;
  const ready = view.phase === "ready" && snapshot !== null;
  const locked = snapshot ? locksRoute(snapshot) : true;
  const server = snapshot?.servers.find(
    (item) => item.id === snapshot.selection.serverId,
  );
  const source = snapshot?.sources.find(
    (item) => item.id === snapshot.selection.sourceId,
  );
  const channel = server?.channels.find(
    (item) => item.id === snapshot?.selection.channelId,
  );
  const sourceId = snapshot?.selection.sourceId ?? "";
  const serverId = snapshot?.selection.serverId ?? "";
  const channelId = snapshot?.selection.channelId ?? "";
  const discoveryLoading = snapshot?.discovery.phase === "loading";
  const botBusy =
    snapshot?.bot.status === "checking" ||
    view.pending.includes("recheck-configuration");
  const catalogBusy =
    discoveryLoading || view.pending.includes("refresh-catalog");
  const reason = snapshot ? readiness(snapshot) : "Waiting for app status.";
  const canStop =
    snapshot &&
    ["connecting", "connected", "recovering"].includes(
      snapshot.connection.phase,
    );
  const cleanup = snapshot?.connection.phase === "cleanup-incomplete";
  const stopping = snapshot?.connection.phase === "disconnecting";
  const actionLabel = cleanup
    ? "Retry disconnect"
    : canStop || stopping
      ? "Disconnect"
      : "Connect";
  const actionDisabled =
    !ready ||
    stopping ||
    view.pending.includes("disconnect") ||
    (!canStop &&
      !cleanup &&
      (reason !== null || view.pending.includes("connect")));
  const volume = view.volumeDraft ?? snapshot?.output.volumePercent ?? 0;
  const muted = snapshot?.output.muted ?? false;

  useEffect(() => {
    void adapter.start();
    return adapter.dispose;
  }, [adapter]);

  const refresh = () => {
    void adapter.dispatch({ type: "refresh-catalog" });
  };
  const recheck = () => {
    void adapter.dispatch({ type: "recheck-configuration" });
  };
  const botLabel =
    snapshot?.bot.status === "ready"
      ? `Bot: ${snapshot.bot.name ?? "Ready"}`
      : snapshot?.bot.status === "checking"
        ? "Checking bot configuration…"
        : snapshot?.bot.status === "invalid"
          ? "Bot configuration invalid"
          : snapshot?.bot.status === "error"
            ? "Bot configuration failed"
            : "Bot configuration needed";

  return (
    <main className={styles.app}>
      <header className={styles.header}>
        <h1>Echocord</h1>
        <Button
          className={styles.quietButton}
          variant="default"
          onClick={() => setSettingsOpen(true)}
        >
          Bot settings
        </Button>
      </header>
      {fixtures && (
        <aside className={styles.fixtureNotice} aria-label="Fixture mode">
          Fixture preview — no audio or Discord connection
        </aside>
      )}
      <div className={styles.routeScroll} data-testid="route-scroll">
        {view.phase === "loading" && (
          <p role="status" className={styles.notice}>
            Loading app status…
          </p>
        )}
        {view.error && (
          <Alert
            className={styles.errorAlert}
            color="red"
            title="App status unavailable"
            role="alert"
          >
            {view.error.message}
            <Button
              variant="default"
              mt="sm"
              onClick={() => void adapter.start()}
            >
              Retry loading status
            </Button>
          </Alert>
        )}
        {view.commandError && (
          <Alert
            className={styles.errorAlert}
            color="red"
            title="Control not applied"
            role="alert"
          >
            {view.commandError.message}
            <Button variant="default" mt="sm" onClick={adapter.dismissError}>
              Dismiss error
            </Button>
          </Alert>
        )}
        <NativeSelect
          label="Recording device"
          title={source?.name ?? "Choose a recording device"}
          classNames={{ input: styles.routeInput }}
          disabled={
            !ready ||
            locked ||
            catalogBusy ||
            view.pending.includes("select-source")
          }
          value={sourceId}
          data={[
            {
              value: "",
              label:
                snapshot?.sources.length === 0
                  ? "No recording devices"
                  : "Select a recording device",
              disabled: true,
            },
            ...(sourceId && !source
              ? [
                  {
                    value: sourceId,
                    label: "Saved recording device unavailable",
                    disabled: true,
                  },
                ]
              : []),
            ...(snapshot?.sources.map((item) => ({
              value: item.id,
              label: item.name + (item.available ? "" : " (unavailable)"),
              disabled: !item.available,
            })) ?? []),
          ]}
          onChange={(event) =>
            void adapter.dispatch({
              type: "select-source",
              sourceId: event.currentTarget.value,
            })
          }
        />
        {source?.detail && (
          <p className={styles.deviceDetail}>{source.detail}</p>
        )}
        <InputLevel snapshot={snapshot} />
        {snapshot?.sources.length === 0 && !catalogBusy && (
          <p className={styles.notice}>
            No recording devices found. Enable the recording device in Windows
            Sound settings, then refresh.
          </p>
        )}
        {sourceId && !source?.available && !snapshot?.capture.message && (
          <p className={styles.warning}>
            The saved recording device is unavailable. Restore it or choose
            another while disconnected.
          </p>
        )}
        {snapshot?.capture.message && (
          <Alert
            className={styles.errorAlert}
            color="yellow"
            title={captureLabel(snapshot)}
            role="alert"
          >
            {snapshot.capture.message}
          </Alert>
        )}
        <div className={styles.destination}>
          <NativeSelect
            label="Server"
            title={server?.name ?? "Choose a server"}
            classNames={{ input: styles.routeInput }}
            disabled={
              !ready ||
              locked ||
              catalogBusy ||
              snapshot?.bot.status !== "ready" ||
              view.pending.includes("select-destination")
            }
            value={serverId}
            data={[
              {
                value: "",
                label:
                  snapshot?.servers.length === 0
                    ? "No servers available"
                    : "Select a server",
                disabled: true,
              },
              ...(serverId && !server
                ? [
                    {
                      value: serverId,
                      label: "Saved server unavailable",
                      disabled: true,
                    },
                  ]
                : []),
              ...(snapshot?.servers.map((item) => ({
                value: item.id,
                label: item.name,
              })) ?? []),
            ]}
            onChange={(event) =>
              void adapter.dispatch({
                type: "select-destination",
                serverId: event.currentTarget.value,
                channelId: null,
              })
            }
          />
          <NativeSelect
            label="Voice channel"
            title={channel?.name ?? "Choose a voice channel"}
            classNames={{ input: styles.routeInput }}
            disabled={
              !ready ||
              locked ||
              catalogBusy ||
              !server ||
              snapshot?.bot.status !== "ready" ||
              view.pending.includes("select-destination")
            }
            value={channelId}
            data={[
              {
                value: "",
                label:
                  server?.channels.length === 0
                    ? "No voice channels"
                    : "Select a voice channel",
                disabled: true,
              },
              ...(channelId && !channel
                ? [
                    {
                      value: channelId,
                      label: "Saved channel unavailable",
                      disabled: true,
                    },
                  ]
                : []),
              ...(server?.channels.map((item) => ({
                value: item.id,
                label: item.name,
              })) ?? []),
            ]}
            onChange={(event) => {
              if (server)
                void adapter.dispatch({
                  type: "select-destination",
                  serverId: server.id,
                  channelId: event.currentTarget.value,
                });
            }}
          />
        </div>
        {server && server.channels.length === 0 && !catalogBusy && (
          <p className={styles.notice}>
            This server has no available voice channels. Check bot access or
            choose another server.
          </p>
        )}
        {snapshot?.servers.length === 0 && !catalogBusy && (
          <p className={styles.notice}>
            No servers are available. Check the bot's server membership and
            configuration, then refresh.
          </p>
        )}
        <div className={styles.botRow}>
          <span>{botLabel}</span>
          <Button
            className={styles.quietButton}
            variant="default"
            size="xs"
            disabled={!ready || locked || catalogBusy || botBusy}
            onClick={refresh}
          >
            Refresh
          </Button>
        </div>
        {discoveryLoading && (
          <p className={styles.notice} role="status">
            Loading devices and servers…
          </p>
        )}
        {snapshot?.discovery.phase === "error" && (
          <Alert
            className={styles.errorAlert}
            color="red"
            title="Could not refresh"
            role="alert"
          >
            {snapshot.discovery.message}
          </Alert>
        )}
        {snapshot?.connection.phase === "error" && (
          <Alert
            className={styles.errorAlert}
            color="red"
            title="Connection failed"
            role="alert"
          >
            {snapshot.connection.message}
          </Alert>
        )}
        {snapshot?.connection.phase === "recovering" && (
          <p className={styles.warning}>
            {snapshot.connection.message} Attempt {snapshot.connection.attempt}.
          </p>
        )}
        {cleanup && snapshot?.connection.phase === "cleanup-incomplete" && (
          <Alert
            className={styles.errorAlert}
            color="yellow"
            title="Cleanup incomplete"
            role="alert"
          >
            {snapshot.connection.message}
          </Alert>
        )}
        {locked && (
          <p className={styles.notice}>
            Disconnect before changing the route or bot configuration.
          </p>
        )}
        <details className={styles.routingHelp}>
          <summary>Routing help</summary>
          <p>
            Send your music app to VB-Cable's playback device, and select its
            recording device above. Echocord provides no local monitoring.
          </p>
        </details>
        {fixtures && (
          <details className={styles.fixtureControls}>
            <summary>Fixture states</summary>
            <NativeSelect
              label="Fixture state"
              value={preview}
              data={fixtures.names.map((name) => ({
                value: name,
                label: name.replaceAll("-", " "),
              }))}
              onChange={(event) => {
                const name = event.currentTarget.value as FixtureName;
                setPreview(name);
                fixtures.preview(name);
                adapter.dismissError();
              }}
            />
          </details>
        )}
      </div>

      <section className={styles.controls} aria-label="Session controls">
        <div className={styles.sessionRow}>
          <h2 data-testid="connection-status">
            {snapshot
              ? connectionLabel(snapshot)
              : view.phase === "error"
                ? "App unavailable"
                : "Loading app"}
          </h2>
          <Button
            className={
              canStop || cleanup || stopping
                ? styles.secondaryAction
                : styles.accentButton
            }
            variant={canStop || cleanup || stopping ? "default" : "filled"}
            disabled={Boolean(actionDisabled)}
            onClick={() =>
              void adapter.dispatch({
                type: canStop || cleanup ? "disconnect" : "connect",
              })
            }
          >
            {actionLabel}
          </Button>
        </div>
        {snapshot?.connection.phase === "connecting" && (
          <p
            className={styles.readiness}
            data-testid="connection-progress"
            role="status"
          >
            {connectStageLabels[snapshot.connection.stage]}
          </p>
        )}
        {reason && !canStop && (
          <p className={styles.readiness} data-testid="readiness">
            {reason}
          </p>
        )}
        <div className={styles.outputRow}>
          <span>Output</span>
          <strong data-testid="output-status">
            {snapshot ? outputLabel(snapshot) : "Unavailable"}
          </strong>
        </div>
        <div className={styles.volumeAndMute}>
          <div className={styles.volume}>
            <div className={styles.volumeLabel}>
              <span>Discord volume</span>
              <output aria-label="Discord volume value">
                {snapshot
                  ? `${volume}%${view.volumeDraft !== null ? " pending" : ""}`
                  : "—"}
              </output>
            </div>
            <Slider
              thumbLabel="Discord volume"
              min={0}
              max={100}
              step={1}
              value={volume}
              disabled={!ready}
              onChange={adapter.setVolume}
              label={(value) => `${value}%`}
              size="sm"
            />
          </div>
          <Button
            className={muted ? styles.accentButton : styles.secondaryAction}
            variant={muted ? "filled" : "default"}
            aria-label="Mute output"
            aria-pressed={muted}
            disabled={!ready || view.pending.includes("set-muted")}
            onClick={() =>
              void adapter.dispatch({ type: "set-muted", muted: !muted })
            }
          >
            Mute output{muted ? " ✓" : ""}
          </Button>
        </div>
        {view.commandError && (
          <p className={styles.footerError}>
            Control not applied. See the error above.
          </p>
        )}
        <VisuallyHidden>
          <div role="status" aria-live="polite" aria-atomic="true">
            {snapshot
              ? `${connectionLabel(snapshot)}. ${captureLabel(snapshot)}. ${outputLabel(snapshot)}.`
              : view.phase === "error"
                ? "App unavailable."
                : "Loading app."}
          </div>
        </VisuallyHidden>
      </section>

      <Modal
        opened={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        title="Bot settings"
        centered
        size="sm"
        classNames={{ content: styles.modal }}
        closeButtonProps={{
          "aria-label": "Close bot settings",
        }}
      >
        <Stack gap="md">
          <div>
            <Text fw={600}>{botLabel}</Text>
            <Text size="sm" c="dimmed">
              {snapshot?.bot.message ??
                "Configuration is loaded by the app. No credentials are entered or stored in this window."}
            </Text>
          </div>
          <Text size="sm" c="dimmed">
            After updating the app's bot configuration, recheck it and refresh
            the server list. The bot needs Connect and Speak access to the
            chosen voice channel.
          </Text>
          <Button
            className={styles.secondaryAction}
            variant="default"
            disabled={!ready || locked || botBusy || catalogBusy}
            onClick={recheck}
          >
            Recheck configuration
          </Button>
          <Button
            className={styles.secondaryAction}
            variant="default"
            disabled={
              !ready ||
              locked ||
              catalogBusy ||
              snapshot?.bot.status !== "ready" ||
              botBusy
            }
            onClick={refresh}
          >
            Refresh servers
          </Button>
          {locked && (
            <Text size="sm" c="dimmed">
              Settings remain readable. Disconnect before rechecking
              configuration or refreshing servers.
            </Text>
          )}
          {view.commandError && (
            <Alert color="red" className={styles.errorAlert} role="alert">
              {view.commandError.message}
            </Alert>
          )}
          {snapshot?.discovery.phase === "error" && (
            <Alert color="red" className={styles.errorAlert} role="alert">
              {snapshot.discovery.message}
            </Alert>
          )}
        </Stack>
      </Modal>
    </main>
  );
}
