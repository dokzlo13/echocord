# Echocord UI

React/TypeScript provides compact On Air cards, full-window sheets, setup guidance and measured meters. Rust owns credentials, discovery, audio and recovery. No sample buffers cross frontend IPC. Native controls, local Sofia Sans fonts and Lucide icons are used throughout.

From this directory, with the pinned Node version:

```sh
npm ci
npx --no-install playwright install chromium
npm run dev
```

| Script | Purpose |
| --- | --- |
| `npm run dev` | Fixture UI preview |
| `npm run dev:native` | Frontend for native-shell development |
| `npm run lint` / `npm run typecheck` | Biome / strict types |
| `npm run build` | Production frontend bundle |
| `npm test` | Build and browser/adapter tests |
| `npm run check` | Lint, build and tests |

Tests default to fixture port4184 and production-preview port4185. Set `ECHOCORD_TEST_PORT` and `ECHOCORD_PREVIEW_PORT` for isolated parallel checkouts. Artifacts go to the root `artifacts/test-results/` directory. No bot credentials or real Discord services are used.

`src/generated.ts` is generated from the Rust control model; do not hand-edit it. `nativeAdapter.ts` is production transport, and `fixtures.ts` is test/preview only. Package version is injected by Vite for the Settings display. Setup links use an exact native allowlist; adding a new external destination requires updating and testing that allowlist.

Read [DESIGN.md](../DESIGN.md), [development](../docs/DEVELOPMENT.md) and [testing](../docs/TESTING.md) before changes. UI behavior tests do not establish Windows audio or native accessibility behavior.
