import "@mantine/core/styles.css";
import { MantineProvider } from "@mantine/core";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ControlAdapter } from "./adapter";
import { NativeAdapter } from "./nativeAdapter";
import { theme } from "./theme";
import "./global.css";

async function mount() {
  const root = document.getElementById("root");
  if (!root) throw new Error("Missing application root");
  let adapter: ControlAdapter = new NativeAdapter();
  if (import.meta.env.DEV && import.meta.env.MODE === "fixture") {
    const { FixtureTransport, fixtureNames } = await import("./fixtures");
    const name = new URLSearchParams(location.search).get("fixture");
    const fixtures = new FixtureTransport(
      fixtureNames.find((item) => item === name) ?? "disconnected",
    );
    adapter = new ControlAdapter(fixtures);
  }
  createRoot(root).render(
    <StrictMode>
      <MantineProvider theme={theme} forceColorScheme="dark">
        <App adapter={adapter} />
      </MantineProvider>
    </StrictMode>,
  );
}
void mount();
