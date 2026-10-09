import "@mantine/core/styles.css";
import {
  Button,
  createTheme,
  MantineProvider,
  Modal,
  NativeSelect,
} from "@mantine/core";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ControlAdapter } from "./adapter";
import type { FixtureTransport } from "./fixtures";
import { NativeAdapter } from "./nativeAdapter";
import "./global.css";

const theme = createTheme({
  primaryColor: "cyan",
  primaryShade: 4,
  colors: {
    cyan: [
      "#effafb",
      "#d6f0f2",
      "#bde7ea",
      "#a3dde2",
      "#72cbd3",
      "#65b8bf",
      "#529ba2",
      "#3d7980",
      "#305e64",
      "#24474b",
    ],
    dark: [
      "#f4f4f4",
      "#dedede",
      "#bdbdbd",
      "#969696",
      "#737373",
      "#545454",
      "#343434",
      "#242424",
      "#181818",
      "#101010",
    ],
  },
  fontFamily: '"Segoe UI Variable", "Segoe UI", system-ui, sans-serif',
  fontSizes: { xs: "0.75rem", sm: "0.875rem", md: "0.875rem" },
  headings: { fontWeight: "600" },
  defaultRadius: 6,
  focusRing: "always",
  cursorType: "pointer",
  components: {
    Button: Button.extend({ defaultProps: { size: "sm", autoContrast: true } }),
    NativeSelect: NativeSelect.extend({ defaultProps: { size: "sm" } }),
    Modal: Modal.extend({ defaultProps: { transitionProps: { duration: 0 } } }),
  },
});

async function mount() {
  const root = document.getElementById("root");
  if (!root) throw new Error("Missing application root");
  let adapter: ControlAdapter = new NativeAdapter();
  let fixtures: FixtureTransport | undefined;
  if (import.meta.env.DEV && import.meta.env.MODE === "fixture") {
    const { FixtureTransport, fixtureNames } = await import("./fixtures");
    const name = new URLSearchParams(location.search).get("fixture");
    fixtures = new FixtureTransport(
      fixtureNames.find((item) => item === name) ?? "disconnected",
    );
    adapter = new ControlAdapter(fixtures);
  }
  createRoot(root).render(
    <StrictMode>
      <MantineProvider theme={theme} forceColorScheme="dark">
        <App adapter={adapter} fixtures={fixtures} />
      </MantineProvider>
    </StrictMode>,
  );
}
void mount();
