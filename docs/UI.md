# Desktop control UI

The production UI lives in `ui/`. Its initial surface passed native and browser checks but was rejected visually in user review. The first replacement concepts were also rejected. Independent design work is exploring a compact source-to-output surface with contextual selection panels; no replacement visual direction is approved yet. The details below describe the current alpha, not the target design. The obsolete standalone mock was removed from the working tree.

## Current alpha layout and state behavior

Use a compact, left-aligned route form above a persistent session-control area. The default client is 480×600 logical pixels, with 420×540 as the supported minimum. Recording device and pre-output meter precede server/channel and concise configuration/discovery status. The route area scrolls independently; connection, Discord volume, and Mute output remain visible. Bot settings stay readable during a session. Routing help is available on demand rather than occupying the main controls.

The reviewed token plan uses window `#181818`, surface `#242424`, text `#F4F4F4`, secondary `#BDBDBD`, border `#737373`, and action/focus `#72CBD3`. Accent buttons use dark text. Typography uses locally available Segoe UI Variable, Segoe UI, then system fallbacks, with 14/20 body, 12/16 secondary, and 16/22 semibold title/status. No remote fonts or added UI framework. The design review removed repeated headings, tagline, and tutorial panels to preserve the compact route form and persistent controls. CSS Modules contain the limited custom layout and interaction styling; Mantine supplies controls and the modal.

The frontend does not start capture or connect automatically. Initial backend connection is disconnected and capture stopped. Remembered selections are shown only as supplied by the backend; absent or unavailable selections remain explicit. No source or destination is silently substituted. Choosing another server explicitly clears its channel.

| Independent state | Display and interaction |
| --- | --- |
| Connection | Disconnected, Connecting, Connected, Recovering, Disconnecting, Cleanup incomplete, or Connection failed |
| Capture | Capture stopped, Opening input, Input running, Device missing, or Input failed; scalar level is shown only for running capture |
| Output | Output enabled, Muted, or Volume is zero; this is control state, not evidence of sending or listener audibility |
| Configuration/discovery | Backend-loaded bot identity/status; configuration checking/errors, catalog loading/errors/empty states, and refresh/recheck actions |

Mute never replaces Connected. Input level is pre-volume/pre-mute; changing volume or mute does not alter the meter. Silence remains valid input. The stable button is always named **Mute output**, with `aria-pressed` reflecting the authoritative mute state. Significant connection/capture/output changes have a polite status announcement; meter samples are not live announcements. Volume offers percentage text and keyboard arrows.

Routes and configuration changes are locked during connecting/connected/recovering/disconnecting/cleanup-incomplete. Disconnect remains available during a pending join or recovery. Disconnecting cannot Connect. Cleanup incomplete offers Retry disconnect and prevents a new connection; the shell owns explicit Quit from the tray. Connection, capture, configuration, discovery, and rejected-control errors show cause and next actions independently.

The settings modal starts focus on its always-enabled Close control through Mantine's default focus trap, even when Recheck configuration and Refresh servers are disabled. Escape restores focus to Bot settings. There are no token, path, text, or password fields. After backend-only configuration changes, Recheck configuration and Refresh servers invoke the agreed commands.

## Native adapter and generated model

`ui/src/generated.ts` is coordinator-owned and generated from Rust. It is typechecked with the frontend, excluded from Biome formatting/lint, and unchanged by this worker. There is no competing TypeScript wire model.

- `get_snapshot` takes no arguments and returns generated `ControlSnapshot`.
- `configure_bot` takes `{ token: string }`, validates and stores the secret in Rust, and returns no credential data. Its user-facing setup form is pending the new design.
- `dispatch` takes `{ command: ControlCommand }`, resolves void on acceptance, and rejects with generated `AppError`.
- `echocord://snapshot` carries generated `ControlSnapshot`.

