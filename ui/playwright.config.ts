import { fileURLToPath } from "node:url";
import { defineConfig } from "@playwright/test";

const testPort = Number(process.env.ECHOCORD_TEST_PORT ?? 4184);
const previewPort = Number(process.env.ECHOCORD_PREVIEW_PORT ?? 4185);

export default defineConfig({
  testDir: "./tests",
  outputDir: fileURLToPath(
    new URL("../artifacts/test-results/", import.meta.url),
  ),
  fullyParallel: true,
  forbidOnly: true,
  retries: 0,
  workers: 2,
  reporter: "list",
  use: {
    baseURL: `http://127.0.0.1:${testPort}`,
    browserName: "chromium",
    viewport: { width: 440, height: 640 },
    colorScheme: "dark",
    reducedMotion: "reduce",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: [
    {
      command: `npm run dev -- --port ${testPort} --strictPort`,
      url: `http://127.0.0.1:${testPort}`,
      reuseExistingServer: false,
      timeout: 30_000,
    },
    {
      command: `npx --no-install vite preview --host 127.0.0.1 --port ${previewPort} --strictPort`,
      url: `http://127.0.0.1:${previewPort}`,
      reuseExistingServer: false,
      timeout: 30_000,
    },
  ],
});
