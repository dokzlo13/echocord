# Testing and validation

The normal gate is `python scripts/dev.py check` (`python3` on Linux/WSL). It runs formatting, strict Clippy, portable core/probe tests, generated-contract drift checks, frontend lint/type/build/browser tests, and Windows compilation checks. `check --portable` skips the Windows application compilation gate; on native Windows, core checks still compile Windows code and require MSVC. Native Windows CI also builds and runs the applicable native Rust tests.

## Automated checks

Core tests cover buffer accounting, exact source identity, gain/mute, shared mixing, bounded readers, output capacity and reservations, cancellation/recovery, stale generations, saved setup and source-colour repair. Regression tests should exercise meaningful behavior rather than mirror an implementation.

Recovery fixtures cover stale queued room changes after gateway failure, repeated transport-refresh events, bounded refresh budgets, replacement only after confirmed worker exit, permission denial, transient credential reads and cancellation during gateway recovery. They use fabricated credentials and no network connection. Persistence fixtures block the writer deliberately while issuing Stop, coalesce rapid changes, fence older completions and verify shutdown flush/failure. Windows versions of these fixtures open no audio endpoints.

Browser tests use the fixture/control adapter and cover per-card operations, configuration, errors, sheets, keyboard focus, telemetry isolation and layout. Use synthetic names/accounts. Test artifacts belong in `artifacts/`, not source control. The preview is not a functional audio runtime.

Check UI changes at 440×640,455×455,420×420,760×440 and1000×640 as relevant, with Simple and History meters. Preserve complete controls, independent list scrolling and no document overflow. Include long identities, faults, reduced motion, visible focus, full-sheet focus trapping/Escape/return, and progressive source/destination card/jump numbering. Inspect a bounded screenshot set and fix findings together.

Source add/remove/replace must preserve surviving IDs, colours, commands and histories. Test freed-colour reuse after repeated operations and saved neutral-colour repair without altering normal restoration. Frozen-clock tests should set time before navigation; do not race a browser clock against a host `new Date()`.

Recovery UI tests include ordinary same-source retry, explicit reselection, faulted gain/mute, unconfirmed cleanup, native recovery messages, per-card operation errors, bot recheck and focus retained through state transitions. Test the longest real fault messages in both meter modes at all five sizes; faders must remain usable and notes reachable by scrolling.

## Windows runtime

Cross-compilation and browser tests do not establish WASAPI or native WebView/tray behavior. Validate the actual executable with a separate disposable profile and no credentials first. Native smoke mode (`--smoke-report` plus explicit `--data-dir`) checks UI/IPC readiness, local discovery, output idle restoration, window resize/visibility and tray lifecycle; saved recording sources can reopen through normal restoration, so use only owned fixtures.

Also check drag, resize, minimize, close-to-tray, tray-disabled Close, reopen/Quit and remembered size on supported Windows display scales. Keyboard/browser accessibility checks do not establish native screen-reader or text-scaling acceptance.

For capture changes, retain exact process-tree/device selection, COM ownership, packet release, activation lifetime and cleanup invariants. Use owned synthetic applications and unrelated positive controls, checking isolation, stereo/silence, source exit, supported formats and bounded queues. A silent source is valid; it must not trigger broader capture or a different process.

## Real audio and Discord

Live tests are separate, explicit manual work. Use a dedicated bot and coordinated listeners; do not send audio or join a room as a side effect of automated CI. Select playback devices explicitly and avoid changing system default devices, endpoint volume or personal routes.

Verify source/destination mute and gain separately, independent output survival, cancellation during join/recovery, actual disconnect, permissions, device/network/sleep changes and shutdown. Distinguish decoded/captured samples, native submission, physical audibility and listener receipt. Gateway ping and a moving output meter do not prove end-to-end delivery or latency.

Quantitative latency, sustained resource usage, broad recovery and multi-hour stability need measurements with clear boundaries. Record what actually ran and what remains untested; a passing Linux suite is not Windows audio evidence, and one successful setup is not a general reliability guarantee.

## Release gate

CI must pass on the tagged source and versions must match. The checked Windows executable embeds complete notices and corresponding-source information; the release page provides runtime instructions. Verify the executable's build receipt, the exact corresponding-source archive and `SHA256SUMS`. Scan public source/history and release binaries for secrets or personal build paths before publication. Inspect **the final release executable's** offline Settings → Licenses catalog, including the original notice bytes; application unit tests rebuild frontend preview assets and do not replace this check. See [RELEASING.md](RELEASING.md).

