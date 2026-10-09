# Echocord control UI prototype

**Draft advisory-review material.** Further UI implementation is stopped pending architecture/UI review and upstream skill research. This prototype is not approved to seed real app UI integration; final human visual acceptance remains unfinished.

EC-004 desktop control study: React, TypeScript, Mantine, and CSS Modules. The visible mock adapter uses fixed, in-memory fixtures. It does not capture, monitor, or transmit audio, connect to Discord, accept tokens, or persist app preferences.

Run from this directory in WSL with the Node version in `.nvmrc`:

```sh
npm ci
npx playwright install chromium
npm run dev
```

The dev server binds to loopback. This is a browser preview of a desktop UI, not a hosted product. Browser installation may require Playwright's Linux system dependencies on a fresh WSL distribution.

| Script | Purpose |
| --- | --- |
| `npm run dev` | Local interactive preview |
| `npm run build` | Strict TypeScript check and production Vite build |
| `npm test` | Adapter, browser behavior, keyboard, and layout checks; private screenshots |
| `npm run check` | Biome lint/format check, typecheck, build, and all tests |
| `npm run typecheck` | Strict TypeScript including tests and configuration |
| `npm run format` | Apply Biome formatting/import fixes |

The coordinator can wrap these scripts from the root Makefile without moving this prototype's manifest or dependencies. `npm test` starts and stops its own loopback dev server on port 4174; that port must be free. `build` writes ignored `dist/`. Tests place screenshots, failure traces, and test output under the repository's ignored `.trash/control-ui/` directory. To keep raw console evidence private, redirect it there as well.

The initial demo is disconnected with a fixed ready bot. Use Bot setup to switch among fixed profiles. Source/destination changes require disconnect; a server change requires explicit channel selection. Volume and mute remain available throughout a session. Demo controls provide seven held state previews, silence, device removal/return, and a temporary connection drop. Clearing a preview resets the fixture to disconnected; previews are not real failures or timers.

The mock uses short cosmetic connection/recovery delays. It does not implement production retry budgets, device-return grace periods, tray behavior, Rust IPC, or credential handling. [UI design and proposed control contract](../../docs/UI.md) describes what is accepted, proposed, and measured. Independent review and Windows desktop validation remain required before integration can close the task.
