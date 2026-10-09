# Echocord desktop UI

React, TypeScript, Mantine and CSS Modules implement the accepted Signal Bridge surface in the native Tauri app. Rust-generated `src/generated.ts` is the wire-model authority. Rust owns configuration, encrypted credentials, discovery, audio and recovery; the frontend carries no sample buffers.

Run in WSL using the Node version in `.nvmrc`:

```sh
npm ci
npx playwright install chromium
npm run dev
```

`dev` uses an in-memory fixture transport and never starts real audio or connects to Discord. Select a development state with `?fixture=connected` (or a name from `src/fixtures.ts`). The main UI has no development controls. Production cannot activate fixtures through flags or query parameters; outside Tauri, it shows an explicit unavailable state.

| Script | Result |
| --- | --- |
| `npm run dev` | Loopback fixture preview |
| `npm run dev:native` | Frontend preview for the native Tauri shell |
| `npm run build` | Strict TypeScript and production native bundle in ignored `dist/` |
| `npm test` | Build, then adapter, fixture, production-host and rendering checks |
| `npm run check` | Biome, strict TypeScript, production build and tests |

Tests serve development fixtures on 4175 and the production bundle on 4176. Coordinate those ports with other workers. Raw browser output goes to ignored `.trash/ec011/`. Final review screenshots belong in the coordinator's ignored evidence directory before worktree cleanup. Tests use synthetic credentials and no real services.

The surface separates recording input, shared mix and Discord destination. Pickers stage route changes; Apply selection saves without connecting, while Apply & reconnect stops the active session, confirms each change and starts the new route. Failure/cancellation can preserve already-confirmed changes, with only the remaining draft queued. Cleanup failure blocks further route application until stop succeeds. Mute and stop/cancel remain available below nonmodal selection/setup drawers. Ctrl+K opens the room picker; Escape closes a drawer and returns focus.

Bot settings provide one Test & Save operation through `configureBot`. The token is ephemeral, clears on success/close, and is absent from control snapshots and command history. Failed saves never claim rollback. First-run has a direct Set up bot action, then explicit input/room selection; nothing auto-connects or chooses a fallback.

App subscribes only to stable controls. `Telemetry.tsx` alone subscribes to the adapter's independent `{sessionId,sourceId,capture,visible}` stream. Event-driven DOM/ref meters cap painting at 20Hz, queue text at 5Hz and detailed counters at 4Hz; one pending timer per subscriber and no idle animation loop. Document hiding or native visibility false cancels work; resume reads the latest observation. Input peak is pre-gain; mix peak is Rust's actual post-gain/mute reader measurement before Discord. Null means no reading and the native -96dBFS floor means silent; quieter values below the displayed -60dBFS bar remain numeric. No listener latency or delivery is inferred.

Capture queue duration uses the known 48 kHz engine PCM boundary; its occupied/total frame fraction and bar express occupancy of the 1,920-frame queue. Details describe its 40ms capacity and local frame counters. The main surface fits 480×600 and 420×540 including pending/error bands; compact crowded states compress secondary copy and calibration ticks, while drawer lists may scroll.

The browser-only `tests/harness.html`/`harness.tsx` wraps the real App in React Profiler and exposes synthetic transport controls for burst, visibility and partial-failure checks. It is never imported by the production entry. Browser checks cover user behavior and geometry, including containment/overlap; they do not establish Windows WebView scaling, live audio, actual FPS/CPU, tray behavior or long-session stability.

See [DESIGN.md](../DESIGN.md) for the final visual contract and [QUALITY.md](../docs/QUALITY.md) for native/audio acceptance. The coordinator owns shell capabilities, native integration and final Windows smoke.