## Reproducible audio evidence

The recording converter's portable fixtures cover 44.1/96 kHz × mono/stereo,
exact 48 kHz bypass, independent left/right tones, irregular callback sizes,
anti-alias rejection, non-finite input, reset/mute/discontinuity, bounded
queues and zero allocations during processing/reset. The real mix-bus tests
simulate producer jitter, stalls, independent readers and ±100/500 ppm clock
skew over 60 virtual seconds. These are deterministic model tests, not a
claim of glitch-free physical audio.

Run the conversion-only throughput fixture explicitly when comparing changes:

```sh
cargo test --locked --release -p echocord-core benchmark_eight_recording_converters -- --ignored --nocapture
```

It opens no device, uses no credentials and emits no sound. Record platform,
build mode and elapsed processing time; do not describe it as total app CPU,
power consumption or measured listener latency. Windows format-selection unit
tests use synthetic CPAL descriptors without accessing endpoints.

## Coordinated Windows acceptance sheet

Use a specific executable hash and a new disposable `--data-dir`, with no
personal settings or token. Before capture/playback/network tests, agree on
the exact owned sources, endpoints, dedicated bot/server/room and listeners.
Do not change system default routes or record private speech. Start with a
synthetic -18 dBFS 440 Hz tone, an independent 880 Hz tone, then an alternating
left/right pattern. Keep playback levels low; avoid feedback loops.

| Scenario | Required observation |
| --- | --- |
| Empty-profile launch | UI/IPC/tray work; no sources capture and all outputs remain idle. Offline license list and documents load from the EXE. |
| Recording formats | For each available 44.1/48/96 kHz mono/stereo endpoint, note negotiated format, channel order, correct pitch and bounds. Mark unavailable hardware cases untested. |
| Selected application | Only the selected process tree contributes; restart requires explicit reselection; unrelated synthetic producer is excluded. |
| Two sources | Independent gain/mute/stop, no stale signal after rapid mute/unmute; overload indication agrees with synthetic mix. |
| Independent outputs | Stopping/failing one selected playback device or Discord destination preserves peers. |
| Discord listener | A coordinated separate listener hears the correct signal in the intended room; local meters alone do not pass this case. |
| Recovery | Cancel during opening/join, unplug/replug, permission revocation, network loss and sleep/resume have bounded, truthful outcomes without a different source/room. |
| Persistence | Read-only/blocked fixture settings path preserves live controls, shows save warning, and retry saves after access is restored. Old schemas migrate; future file bytes stay intact. |
| Resource sessions | Record 30 minutes under ordinary/CPU-load conditions; separately run a four-hour soak. Compare RSS, handles, underruns/drops and audible glitches at start/end. |
| Shutdown | Quit ends owned workers and audio; close-to-tray behaves as configured. |

Record build hash, Windows build, CPU, selected format/type (omit private device
IDs), source/output counts, duration, verification boundary and pass/fail/
untested for each row. A click train can support separately measured local or
listener delay; name the measurement endpoints and report median/p95/p99 only
when actually measured. Do not expose credentials, server identifiers, user
paths or private audio in public reports. One PC provides one environment's
evidence; missing hardware is an explicit gap, not a simulated hardware pass.

The source-extracted CPAL packet fixture exercises the actual patched capture
function against synthetic packet providers, including silent/empty packets,
sample equilibrium, alignment, bounds and release-on-error. It does not open a
device. The callback allocation assertion and clock-skew simulations remain
separate from physical Windows acceptance.

For the longer shared-bus clock-skew baseline, run:

```sh
cargo test --locked --release -p echocord-core --lib measure_long_session_clock_skew -- --ignored --nocapture
```

This drives the real bus through 600 virtual seconds with independent readers
and reports queued, dropped and missing frames. It is intentionally outside the
normal gate. The current engine bounds queues and isolates peers but does not
compensate independent audio clocks: a faster reader eventually underruns, and a
slower reader builds a backlog before dropping frames. A short run can miss the
latter. Priming a fixed buffer alone cannot correct a lasting rate difference.

Suspension tests cover both producer-first and reader-first resume, including a
reader paused during a copy. A final performance-counter check rejects a block
that became stale during the read. No check can rule out a subsequent scheduling
pause between returning the block and submitting it to the device.
