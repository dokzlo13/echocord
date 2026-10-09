# Draft control UI study — EC-004

Draft checkpoint for architecture/UI advisory review. The design and control contract are not approved for real app integration. Further UI implementation is stopped pending the requested advisory review and research into UI design and vetted upstream skills. Existing browser checks are evidence for discussion, not final visual acceptance.

## Accepted scope

One explicitly selected Windows recording device, one Discord bot voice channel, volume, mute, connect/disconnect, and bot setup. Rust owns samples and recovery; the frontend only controls and observes the session. No local monitoring, playback controls, mixing, or routing matrix. Source, dependencies, and prototype execution stay in WSL. Real credentials never belong in this prototype or its frontend state.

`prototypes/control-ui/` is an isolated React + TypeScript + Mantine + CSS Modules study with its own pinned manifest and lockfile. A persistent notice says **Mock adapter · no audio or Discord connection**. Device/server/channel names and input levels are fixtures. Bot setup offers fixed demo profiles with no text, password, token, or credential input. Demo settings live only in memory and reset on reload. No real Discord or audio APIs are called.

## Design proposal for integration review

One dark theme, a compact source/destination panel, and one session panel with a prominent connection action, volume, mute, and delivery text. Bot setup is a focus-trapped modal. Demo-only controls remain collapsed under the main utility. There is no navigation shell, dashboard, or browser hosting requirement.

Input level is pre-volume and pre-mute, and is distinct from connection/delivery. A visible level while disconnected does not mean audio is being sent. Silence is valid; it never triggers recovery. A connected session at zero volume says **Volume is zero**; mute says **Muted** while the input meter remains visible. Missing device is unavailable input, not silence. Delivery text describes adapter state, never listener audibility or measured latency.

| Visible state | Connection/capture meaning | Actions and intent |
| --- | --- | --- |
| Disconnected | No session; simulated input may show activity | Choose one source and destination; connect explicitly |
| Connecting | User-started join pending | Disconnect cancels join; mute and volume remain available |
| Streaming | Demo connection established | Disconnect, adjust volume, mute |
| Muted | Connected with output mute set | Unmute; input activity stays independent |
| Recovering | Same session interrupted | Preserve route, volume, and mute; disconnect cancels retry |
| Device missing | Selected recording source unavailable | Wait for that exact device while active; after disconnect, explicitly choose another source |
| Connection error | Permission denial or terminal failure | Show cause and setup action; require user action to rejoin |

Route selection is locked while connecting, connected, or recovering. Changing servers while stopped clears the channel; the user must explicitly choose the next channel. Bot configuration cannot change during a session. A fixed demo bot is ready on fresh launch, but the session always starts disconnected.

Mock connection/recovery timers are cosmetic (1.2 seconds to connect; 1.8 seconds for the simulated network interruption). Screenshot previews hold their state indefinitely. Device-return simulation waits for explicit return of the same fixture. These timings and held fixtures do not implement, measure, or approve production retry/backoff/exhaustion or device-return grace limits.

## Proposed TypeScript contract — not final Rust bindings

The proposal lives in `prototypes/control-ui/src/contract.ts`. It must be reconciled with the Rust owner before choosing binding generation or declaring IPC type safety. The mock-only controls and bot demo profiles are deliberately outside the product command union.

| Shape | Meaning |
| --- | --- |
| `ControlSnapshot` | Immutable cached snapshot with monotonically increasing `revision`; source/server catalogs, selected IDs, sanitized bot status, connection, capture, output |
| `Connection` | Discriminated union: `disconnected`, `connecting`, `connected`, `recovering` with reason/attempt, `error` with typed code/message |
| `capture` | Separate `ready`/`missing-device` health and nullable scalar `levelDbfs`; no sample buffers |
| `output` | Independent `volumePercent` (0–100) and `muted`; preserved across recovery/disconnect |
| `ControlAdapter` | Cached `getSnapshot`, `subscribe` returning unsubscribe, asynchronous `dispatch` returning `CommandResult` |
| `CommandResult` | Command acceptance or rejected-control message; acceptance is not completed connection/delivery |

Product command proposal:

```ts
type ControlCommand =
  | { type: 'select-source'; sourceId: string }
  | { type: 'select-destination'; serverId: string; channelId: string | null }
  | { type: 'connect' }
  | { type: 'disconnect' }
  | { type: 'set-volume'; percent: number }
  | { type: 'set-muted'; muted: boolean };
```

Commands reject invalid IDs, a channel in another server, unavailable devices, active-session route changes, unconfigured bots, and out-of-range/non-finite volume. Disconnect invalidates pending mock joins/retries. Connection events remain distinct from control errors. The frontend uses React's `useSyncExternalStore` subscription and derives display/delivery from these independent fields.

Before live integration: agree identifier/serialization conventions, an initial snapshot plus ordered event subscription with stale-event handling, command error codes and correlation, terminal removal/permission behavior, authoritative retry exhaustion/device grace, and snapshot ownership after reconnect. This prototype has no cross-process races and does not validate these issues. Real bot authorization and OS-backed credential handling require a separately designed Rust-owned interface; no token field is proposed here.

## Validation and limits

Strict TypeScript covers source, test code, and configuration. Biome checks formatting, imports, and recommended lint/accessibility rules. Production Vite build and Playwright Chromium checks exercise route selection/locking, separate input/delivery, mute/volume, silence, cancellation during join/recovery, same-device return, permission errors without automatic rejoin, fixed bot setup, fresh-launch behavior, subscriptions, invalid commands, keyboard order/focus/modal escape, and absence of outbound service requests/runtime errors.

Screenshots cover all seven states at 720×760 CSS pixels, compact layouts at 640×720 (DPR 1.25), 480×640 (DPR 1.5), and 360×640 (DPR 2), plus bot setup and 200% CSS zoom. Tests assert no horizontal overflow and reachable controls; small viewports use vertical scrolling. Device-scale factors change raster density, while viewport reduction and CSS zoom exercise layout constraints. They do **not** prove actual Windows display scaling, WebView2, Tauri tray/window behavior, audio isolation, recovery timing, or Discord delivery. Raw evidence stays in ignored `.trash/`; task evidence records the sanitized result. No stable screenshot baselines or latency claims are used.

Independent parent review and a Windows desktop smoke remain before final acceptance. Integration must remove or clearly fence demo tools and replace fixtures without exposing secrets or pushing samples through frontend IPC.

## Sources consulted

Official documentation and Context7 queries informed the implementation; no third-party skills or UI templates were imported. Dependencies are pinned in the prototype manifest/lockfile. Upstream skill vetting remains EC-007.

- [Mantine with Vite](https://mantine.dev/guides/vite/): package stylesheet and provider setup.
- [Mantine Modal](https://mantine.dev/core/modal/): modal labels, focus trapping, Escape, and focus return.
- [Mantine Slider](https://mantine.dev/core/slider/) and [NativeSelect](https://mantine.dev/core/native-select/): thumb labels, keyboard arrows, and input labels.
- [React useSyncExternalStore](https://react.dev/reference/react/useSyncExternalStore): stable immutable snapshots and subscription cleanup; consulted through Context7.
- [Playwright configuration](https://playwright.dev/docs/test-configuration), [clock](https://playwright.dev/docs/clock), and [emulation](https://playwright.dev/docs/emulation): browser checks, controlled timers, viewport and device-scale testing.
