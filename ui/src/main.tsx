import "@fontsource-variable/sofia-sans";
import "@fontsource-variable/sofia-sans-condensed";
import "./styles/base.css";
import "./styles/onair.css";
import "./styles/native.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ControlAdapter } from "./adapter";
import { NativeAdapter } from "./nativeAdapter";

async function mount() {
  const root = document.getElementById("root");
  if (!root) throw Error("Missing application root");
  let adapter: ControlAdapter = new NativeAdapter();
  if (import.meta.env.DEV && import.meta.env.MODE === "fixture") {
    const { FixtureTransport, fixtureNames } = await import("./fixtures");
    const name = new URLSearchParams(location.search).get("fixture");
    const transport = new FixtureTransport(
      fixtureNames.find((x) => x === name) ?? "disconnected",
    );
    transport.startTelemetry();
    adapter = new ControlAdapter(transport);
  }
  createRoot(root).render(
    <StrictMode>
      <App adapter={adapter} />
    </StrictMode>,
  );
}
void mount();