The adapter awaits listener registration before the initial snapshot read. Cached immutable view state works with React `useSyncExternalStore`; snapshots with older/equal revisions are ignored. Lifecycle generations reject callbacks and command errors from disposed listeners or earlier mounts. If listener setup finishes after disposal, its returned listener is immediately removed. Retrying status loading replaces the previous subscription. No `ui_ready` or other shell API is introduced.

Dispatch acceptance does not invent connection, capture, or output completion. Pending commands are indicated separately; failures surface without changing authoritative values. The Rust `AppError` contract supplies sanitized user messages. Unexpected transport exceptions are replaced with a generic status-loading action, without showing raw parser excerpts or private paths.

Volume uses a 120 ms trailing coalescer and at most one value queued behind an in-flight request. The slider shows the pending draft until a snapshot confirms it; rejection restores the backend value. Meter events cannot erase a newer slider intent. Mute and Disconnect bypass the volume queue. Disconnect/disposal cancel unsent volume changes. Rust still validates all commands and owns session cancellation, retry budgets, process-exit confirmation, saved preferences, and real configuration.

## Explicit fixture mode

`npm run dev` selects Vite's development `fixture` mode. Only `DEV && MODE === 'fixture'` dynamically imports the fixture transport. Its notice and state picker are visible, and it has no credential/service/audio APIs. Production `npm run build` emits the native adapter to `ui/dist` and excludes the fixture transport. A query parameter or missing native host cannot activate fixture fallback in production. Opening the production bundle in a browser reports that the desktop app is required.

Fixtures cover stopped/disconnected, first run, loading, empty devices/servers/channels, unavailable saved routes, invalid configuration, discovery failure, connecting, connected/muted, recovery, missing device, capture-open failure, connection failure, disconnecting, incomplete cleanup, rejected commands, and unavailable backend. They verify presentation and native API consumption, not Rust recovery or audible delivery.

## Validation and remaining gates

See [task evidence](../tasks/EC-011.md) for commands and measured results. Automated checks include strict TypeScript, Biome, production build, adapter lifecycle/revision/volume races, browser route/readiness/error behavior, keyboard/modal focus, calculated contrast, supported-size persistent-control assertions, DPR/zoom layouts, and a production-bundle test through the actual Tauri JavaScript SDK with an emulated host. The latter verifies API names, payloads, event handling, and no silent production fallback; it is not a native smoke.

Screenshots and CLI review use isolated Chromium and the pinned Playwright runtime, with all raw evidence under ignored `.trash/`. Browser viewport/DPR/CSS zoom checks do not establish actual Windows display/text scaling, Segoe rendering, Narrator, WebView2, tray behavior, process cleanup, capture, Discord delivery, or latency. The coordinator integrates the native shell/runtime and runs Windows/live checks. This worker does not modify Cargo, Makefile, shell, or generated bindings.

Applied skills are the pinned [frontend-design](../.agents/skills/frontend-design/SKILL.md) and [playwright-cli](../.agents/skills/playwright-cli/SKILL.md), under the applicability/privacy rules in [SOURCES](../.agents/SOURCES.md). Official [Tauri command API](https://v2.tauri.app/reference/javascript/api/namespacecore/) and [event API](https://v2.tauri.app/reference/javascript/api/namespaceevent/), Mantine modal/slider guidance, and Context7 queries informed the implementation. No new or placeholder skills were created.

## Prepared redesign behavior

The adapter now exposes ephemeral bot configuration and explicit route apply. An active route must reach observed Disconnected before selections change; each selection is confirmed by snapshot, and reconnect occurs only when the caller explicitly requested it. Stop/disposal invalidates pending apply work; failed cleanup blocks further steps. Already-confirmed selection changes can persist when a later step fails, so the eventual draft UI must reconcile against authoritative state rather than claim atomic rollback. These operations have fixture tests but are not yet exposed by the rejected alpha layout. Only truthful connection-stage text has been added to that layout.
