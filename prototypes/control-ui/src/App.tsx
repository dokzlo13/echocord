import {
  Alert,
  Badge,
  Button,
  Modal,
  NativeSelect,
  Slider,
  Stack,
  Switch,
  Text,
  VisuallyHidden,
} from "@mantine/core";
import { useState, useSyncExternalStore } from "react";
import styles from "./App.module.css";
import {
  type ControlCommand,
  type ControlSnapshot,
  deliveryLabel,
  sessionActive,
} from "./contract";
import {
  type DemoBotProfile,
  type MockControlAdapter,
  type Preview,
  previews,
} from "./mockAdapter";

function statusView(snapshot: ControlSnapshot) {
  if (snapshot.capture.health === "missing-device") {
    return {
      label: "Device missing",
      detail: sessionActive(snapshot.connection)
        ? "Waiting for the same recording device. No alternate input will be selected."
        : "Restore the selected recording device, or choose another while disconnected.",
      tone: "yellow",
    };
  }
  switch (snapshot.connection.phase) {
    case "disconnected":
      return {
        label: "Disconnected",
        detail: "Ready when your table is. Connect to start the demo.",
        tone: "gray",
      };
    case "connecting":
      return {
        label: "Connecting",
        detail: "Joining the selected voice channel in the demo…",
        tone: "teal",
      };
    case "connected":
      return snapshot.output.muted
        ? {
            label: "Muted",
            detail:
              "The demo session stays connected. Input activity is still visible.",
            tone: "yellow",
          }
        : {
            label: "Streaming",
            detail: "Demo session connected. Delivery below is simulated.",
            tone: "teal",
          };
    case "recovering":
      return {
        label: "Recovering",
        detail: `Reconnect attempt ${snapshot.connection.attempt} · same source and channel. You can disconnect to cancel.`,
        tone: "yellow",
      };
    case "error":
      return {
        label: "Connection error",
        detail: snapshot.connection.message,
        tone: "red",
      };
  }
}

const meterSegments = Array.from(
  { length: 32 },
  (_, index) => (index / 32) * 100,
);

function InputMeter({ snapshot }: { snapshot: ControlSnapshot }) {
  const level = snapshot.capture.levelDbfs;
  const percent =
    level === null ? 0 : Math.round(((Math.max(-60, level) + 60) / 60) * 100);
  const label =
    level === null ? "Unavailable" : level <= -60 ? "Silent" : `${level} dBFS`;
  return (
    <div className={styles.meterBlock}>
      <div className={styles.meterLabel}>
        <span id="input-level-label">
          Input level <span className={styles.dim}>· simulated</span>
        </span>
        <span className={styles.meterValue} data-testid="input-level-value">
          {label}
        </span>
      </div>
      <VisuallyHidden>
        <meter
          aria-labelledby="input-level-label"
          min={-60}
          max={0}
          value={level ?? -60}
          aria-valuetext={label}
        />
      </VisuallyHidden>
      <div aria-hidden="true" className={styles.meter}>
        {meterSegments.map((threshold) => (
          <span
            key={threshold}
            className={threshold < percent ? styles.lit : styles.unlit}
          />
        ))}
      </div>
    </div>
  );
}

