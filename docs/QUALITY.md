# Quality gates

## Initial scaffolding gate (historical)

Verify WSL builds a Windows executable, then verify the actual native audio dependencies and Tauri shell. Record commands, versions, target, outputs, and failures. A successful generic executable does not establish Songbird or Tauri compatibility.

Documentation validation is separate from future application gates. Do not add green CI placeholders that skip missing manifests. Scaffold real checks with the code they validate.

## Current executable gates

`make check` runs production core formatting/Clippy/tests, generated-contract drift checks, UI lint/type/build/browser checks, and Windows application Clippy. `make app-smoke` rebuilds and verifies native UI/IPC/tray without sending Connect or joining voice; normal startup restoration may reopen saved recording inputs. `make engine-smoke` checks actual worker startup with synthetic empty credentials. Retired dependency probes and draft prototypes are no longer active gates. Live delivery and five-hour stability require the explicit acceptance steps below.

## Automated gates

- Rust formatting and Clippy with warnings denied on supported targets; meaningful unit tests for buffer accounting, conversion, and recovery transitions.
- Windows-target compilation of the actual adapters. WSL/Linux unit tests cover portable logic, not WASAPI integration.
- TypeScript strict mode, Biome lint, production frontend build and meaningful tests for controls/failure states.
- One source of truth for Rust/TypeScript command/event shapes; prove binding generation or schema checking before claiming compile-time IPC safety.
- Commit lockfiles and pin the validated Rust/tool versions. Review dependency features and avoid unnecessary playback/decoder/network-fetch features.
- Independent review and applicable combined checks before closing implementation tasks.

