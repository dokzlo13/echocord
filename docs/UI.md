# Desktop control UI

The production React/Mantine surface follows [DESIGN.md](../DESIGN.md). Audio source, shared mix and Discord destination have separate spatial regions; source and room selection use compact contextual popovers. A single selected application or recording input feeds one Discord destination. Slim input/post-gain meters and a bounded capture-queue readout sit on the main surface. A persistent transport exposes mute and connect/cancel/stop.

## Selection, setup and errors

Picker choices create a local draft. Apply selection while disconnected saves the route; Apply & reconnect explicitly stops an active session, waits for observed cleanup, confirms selection steps and starts the requested route. Confirmed endpoint identity remains authoritative. Cancellation/partial failure can leave already-confirmed selections saved; the remaining draft is reconciled rather than presented as an atomic rollback. No default microphone, room or implicit connect fallback is chosen.

Applications and Recording inputs have separate searchable tabs. Source refresh uses its own native status/command, works while streaming or without a configured/reachable bot, and keeps cached rows visible on error. Application details distinguish process instances; unavailable/current markers survive exit, and restarted applications require explicit reselection. Main labels and error copy reflect the confirmed source kind. Browser overlap and original local playback are stated once in the picker; no independent local mute is shown. Same-name queued applications expose their instance details without parsing IDs.

Grouped room search uses server/category context and actual catalog icons with an initials fallback. Long identities remain available in the pickers. Popovers use content-sized height with bounded width/height, list-only scrolling, search autofocus and Escape/close focus return. Transport stays accessible, including a direct outside Mute/Stop click. Main states, including pending/errors, fit the checked 480x600 and 420x540 viewports without internal/document scrolling. Labelled queued-change rows distinguish confirmed and proposed route fields. Bot setup and complete error/details content retain secondary sheets.

First-run offers bot setup. Test & Save sends an ephemeral password-field value through a private command; Rust validates before current-user DPAPI persistence. The field clears on success or close. Errors never echo arbitrary secret-bearing exceptions or promise rollback after an unconfirmed response. Bot replacement is blocked during an active/pending session. Closing the app window keeps the session in the tray; Quit ends the application.

## Control and telemetry boundary

Rust model types generate `ui/src/generated.ts`. The frontend sends intentions and renders authoritative snapshots; it owns no audio buffers, retry budgets or process lifetimes.

- `get_snapshot` returns ControlSnapshot; `echocord://snapshot` delivers newer revisions.
- `dispatch` accepts `{ command }` for typed control intentions.
- `configure_bot` accepts `{ token }`, validates/saves in Rust and returns no credential data.
- `get_visibility` and `echocord://visibility` report native shown/non-minimized state.

The adapter preserves a cached control ViewState across telemetry-only revisions while retaining the latest full snapshot for ordering and command confirmation. Capture phase/message changes still notify controls. A separate cached telemetry subscription carries current capture scalars and visibility; restart clears old-generation observations before fresh data arrives. Event/read races and late disposal callbacks are guarded.

Meters and diagnostics use DOM refs, not React state per observation. Visible meter updates are capped at 20Hz, queue text at 5Hz and counters at 4Hz, with no idle animation loop. Meter bars interpolate through CSS (80ms attack, 180ms release); numerical and semantic readings stay exact. Document/native hidden states cancel pending presentation work and transitions; return snaps to the latest observation. Source/session changes, unavailable levels and reduced motion also snap. A browser Profiler regression verifies that telemetry bursts do not commit the control surface, while ordinary controls still do.

Input peak is pre-gain. Mix peak is actual completed PCM reader handoff after gain/mute, before Discord, with bounded freshness. Neither measures received loudness or listener delay. Queue milliseconds represent buffered frames at the known engine rate, not total latency. Quiet readings below the visual scale retain their numeric value; only the native silence floor is labelled Silent.

## Validation and scope

EC-013/018/019/021/022 record measured checks and limitations. Browser coverage includes keyboard/focus, setup, drafts/cancel/partial failure, native SDK wiring, source kinds/refresh/restarts, compact containment and live resizing, text/DPR cases and telemetry rendering. Coordinator native smoke checks actual frontend IPC, independent local discovery, encrypted bot discovery when configured, window visibility and tray lifecycle. Native text scaling/accessibility, real FPS/CPU under a live session, application-to-Discord listening, listener latency and five-hour stability remain separate acceptance.

Development fixture mode is explicit and excluded from production builds. Browser tests use synthetic catalogs and measurements; no live credentials or services. Raw screenshots and test artifacts stay private, with redundant files removed at closeout. Vetted skill provenance remains in .agents/SOURCES.md; prototype research is historical evidence, not current visual authority.
