# Echocord desktop UI

React, TypeScript, Mantine, and CSS Modules for the native Tauri app. Rust-generated `src/generated.ts` is the only wire-model definition. Rust owns configuration, discovery, capture, recovery, and output. The frontend carries no audio samples. Its credential setup transport sends an ephemeral token to Rust; secrets are absent from snapshots and view state. The entry form awaits design acceptance.

Run from this directory in WSL with the Node version in `.nvmrc`:

```sh
npm ci
npx playwright install chromium
npm run dev
```

`dev` explicitly starts an in-memory **fixture preview**, visibly identified as having no audio or Discord connection. It never contacts services. Framework versions match the reviewed prototype; the Tauri JavaScript API is pinned separately. Browser installation can require Playwright's Linux system dependencies on a fresh WSL distribution.

| Script | Result |
| --- | --- |
| `npm run dev` | Loopback fixture preview, Vite mode `fixture` |
| `npm run dev:native` | Native preview for the Tauri shell; requires the native host |
| `npm run build` | Strict TypeScript and production native bundle in ignored `dist/` |
| `npm test` | Build the native bundle, then adapter/fixture/production-host browser checks |
| `npm run check` | Biome lint/format checks, strict TypeScript, native build, and tests |
| `npm run format` | Apply Biome formatting/import fixes without touching generated types |

Production builds cannot activate fixtures through environment flags or query parameters. Outside Tauri, production shows an explicit unavailable state with disabled session actions. Tests serve fixture development mode on port 4175 and the production bundle on 4176, both loopback; these ports must be free. All screenshots, failure traces, and test output go to ignored `.trash/ec011/`; redirect raw console logs there too. Browser tests never use credentials or real services.

The shell loads `ui/dist`. The native adapter listens to `echocord://snapshot` before invoking `get_snapshot`, then dispatches `dispatch` with `{ command }`. Snapshots remain authoritative, stale revisions are discarded, and pending volume is coalesced. Credential setup uses the separate `configure_bot` command. The adapter also supports explicit route apply with observed disconnect and selection confirmation; the redesigned controls that expose it are pending. The coordinator owns shell capabilities, generated-source drift checks, close-to-tray/Quit behavior, Rust integration, and native verification.

See [UI.md](../docs/UI.md) for design and adapter behavior and [EC-011](../tasks/EC-011.md) for measured validation and remaining gates.
