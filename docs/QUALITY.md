# Quality gates

## Before application scaffolding

Verify WSL builds a Windows executable, then verify the actual native audio dependencies and Tauri shell. Record commands, versions, target, outputs, and failures. A successful generic executable does not establish Songbird or Tauri compatibility.

Documentation validation is separate from future application gates. Do not add green CI placeholders that skip missing manifests. Scaffold real checks with the code they validate.

## Current executable gates

`make check` runs production core formatting/Clippy/tests, generated-contract drift checks, UI lint/type/build/browser checks, and Windows application Clippy. `make app-smoke` rebuilds and verifies the native UI without joining voice. `make engine-smoke` checks actual worker startup with synthetic empty credentials. Retired dependency probes and draft prototypes are no longer active gates. Live delivery and five-hour stability require the explicit acceptance steps below.

## Planned automated gates

- Rust formatting and Clippy with warnings denied on supported targets; meaningful unit tests for buffer accounting, conversion, and recovery transitions.
- Windows-target compilation of the actual adapters. WSL/Linux unit tests cover portable logic, not WASAPI integration.
- TypeScript strict mode, lint, production frontend build, and tests for controls and failure states. Select one formatter/linter setup when scaffolding.
- One source of truth for Rust/TypeScript command/event shapes; prove binding generation or schema checking before claiming compile-time IPC safety.
- Commit lockfiles and pin the validated Rust/tool versions. Review dependency features and avoid unnecessary playback/decoder/network-fetch features.
- Independent review and applicable combined checks before closing implementation tasks.

UI tests should exercise user behavior with a mock control-plane adapter. Browser visual tests do not prove tray or WebView behavior; run a Windows desktop smoke as well. Tauri documents [mock and desktop tests](https://v2.tauri.app/develop/tests/) and [WebDriver support](https://v2.tauri.app/develop/tests/webdriver/).

## Visual acceptance

Use a compact utility layout with clear source, destination, streaming state, and mute. Prefer Mantine components and a consistent theme over decorative dashboards. Review screenshots of disconnected, connecting, streaming, muted, missing-device, and error states. Check keyboard navigation, visible focus, labels, non-color status cues, and Windows display scaling. A typecheck cannot certify a good layout. Keep raw screenshots and host settings in ignored `.trash/`; publish only sanitized results. Avoid brittle screenshots of live meters. Apply vetted upstream skills when available.

## Audio acceptance experiment

Proposed targets, to calibrate against the first working prototype:

| Measure | Initial target | Boundary and method |
| --- | --- | --- |
| Local pipeline delay | p95 ≤60 ms | Timestamp samples when capture exposes them; observe age at handoff for Discord transmission. Do not label this end-to-end latency. Account for hidden library buffering. |
| Listener delay | target ≤300 ms; investigate repeated >500 ms | Record a known marker from source and an independent receiving Discord client on a common recording clock where possible. Document network/client/device conditions and measurement uncertainty. |
| Mute response | ≤100 ms to stop submitting audible frames | Timestamp the command and last submitted non-silent frame; received playback can lag. |
| Session stability | five hours | Normal virtual-tabletop/browser/Discord load; no crash, manual restart, unexplained stoppage, progressive delay, or audible artifacts attributable to the app. |

Measure queue occupancy/age, underruns, overruns, stale-frame drops, reconnect attempts, CPU, and memory including WebView processes. Keep queues bounded. Sample clocks can drift even with one capture device against a network sender; investigate clock correction before increasing buffers.

Run silence/resume, 44.1/48 kHz inputs, stereo channel checks, and supported-format conversion. Keep a listener observation log and correlate problems with diagnostics. Do not promise zero network loss or detect every audible issue automatically.

Separate the stable five-hour run from deliberate network/device/permission failure tests. Verify retry exhaustion, preserving mute, explicit disconnect during recovery, device return, sleep/resume, tray reopening, and clean Quit. A silent input must not trigger reconnects.

Resources are a practical guardrail: no unexplained sustained load, unbounded queues, or continuing memory growth after warm-up. Compare experience with Kenku informally if useful; there is no exact Kenku superiority gate.

## Live visualization budget

Meters are a telemetry surface, not whole-application state. Keep audio callbacks and PCM in Rust. Publish bounded scalar observations independently of audio sample rate; the current engine/control telemetry cadence is approximately 10 Hz. The redesigned UI must isolate those observations from top-level React controls, selection lists and settings renders. Start with lightweight DOM/CSS; add Canvas only when a concrete history/plot warrants it. No charting/WebGL dependency is required for simple bars.

Target a capped 15–30 visual updates per second while visible, with bounded smoothing/history and no invented measurements. Suspend visualization work when hidden/minimized; verify the actual Tauri/WebView visibility path as well as browser behavior. Control state changes and mute/cancel must remain responsive. A focused render-count/telemetry-burst test should demonstrate that metering does not rerender the whole control surface; a visibility test should demonstrate paused visual work and a safe refresh on return.

Diagnostic values must name their measurement boundary. Capture queue occupancy can be expressed in frames or milliseconds at the known sample rate; it is not local total latency or listener delay. A pre-Discord output meter must observe samples after gain/mute at the native reader boundary, or be explicitly identified as derived. Do not fabricate latency, spectrum, delivery health or sample-rate measurements for visual appeal. The complete pipeline and five-hour resource checks remain separate acceptance.
