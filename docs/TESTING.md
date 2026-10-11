# Testing and validation

The normal gate is `python scripts/dev.py check` (`python3` on Linux/WSL). It runs formatting, strict Clippy, portable core/probe tests, generated-contract drift checks, frontend lint/type/build/browser tests, and Windows compilation checks. `check --portable` skips the Windows application compilation gate; on native Windows, core checks still compile Windows code and require MSVC. Native Windows CI also builds and runs the applicable native Rust tests.

## Automated checks

Core tests cover buffer accounting, exact source identity, gain/mute, shared mixing, bounded readers, output capacity and reservations, cancellation/recovery, stale generations, saved setup and source-colour repair. Regression tests should exercise meaningful behavior rather than mirror an implementation.

Browser tests use the fixture/control adapter and cover per-card operations, configuration, errors, sheets, keyboard focus, telemetry isolation and layout. Use synthetic names/accounts. Test artifacts belong in `artifacts/`, not source control. The preview is not a functional audio runtime.

Check UI changes at 440×640,455×455,420×420,760×440 and1000×640 as relevant, with Simple and History meters. Preserve complete controls, independent list scrolling and no document overflow. Include long identities, faults, reduced motion, visible focus, full-sheet focus trapping/Escape/return, and progressive source/destination card/jump numbering. Inspect a bounded screenshot set and fix findings together.

Source add/remove/replace must preserve surviving IDs, colours, commands and histories. Test freed-colour reuse after repeated operations and saved neutral-colour repair without altering normal restoration. Frozen-clock tests should set time before navigation; do not race a browser clock against a host `new Date()`.

## Windows runtime

Cross-compilation and browser tests do not establish WASAPI or native WebView/tray behavior. Validate the actual executable with a separate disposable profile and no credentials first. Native smoke mode (`--smoke-report` plus explicit `--data-dir`) checks UI/IPC readiness, local discovery, output idle restoration, window resize/visibility and tray lifecycle; saved recording sources can reopen through normal restoration, so use only owned fixtures.

Also check drag, resize, minimize, close-to-tray, tray-disabled Close, reopen/Quit and remembered size on supported Windows display scales. Keyboard/browser accessibility checks do not establish native screen-reader or text-scaling acceptance.

For capture changes, retain exact process-tree/device selection, COM ownership, packet release, activation lifetime and cleanup invariants. Use owned synthetic applications and unrelated positive controls, checking isolation, stereo/silence, source exit, supported formats and bounded queues. A silent source is valid; it must not trigger broader capture or a different process.

## Real audio and Discord

Live tests are separate, explicit manual work. Use a dedicated bot and coordinated listeners; do not send audio or join a room as a side effect of automated CI. Select playback devices explicitly and avoid changing system default devices, endpoint volume or personal routes.

Verify source/destination mute and gain separately, independent output survival, cancellation during join/recovery, actual disconnect, permissions, device/network/sleep changes and shutdown. Distinguish decoded/captured samples, native submission, physical audibility and listener receipt. Gateway ping and a moving output meter do not prove end-to-end delivery or latency.

Quantitative latency, sustained resource usage, broad recovery and multi-hour stability need measurements with clear boundaries. Record what actually ran and what remains untested; a passing Linux suite is not Windows audio evidence, and one successful setup is not a general reliability guarantee.

## Release gate

CI must pass on the tagged source, versions must match, and the Windows archive must contain the checked executable, runtime instructions, full notices and corresponding-source information. Verify archive contents and checksums. Scan public source/history and release binaries for secrets or personal build paths before publication. See [RELEASING.md](RELEASING.md).
