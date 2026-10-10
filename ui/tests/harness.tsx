/* Browser-only test entry. Never imported by the production bundle. */
import "@fontsource-variable/sofia-sans";
import "@fontsource-variable/sofia-sans-condensed";
import "../src/styles/base.css";
import "../src/styles/onair.css";
import "../src/styles/native.css";
import { Profiler } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../src/App";
import { ControlAdapter } from "../src/adapter";
import { FixtureTransport, fixtureNames } from "../src/fixtures";

const name = new URLSearchParams(location.search).get("fixture");
const transport = new FixtureTransport(
  fixtureNames.find((x) => x === name) ?? "connected",
);
const adapter = new ControlAdapter(transport);
export interface Harness {
  transport: FixtureTransport;
  adapter: ControlAdapter;
  commits: number;
}
declare global {
  interface Window {
    uiHarness: Harness;
  }
}
const host: Harness = { transport, adapter, commits: 0 };
window.uiHarness = host;
const root = document.getElementById("root");
if (root)
  createRoot(root).render(
    <Profiler id="controls" onRender={() => host.commits++}>
      <App adapter={adapter} />
    </Profiler>,
  );
