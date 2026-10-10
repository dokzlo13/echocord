# Echocord desktop UI

React and TypeScript adopt the approved On Air prototype's cards, full-window sheets, styles and combined dB faders. Native HTML controls, local Sofia Sans fonts and Lucide icons replace the earlier component-library surface. Rust-generated `src/generated.ts` is the wire-model authority; Rust owns credentials, discovery, audio, workers and recovery. No sample buffers cross frontend IPC.

Use the project-pinned Node toolchain in WSL. From this directory:

```sh
npm ci
npx --no-install playwright install chromium
npm run dev
```

| Script | Result |
| --- | --- |
| `npm run dev` | Local fixture preview |
| `npm run dev:native` | Frontend for the native Tauri shell |
| `npm run typecheck` / `npm run lint` | Strict types / Biome checks |
| `npm run build` | Typecheck and native production bundle in ignored `dist/` |
| `npm test` | Build, then behavior/adapter/native-SDK/layout checks |
| `npm run check` | Lint, build and tests |

Fixture mode activates only for `DEV && MODE=fixture`; `?fixture=…` selects names in `src/fixtures.ts`. Production cannot enable fixtures through a query string and reports unavailable transport outside Tauri. The isolated browser harness exposes synthetic controls and React Profiler measurements; it never enters the production bundle.

Tests use fixture port 4184 and production-preview port 4185, with a default 440×640 viewport. Coordinate ports with workers. Test artifacts go to ignored `.trash/ec024/test-results/`; preserve selected final evidence outside a worktree before retiring it. Tests use no real credentials or services.

Source edits are immediate and targeted. Capture works before Discord Connect. Outputs support one Discord room and three explicit Windows playback devices; added/restored outputs stay idle. Full-window pickers retain keyboard/focus behavior. Independent meters/counters/history use destination IDs and session generations; scalar bursts do not rerender controls. Source meters are post-gain before source mute; Discord meters observe reader handoff and playback meters observe callback output, without claiming listener receipt. Histories preserve unavailable gaps and pause while hidden.

[UI architecture](../docs/UI.md), [design](../DESIGN.md), [development commands](../docs/DEVELOPMENT.md) and [quality gates](../docs/QUALITY.md) are the current authorities. Browser evidence does not establish native Windows scaling, physical audio, resource use or long-session stability.