export function App({ adapter }: { adapter: MockControlAdapter }) {
  const snapshot = useSyncExternalStore(adapter.subscribe, adapter.getSnapshot);
  const [settingsOpened, setSettingsOpened] = useState(false);
  const [demoProfile, setDemoProfile] = useState<DemoBotProfile>(
    snapshot.bot.status,
  );
  const [commandError, setCommandError] = useState<string | null>(null);
  const [preview, setPreview] = useState("");
  const active = sessionActive(snapshot.connection);
  const server = snapshot.servers.find(
    (item) => item.id === snapshot.selection.serverId,
  );
  const status = statusView(snapshot);
  const missing = snapshot.capture.health === "missing-device";
  const needsBot = snapshot.bot.status !== "ready";
  const connectDisabled =
    !active && (missing || snapshot.selection.channelId === null);

  const send = async (command: ControlCommand) => {
    setPreview("");
    const result = await adapter.dispatch(command);
    setCommandError(result.ok ? null : result.message);
  };
  const openSettings = () => {
    setDemoProfile(snapshot.bot.status);
    setSettingsOpened(true);
  };

  return (
    <main className={styles.shell}>
      <header className={styles.header}>
        <div className={styles.brand}>
          <div className={styles.brandMark} aria-hidden="true">
            <span />
            <span />
            <span />
            <span />
          </div>
          <div>
            <h1>Echocord</h1>
            <p>Audio for your table</p>
          </div>
        </div>
        <Button variant="subtle" color="gray" onClick={openSettings}>
          Bot setup
        </Button>
      </header>

      <aside className={styles.prototypeNotice} aria-label="Prototype notice">
        <Badge variant="light" color="teal" size="sm">
          Prototype
        </Badge>
        <span>Mock adapter · no audio or Discord connection</span>
      </aside>

      <div className={styles.routing}>
        <section
          className={styles.sourceSection}
          aria-labelledby="source-heading"
        >
          <div className={styles.sectionHeading}>
            <h2 id="source-heading">Source</h2>
            <span>One recording device</span>
          </div>
          <NativeSelect
            label="Recording source"
            classNames={{ input: styles.routeInput }}
            title={
              snapshot.sources.find(
                (source) => source.id === snapshot.selection.sourceId,
              )?.name ?? ""
            }
            value={snapshot.selection.sourceId ?? ""}
            disabled={active}
            data={snapshot.sources.map((source) => ({
              value: source.id,
              label: source.name + (source.available ? "" : " · unavailable"),
            }))}
            onChange={(event) =>
              void send({
                type: "select-source",
                sourceId: event.currentTarget.value,
              })
            }
          />
          <p className={styles.help}>
            Route your music app to CABLE Input in Windows. No local monitoring.
          </p>
          <InputMeter snapshot={snapshot} />
        </section>

        <section
          className={styles.destinationSection}
          aria-labelledby="destination-heading"
        >
          <div className={styles.sectionHeading}>
            <h2 id="destination-heading">Destination</h2>
            <span>Discord voice · demo</span>
          </div>
          <div className={styles.destinationFields}>
            <NativeSelect
              label="Server"
              classNames={{ input: styles.routeInput }}
              title={server?.name ?? ""}
              value={snapshot.selection.serverId ?? ""}
              disabled={active || needsBot}
              data={snapshot.servers.map((item) => ({
                value: item.id,
                label: item.name,
              }))}
              onChange={(event) =>
                void send({
                  type: "select-destination",
                  serverId: event.currentTarget.value,
                  channelId: null,
                })
              }
            />
            <NativeSelect
              label="Voice channel"
              classNames={{ input: styles.routeInput }}
              title={
                server?.channels.find(
                  (channel) => channel.id === snapshot.selection.channelId,
                )?.name ?? ""
              }
              value={snapshot.selection.channelId ?? ""}
              disabled={active || needsBot || !server}
              data={[
                { value: "", label: "Select a voice channel", disabled: true },
                ...(server?.channels.map((channel) => ({
                  value: channel.id,
                  label: channel.name,
                })) ?? []),
              ]}
              onChange={(event) => {
                if (server)
                  void send({
                    type: "select-destination",
                    serverId: server.id,
                    channelId: event.currentTarget.value,
                  });
              }}
            />
          </div>
          <div className={styles.botLine}>
            <span className={styles.botDot} aria-hidden="true" />
            {snapshot.bot.name ?? "No demo bot configured"}
            <span className={styles.dim}>
              ·{" "}
              {snapshot.bot.status === "ready"
                ? "demo setup ready"
                : snapshot.bot.status === "unconfigured"
                  ? "setup needed"
                  : "permissions needed"}
            </span>
          </div>
          {active && (
            <p className={styles.lockedHint}>
              Disconnect to change the source or destination.
            </p>
          )}
        </section>
      </div>

      <section className={styles.session} aria-labelledby="session-heading">
        <div className={styles.sessionTop}>
          <div
            className={styles.statusText}
            role="status"
            aria-live="polite"
            aria-atomic="true"
          >
            <div className={styles.statusHeading}>
              <span
                className={`${styles.statusDot} ${styles[status.tone]}`}
                aria-hidden="true"
              />
              <h2 id="session-heading" data-testid="connection-status">
                {status.label}
              </h2>
              {active &&
                snapshot.output.muted &&
                snapshot.connection.phase !== "connected" && (
                  <Badge color="yellow" variant="light" size="sm">
                    Muted
                  </Badge>
                )}
            </div>
            <p>{status.detail}</p>
          </div>
          <Button
            className={styles.connectButton}
            variant={active ? "default" : "filled"}
            disabled={connectDisabled}
            onClick={() => {
              if (active) void send({ type: "disconnect" });
              else if (needsBot) openSettings();
              else void send({ type: "connect" });
            }}
          >
            {active ? "Disconnect" : needsBot ? "Set up bot" : "Connect"}
          </Button>
        </div>

        {!active && missing && (
          <p className={styles.actionHint}>
            Check the selected device in Windows Sound settings. Use Demo
            controls below to simulate its return.
          </p>
        )}
        {!active && !missing && snapshot.selection.channelId === null && (
          <p className={styles.actionHint}>
            Select a voice channel to enable Connect.
          </p>
        )}
        {snapshot.connection.phase === "error" && (
          <p className={styles.actionHint}>
            Review bot setup and channel permissions. Rejoining requires your
            action.
          </p>
        )}

        <div className={styles.outputControls}>
          <div className={styles.volumeControl}>
            <div className={styles.volumeLabel}>
              <span>Output volume</span>
              <output aria-label="Output volume value">
                {snapshot.output.volumePercent}%
              </output>
            </div>
            <Slider
              thumbLabel="Output volume"
              min={0}
              max={100}
              step={1}
              value={snapshot.output.volumePercent}
              label={(value) => `${value}%`}
              onChange={(percent) => void send({ type: "set-volume", percent })}
              size="sm"
            />
          </div>
          <Button
            variant={snapshot.output.muted ? "light" : "default"}
            color={snapshot.output.muted ? "yellow" : "gray"}
            aria-pressed={snapshot.output.muted}
            aria-label={snapshot.output.muted ? "Unmute output" : "Mute output"}
            onClick={() =>
              void send({ type: "set-muted", muted: !snapshot.output.muted })
            }
          >
            {snapshot.output.muted ? "Unmute" : "Mute"}
          </Button>
        </div>
        <div className={styles.delivery}>
          <span>Delivery</span>
          <strong data-testid="delivery-status">
            {deliveryLabel(snapshot)}
          </strong>
        </div>
      </section>

      {commandError && (
        <Alert color="red" title="Control unavailable" role="alert">
          {commandError}
        </Alert>
      )}

      <footer className={styles.footer}>
        <span>One source → one channel</span>
        <span>Control UI study</span>
      </footer>

      <details className={styles.demoTools}>
        <summary>Demo controls</summary>
        <div className={styles.demoToolsBody}>
          <p>
            Fixed fixtures only. State previews pause automatic transitions.
          </p>
          <NativeSelect
            label="State preview"
            value={preview}
            data={[
              { value: "", label: "Follow controls" },
              ...previews.map((value) => ({
                value,
                label: value.replace("-", " "),
              })),
            ]}
            onChange={(event) => {
              const value = event.currentTarget.value;
              setPreview(value);
              setCommandError(null);
              adapter.preview((value || "disconnected") as Preview);
            }}
          />
          <Switch
            label="Silent demo input"
            checked={snapshot.capture.levelDbfs === -60}
            disabled={missing}
            onChange={(event) =>
              adapter.setDemoSignal(event.currentTarget.checked)
            }
          />
          <div className={styles.demoActions}>
            <Button
              variant="default"
              size="xs"
              disabled={snapshot.connection.phase !== "connected"}
              onClick={() => {
                setPreview("");
                adapter.simulateNetworkLoss();
              }}
            >
              Drop demo connection
            </Button>
            <Button
              variant="default"
              size="xs"
              onClick={() => {
                setPreview("");
                missing ? adapter.returnDevice() : adapter.removeDevice();
              }}
            >
              {missing ? "Return demo device" : "Remove demo device"}
            </Button>
          </div>
        </div>
      </details>

      <Modal
        opened={settingsOpened}
        onClose={() => setSettingsOpened(false)}
        title="Bot setup · demo only"
        centered
        size="sm"
        closeButtonProps={{ "aria-label": "Close bot setup" }}
      >
        <Stack gap="md">
          <Alert color="teal" title="No real credentials">
            This prototype only uses fixed demo profiles. It cannot accept or
            store a Discord bot token.
          </Alert>
          <NativeSelect
            data-autofocus
            label="Demo bot profile"
            value={demoProfile}
            disabled={active}
            data={[
              { value: "ready", label: "Echocord Demo · ready" },
              { value: "unconfigured", label: "No bot configured" },
              {
                value: "permission-denied",
                label: "Echocord Demo · permissions denied",
              },
            ]}
            onChange={(event) =>
              setDemoProfile(event.currentTarget.value as DemoBotProfile)
            }
          />
          <div className={styles.permissionList}>
            <span>Connect to voice</span>
            <strong>
              {demoProfile === "ready"
                ? "Simulated permission"
                : "Not available"}
            </strong>
            <span>Speak in channel</span>
            <strong>
              {demoProfile === "ready"
                ? "Simulated permission"
                : "Not available"}
            </strong>
          </div>
          <Text size="sm" c="dimmed">
            Real bot authorization and secure credential storage belong to the
            future Rust adapter.
          </Text>
          {active && (
            <Text size="sm" c="yellow">
              Disconnect before changing bot setup.
            </Text>
          )}
          <Button
            disabled={active}
            onClick={() => {
              adapter.configureDemoBot(demoProfile);
              setCommandError(null);
              setPreview("");
              setSettingsOpened(false);
            }}
          >
            Use demo setup
          </Button>
        </Stack>
      </Modal>
    </main>
  );
}
