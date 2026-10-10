import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ControlAdapter, type Transport } from "./adapter";
import type { ControlSnapshot, MixTelemetry } from "./generated";
export const nativeTransport: Transport = {
  async listen(receive) {
    if (!isTauri())
      throw {
        code: "internal",
        message: "Open the Echocord desktop app to use these controls.",
      };
    return listen<ControlSnapshot>("echocord://snapshot", (e) =>
      receive(e.payload),
    );
  },
  listenTelemetry: (receive) =>
    listen<MixTelemetry>("echocord://telemetry", (e) => receive(e.payload)),
  getSnapshot: () => invoke<ControlSnapshot>("get_snapshot"),
  getVisible: () => invoke<boolean>("get_visibility"),
  listenVisibility: (receive) =>
    listen<boolean>("echocord://visibility", (e) => receive(e.payload)),
  dispatch: (command) => invoke<void>("dispatch", { command }),
  configureBot: (token) => invoke<void>("configure_bot", { token }),
  minimize: () => getCurrentWindow().minimize(),
  close: () => getCurrentWindow().close(),
  startDragging: () => getCurrentWindow().startDragging(),
};
export class NativeAdapter extends ControlAdapter {
  constructor() {
    super(nativeTransport);
  }
}
