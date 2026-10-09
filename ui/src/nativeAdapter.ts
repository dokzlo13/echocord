import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ControlAdapter, type Transport } from "./adapter";
import type { ControlSnapshot } from "./generated";

export const nativeTransport: Transport = {
  async listen(receive) {
    if (!isTauri())
      throw {
        code: "internal",
        message: "Open the Echocord desktop app to use these controls.",
      };
    return listen<ControlSnapshot>("echocord://snapshot", (event) =>
      receive(event.payload),
    );
  },
  getSnapshot: () => invoke<ControlSnapshot>("get_snapshot"),
  dispatch: (command) => invoke<void>("dispatch", { command }),
};

export class NativeAdapter extends ControlAdapter {
  constructor() {
    super(nativeTransport);
  }
}