UI tests should exercise user behavior with a mock control-plane adapter. Browser visual tests do not prove tray or WebView behavior; run a Windows desktop smoke as well. Tauri documents [mock and desktop tests](https://v2.tauri.app/develop/tests/) and [WebDriver support](https://v2.tauri.app/develop/tests/webdriver/).

## Visual acceptance

The owner-approved On Air Stack is the current authority in [DESIGN.md](../DESIGN.md). Adopt its actual components/styles; do not reopen design selection or prefer a replacement component-library theme. Keep local licensed fonts/icons, native controls and the compact custom title bar. No prototype navigation, simulated source activity or fictional network/profile values ship.

Review first-run, restored preflight, capturing before Connect, joining/cancel/recovering, live/muted, source faults, operation errors, cleanup-incomplete and live removal. Test multiple sources, one Discord destination and up to three playback destinations, targeted edits with no global Apply, full-window picker/settings/bot/connection sheets, independent list scrolling and truthful unavailable metrics.

Use 440×640 (default), 455×455, 420×420 (minimum), 760×440 and 1000×640, with Simple and History cases where relevant. Preserve whole-card actions and separate list scrolling without document overflow; include compact faults/operation errors and long/Cyrillic identities. Check source colour numbering, non-colour state cues, dB arrows/Page/Home/End, visible focus, picker selection, sheet trapping/Escape/return and reduced motion. Compare a bounded screenshot set against the accepted reference, batch corrections, and retain valid final captures privately.

Browser/SDK mocks do not prove native window behavior. Check actual drag, resize, minimize, close-to-tray, disabled-tray Close, reopen/Quit and safe remembered size at Windows 100%, 125% and 150% display scaling. The integrated epoch has measured geometry/tray/visibility and tray-disabled exit at 125% (DPI 120), including logical 420×420 and 760×440 plus size restoration. This does not close 100%/150%, native screen-reader/text-scaling or live-resource acceptance. Final native evidence is recorded in EC-026/STATE.

A typecheck cannot certify the layout, and display scaling is distinct from browser DPR or Windows text scaling. Avoid brittle meter-value screenshots. Keep raw captures, profiles and host settings ignored; publish sanitized outcomes. Applicable vetted skills and their licenses/pins are in [.agents/SOURCES.md](../.agents/SOURCES.md).

## Audio acceptance experiment

### Application-capture gate

For capture changes, preserve EC-017's reviewed process identity, activation pointer lifetime, COM ownership, packet release, cancellation during opening/running, and terminal cleanup behavior. Native tests use only owned synthetic sources and no Discord credentials: distinguish selected root/children from unrelated audio; verify stereo, silence/resume, supported render rates, stale identity/source exit, bounded queue behavior and cancellation. Check browser scope separately with an isolated test profile when available. A silent stream is valid input; an exited process never triggers capture of a replacement PID or a broader source.

Local application/session mute effects and original playback audibility are separate evidence. Do not infer independent local suppression from successful process capture. Endpoint changes, protected/exclusive audio, sleep/resume, audible Discord delivery and long-session stability remain explicit acceptance boundaries unless actually exercised. Browser tests cover source-kind selection, refresh without credentials/network, immediate add/replace/start/stop/mute/gain, safe restore/reselection, unavailable sources, full-window pickers, rejected operations, focus/keyboard and control/telemetry isolation. Native shell checks still cover WebView/tray behavior.

### Shared-mix and lifecycle gate

Preserve the persistent tokenless audio worker, disposable Discord worker and independent disposable playback workers, parent/frontend PCM exclusion, bounded shared-memory reader and narrow inherited-handle ownership. Portable tests must cover precise dB gain, source/destination mute, unity summation/clamping and overload telemetry, overlap/capacity reservations, stale-result fencing, migration/restore and worker failure/cleanup. Native tests require owned unrelated positive controls, multiple application sources, silence/resume, per-source stop/replace/exit while the stable reader survives, independent output controls and observed shutdown. Capture without credentials or a destination must drain normally; source changes must not request bot leave.

Measure the producer cursor against real elapsed time after warm-up, rather than a reader's synthesized silence. Short native two-source measurements are recorded in EC-023; they do not prove multihour clock alignment or live Discord delivery. There is no drift resampler or automatic normalization. Mix overload must be observed before clamp and remain visible after destination attenuation. A cleanup containment failure may stop the audio worker; never claim every fatal native failure affects only one source.

Prove destination Cancel/Disconnect during startup/recovery, cleanup-incomplete blocking, repeated idempotent Capture/selection and rejected-worker operations without phantom cards. Room changes must disclose and follow actual reconnect stages, including same-server changes. No default-source or ambiguous restored-app attachment is allowed.

### Playback fan-out gate

Prove explicit opaque endpoint identity, duplicate/type/capacity limits, supported 48 kHz stereo formats and no default-device fallback. Adding/restoring an output must remain silent and idle. Playback discovery/control must work without bot configuration and while Discord is connecting/live. Callback work is nonblocking and bounded; independent reader clocks must not stall the producer or other outputs. Validate per-output gain/mute, startup after a real callback, session-fenced measurements, stop/remove/change reservations through observed exit, fault/cleanup containment and aggregate tray actions.

Native signal tests use an explicitly verified unused endpoint and owned output processes, with no change to device defaults, endpoint/session volume or owner routes. A process-loopback observer may capture only the owned output worker to check actual submitted signal. Record read-only availability/default-role/session checks privately before rendering. Retain independent reader/slow-consumer tests, silence, stale-producer behavior and observed worker exit. The callback meter and local queue counters do not prove physical audibility or end-to-end latency.

Browser checks cover bot-free Playback selection, restored idle output, independent controls, four-card fit, type-change guards, one output's restart without resetting other histories, unavailable devices and concrete routing notes. Compare wide two-source expansion and compact measured sparklines to the actual approved prototype; completed-change notices must follow observed identity changes. Test output lifecycles at the native shell boundary as well as mocked IPC where feasible, preserving an active owner session.

### Multiple-room increment gate

Before accepting the active EC-030 increment, prove one shared credential-bearing gateway and independently disposable per-room voice workers, with no PCM in the parent/frontend and no private voice credentials in public contracts, Debug output or diagnostics. Keep one selected/retiring room per server, four total outputs and the existing playback limit. Versioned migration must preserve earlier setup and restore every destination idle.

Exercise both voice handshake orders, cancellation before/after join transmission and driver startup, late state/server events, unexpected moves/removal, stale gateway epochs, worker generations and credential revisions, and leave receipt before/after local worker exit. A late owed non-null state must invalidate an earlier null receipt; ambiguous cleanup must retain the reservation. Gateway failure must stop its Discord cohort without stopping capture or playback. Cancelling or changing one room must not reset another room's reader, history or controls. Token changes must remain blocked by any gateway/voice/startup/retiring ownership, including faulted cards.

Browser gates cover several room cards, independent operations/faults/meters, same-server selection conflicts, retiring old-server reservations, an open picker's changing capacity, explicit idle restoration, shared gateway status and playback during Discord work. Preserve the approved five-size geometry and keyboard/focus behavior. Native gates cover the actual process/protocol boundary and Windows build/shell. Live listening in different servers and fault isolation during real delivery require a coordinated acceptance run; mock state or local submitted-sample meters do not establish listener receipt.

### Pipeline and session targets

Proposed targets, to calibrate against the first working prototype:

| Measure | Initial target | Boundary and method |
| --- | --- | --- |
| Local pipeline delay | p95 ≤60 ms | Timestamp samples when capture exposes them; observe age at handoff for Discord transmission. Do not label this end-to-end latency. Account for hidden library buffering. |
| Listener delay | target ≤300 ms; investigate repeated >500 ms | Record a known marker from source and an independent receiving Discord client on a common recording clock where possible. Document network/client/device conditions and measurement uncertainty. |
| Mute response | ≤100 ms to stop submitting audible frames | Timestamp the command and last submitted non-silent frame; received playback can lag. |
| Session stability | five hours | Normal virtual-tabletop/browser/Discord load; no crash, manual restart, unexplained stoppage, progressive delay, or audible artifacts attributable to the app. |

Measure queue occupancy/age, underruns, overruns, stale-frame drops, reconnect attempts, CPU, and memory including WebView processes. Keep queues bounded. Sample clocks can drift even with one capture device against a network sender; investigate clock correction before increasing buffers.

Run silence/resume, stereo and supported-format checks. Application fixtures may render at 44.1/48 kHz while process capture supplies 48 kHz stereo. Recording inputs currently require 48 kHz mono/stereo; unsupported recording formats must be rejected rather than silently converted or replaced. Keep a listener observation log and correlate problems with diagnostics. Do not promise zero network loss or detect every audible issue automatically.

Separate the stable five-hour run from deliberate network/device/permission failure tests. Verify retry exhaustion, preserving mute, explicit disconnect during recovery, device return, sleep/resume, tray reopening, and clean Quit. A silent input must not trigger reconnects.

Resources are a practical guardrail: no unexplained sustained load, unbounded queues, or continuing memory growth after warm-up. Compare experience with Kenku informally if useful; there is no exact Kenku superiority gate.

## Live visualization budget

Audio callbacks and PCM stay in Rust. Scalar telemetry is approximately 10 Hz and has its own native event; full control snapshots publish only when retained control state changes. React controls, catalogs and settings must stay stable during scalar bursts. Use lightweight DOM/CSS for meters and bounded canvas histories/plots, with no chart/WebGL dependency required.

The implementation caps meter paints at 20 Hz, level history at 10 Hz and network readouts/plots at 2 Hz. Stop scheduled visual work for both document hiding and native minimize/tray hiding; verify the real Tauri/WebView visibility path as well as browser behavior. On return, refresh scalars and clear/gap stale history rather than showing old samples as recent. Reset destination-session history, bound rings and pending fader work, preserve reduced motion, and keep commands responsive. Profiler/burst, visibility and lifecycle regressions must test these boundaries.

Source levels are native post-gain peaks before source mute, so muted activity continues visibly with hatching. Pre-gain source clipping and summed mix overload are separate observations. Destination levels must be actual post-volume/mute native handoff, idle when not sending; they do not confirm listener receipt. Queue occupancy in frames or milliseconds at 48 kHz is local buffered audio, not total latency. Gateway RTT must be measured, nullable and labelled gateway; it is not voice RTT. Do not fabricate listener counts, permissions, encryption negotiation, latency, spectrum or historical values. The five-hour pipeline/resource gate remains separate acceptance.
