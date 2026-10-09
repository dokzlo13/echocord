/* Browser-only test entry. Never imported by the production bundle. */
import "@mantine/core/styles.css";
import "../src/global.css";
import { MantineProvider } from "@mantine/core";
import { Profiler } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../src/App";
import { ControlAdapter, type Transport } from "../src/adapter";
import { fixtureSnapshot } from "../src/fixtures";
import type { ControlCommand, ControlSnapshot } from "../src/generated";
import { theme } from "../src/theme";

export interface Harness {
  commits: number;
  commands: ControlCommand[];
  emits: number;
  rejectDestination: boolean;
  rejectSave: boolean;
  holdDisconnect: boolean;
  emit: (patch: Partial<ControlSnapshot>) => void;
  visible: (visible: boolean) => void;
  burst: (count: number) => void;
  snapshot: () => ControlSnapshot;
}
declare global {
  interface Window {
    uiHarness: Harness;
  }
}
let current = fixtureSnapshot("connected");
let receive: ((snapshot: ControlSnapshot) => void) | undefined;
let visibility: ((visible: boolean) => void) | undefined;
const host: Harness = {
  commits: 0,
  emits: 0,
  commands: [],
  rejectDestination: false,
  rejectSave: false,
  holdDisconnect: false,
  snapshot: () => current,
  emit: (patch) => {
    current = { ...current, ...patch, revision: current.revision + 1 };
    host.emits++;
    receive?.(structuredClone(current));
  },
  visible: (value) => visibility?.(value),
  burst: (count) => {
    for (let i = 0; i < count; i++)
      host.emit({
        capture: {
          ...current.capture,
          levelDbfs: -30 + (i % 10),
          outputLevelDbfs: -35 + (i % 10),
          queuedFrames: i % 1920,
          droppedFrames: i,
        },
      });
  },
};
window.uiHarness = host;
const transport: Transport = {
  listen: async (callback) => {
    receive = callback;
    return () => {
      receive = undefined;
    };
  },
  listenVisibility: async (callback) => {
    visibility = callback;
    return () => {
      visibility = undefined;
    };
  },
  getVisible: async () => true,
  getSnapshot: async () => current,
  configureBot: async () => {
    await new Promise<void>((resolve) => setTimeout(resolve, 300));
    if (host.rejectSave) throw new Error("Synthetic credential save rejected");
    host.emit({ bot: { status: "ready", name: "New bot", message: null } });
  },
  dispatch: async (command) => {
    host.commands.push(command);
    if (command.type === "disconnect" && host.holdDisconnect) return;
    if (command.type === "disconnect")
      host.emit({
        connection: { phase: "disconnected" },
        capture: {
          ...current.capture,
          phase: "stopped",
          levelDbfs: null,
          outputLevelDbfs: null,
        },
      });
    else if (command.type === "select-source")
      host.emit({
        selection: { ...current.selection, sourceId: command.sourceId },
      });
    else if (command.type === "select-destination") {
      if (host.rejectDestination)
        throw {
          code: "permission-denied",
          message: "Room change failed. Check permissions.",
        };
      host.emit({
        selection: {
          ...current.selection,
          serverId: command.serverId,
          channelId: command.channelId,
        },
      });
    } else if (command.type === "connect")
      host.emit({
        sessionId: current.sessionId + 1,
        connection: { phase: "connecting", stage: "gateway" },
      });
    else if (command.type === "set-muted")
      host.emit({
        output: { ...current.output, muted: command.muted },
        capture: {
          ...current.capture,
          outputLevelDbfs: command.muted ? -96 : null,
        },
      });
    else if (command.type === "set-volume")
      host.emit({
        output: { ...current.output, volumePercent: command.percent },
      });
  },
};
const root = document.getElementById("root");
if (root)
  createRoot(root).render(
    <MantineProvider theme={theme} forceColorScheme="dark">
      <Profiler
        id="control-surface"
        onRender={() => {
          host.commits++;
        }}
      >
        <App adapter={new ControlAdapter(transport)} />
      </Profiler>
    </MantineProvider>,
  );
