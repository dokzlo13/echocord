import "@mantine/core/styles.css";
import { createTheme, MantineProvider } from "@mantine/core";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { MockControlAdapter } from "./mockAdapter";
import "./global.css";

const theme = createTheme({
  primaryColor: "teal",
  primaryShade: 4,
  fontFamily: '"Segoe UI", system-ui, sans-serif',
  fontSizes: { xs: "0.75rem", sm: "0.8125rem", md: "0.875rem" },
  defaultRadius: "md",
  cursorType: "pointer",
  focusRing: "always",
  colors: {
    dark: [
      "#edf1f5",
      "#c9d1da",
      "#a3aeba",
      "#6d7a8a",
      "#45505e",
      "#333d49",
      "#29313c",
      "#202731",
      "#191f28",
      "#131922",
    ],
    teal: [
      "#e9fcf5",
      "#c9f7e7",
      "#a4efd7",
      "#7ee5c5",
      "#61d6b3",
      "#3cc79e",
      "#21af86",
      "#138d6b",
      "#0b7055",
      "#085c46",
    ],
  },
  components: {
    Button: { defaultProps: { size: "sm" } },
    NativeSelect: { defaultProps: { size: "sm" } },
  },
});

const adapter = new MockControlAdapter();
const root = document.getElementById("root");
if (!root) throw new Error("Missing application root");

createRoot(root).render(
  <StrictMode>
    <MantineProvider theme={theme} forceColorScheme="dark">
      <App adapter={adapter} />
    </MantineProvider>
  </StrictMode>,
);
