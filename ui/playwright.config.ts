import { fileURLToPath } from "node:url";
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  outputDir: fileURLToPath(
    new URL("../.trash/ec024/test-results/", import.meta.url),
  ),
  fullyParallel: true,
  forbidOnly: true,
  retries: 0,
  workers: 2,
  reporter: "list",
  use: {
    baseURL: "http://127.0.0.1:4184",
    browserName: "chromium",
    viewport: { width: 440, height: 640 },
    colorScheme: "dark",
    reducedMotion: "reduce",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: [
    {
      command: "npm run dev -- --port 4184 --strictPort",
      url: "http://127.0.0.1:4184",
      reuseExistingServer: false,
      timeout: 30_000,
    },
    {
      command:
        "npx --no-install vite preview --host 127.0.0.1 --port 4185 --strictPort",
      url: "http://127.0.0.1:4185",
      reuseExistingServer: false,
      timeout: 30_000,
    },
  ],
});
