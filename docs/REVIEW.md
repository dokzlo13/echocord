# Destination continuation review

EC-028 accepts one Discord output plus up to three explicit Windows playback outputs, each with its own tokenless disposable worker, independent native reader/device clock, controls and observed teardown. Startup/restoration stays idle; only Play opens a playback stream. The callback must remain nonblocking, Playing follows a real callback, and each output has a session generation for stale-result/history isolation. Endpoint identity is opaque, duplicates retain reservations through cleanup, and no missing endpoint falls back to a default.

Research confirms that different-guild Discord rooms are feasible and multiple gateway sessions are allowed by the platform. The implementation limit reflects this application's voice-state/disposal assumptions. A shared gateway with independently disposable voice drivers is the follow-on direction; it avoids treating unproven duplicate-worker ownership as a finished feature. Sources and architecture are in EC-020. Physical cable pairing cannot be inferred from names or endpoint IDs; explicit route guidance addresses feedback.

The owner reported visible prototype drift. Equivalent-state review found two-source expansion in wide windows, compact measured sparklines and completed-change notices missing from the initial adoption. EC-027 restores those actual-source behaviors while preserving real telemetry and matching type/tokens. Final exact implementation and integrated gate results are recorded in EC-028/029 and STATE; historical reviews below remain context.

# Prior mixing-epoch decision

The owner authorizes independent capture, multiple-source mixing and targeted card edits, replacing the earlier capture-only-on-Connect/global-Apply policy. Reviewed architecture: a persistent tokenless audio worker owns capture and a fixed native mix timeline; Discord retains its disposable worker and observed-exit/job-kill boundary. A dedicated bounded shared-memory PCM ring crosses those native workers through explicitly inherited handles. The parent holds handles but never maps/reads PCM; control JSON and frontend IPC still carry no samples. Source faults normally affect only that source; unconfirmed native cleanup is contained by worker ownership. That checkpoint shipped one destination, with the shared mix available to future independent readers. The original review below records why network disposal remains important.

Independent EC-025 review approved the final native and adopted UI checkpoints. Blocking findings were resolved before integration: atomic output gain/mute publication, source overlap checking outside the mix tick, pre-clamp overload reporting, failed-host Stop/Remove, inert controller preparation before single-instance admission, same-source phase preservation, telemetry-only IPC isolation, history freshness, and compact error recovery. Native mixing and UI checks support those decisions; new live Discord listening and long-session acceptance remain separate. EC-026 records final combined shell validation.

# Advisory decisions for the first user build

The independent architecture and desktop-UX reviews examined the pinned dependencies, actual probe code, and draft UI checkpoint 4f48825. This record distinguishes adopted changes from recommendations deferred beyond the first test.

## Adopted

| Finding | Decision | Required evidence |
| --- | --- | --- |
| Songbird's normal disconnect does not invalidate all scheduled retries and task termination is not acknowledged | Use a disposable, parent-owned engine process per explicit session, launched in an internal mode of the same executable. Audio remains inside the engine. Keep the parent REST-only for discovery. | Windows job ownership from creation, bounded graceful stop followed by terminate-and-wait, no orphan after parent failure, no reconnect before exit confirmation |
| Finite Cursor preparation does not prove a live PCM adapter | Implement and test the bounded live PCM source before enabling live transmission. No WouldBlock/temporary EOF; capped reads, frame alignment, stale-frame policy, silence/resume, cancellation, and explicit 48 kHz support. | Portable deterministic tests through the actual reader chain plus Windows capture validation |
| Capture and voice state were collapsed in the mock | Keep capture stopped/opening/running/missing/error independent of connection and output mute. Start capture only on explicit Connect. | State/command tests and real-adapter UI fixtures |
| Hand-maintained TS shapes can drift from Rust | Rust serde types generate TypeScript using pinned ts-rs 12.0.1. IDs remain strings; snapshots carry revision/session identity; Rust validates commands. | Serialization fixtures, generation drift check, strict frontend build |
| Draft primary button contrast and small-window control access were inadequate | Use sufficient text contrast, a stable accessible Mute output toggle, readable type, and persistent session controls at supported window sizes. | Screenshot and keyboard review, contrast measurement, native smoke |
| Ready-only fixtures hid first-run failures | Add configuration/discovery loading and errors, empty catalogs, unavailable saved routes, capture-open failure, disconnecting/cleanup failure. Display output state without claiming listener audibility. | Browser fixtures for each state and backend validation |
| Existing source cache and artifact launch could hide drift | Full generated-source hash inventory and explicit target directories are implemented. | Five cache-regression tests and redirected-target Windows smoke passed |

## Process lifetime and protocol

Keep one portable core crate and one thin Tauri shell. The shell launches the exact current executable with an internal engine flag before any WebView/tray initialization. The engine does not launch subprocesses. Windows creation-time job assignment with kill-on-close provides parent-death ownership; no breakaway or inherited job handle.

Typed bounded JSON lines carry initialization, volume/mute/stop, and status/telemetry. They carry no PCM. Credentials stay backend-only and must never enter CLI arguments, frontend types, protocol transcripts, or parser error excerpts. One writer per pipe, concurrent output draining, message-size limits, session generation, protocol version, and coalesced meter/control updates prevent deadlock and stale state.

Disconnect invalidates intent immediately, enters Disconnecting, and starts a deadline independent of pipe progress. Allow a short graceful leave, then terminate if required and wait for actual exit. Only observed process exit proves local quiescence. EOF or a Stopped message alone is insufficient. Failed cleanup remains explicit and disables Connect; Quit is always available. Already-submitted audio and remote membership may lag.

Parent-owned connection cooldown survives worker restarts. No automatic crash respawn. Respect Discord session-start limits; ordinary finite transport recovery stays inside the existing worker. Terminal permission/removal/unexpected-move events stop recovery.

## Deferred or superseded

- The earlier advisory suggested adding shutdown receipts throughout Songbird's task graph. The process boundary supersedes that larger dependency change; only the existing Opus compatibility patch is needed initially.
- Adaptive clock resampling follows measured drift. For the first test, support a deliberate 48 kHz configuration and bounded sample age rather than silently accepting unsupported formats.
- Public credential onboarding, installers/signing, clean-machine distribution, additional sources/destinations, and the five-hour acceptance run follow the first informative user test.
- Skills are a bounded selection/evaluation step, not a permanent prerequisite requiring more research before every change. Preserve verified upstream skills and project-specific decisions separately.

## Sources and limitations

The cancellation hazard and shutdown API limits were source-inspected, not reproduced against a live call: [Songbird core loop](https://github.com/serenity-rs/songbird/blob/v0.6.0/src/driver/tasks/mod.rs), [retry configuration](https://github.com/serenity-rs/songbird/blob/v0.6.0/src/driver/retry/mod.rs), and [manager removal](https://github.com/serenity-rs/songbird/blob/v0.6.0/src/manager.rs). Process lifetime guidance follows [Windows job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) and [termination semantics](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess).

Reader behavior follows [Symphonia buffering](https://github.com/pdeljanov/Symphonia/blob/v0.5.5/symphonia-core/src/io/media_source_stream.rs) and [Songbird mixer handling](https://github.com/serenity-rs/songbird/blob/v0.6.0/src/driver/tasks/mixer/mix_logic.rs). Browser screenshots and contrast calculations support UI findings but do not establish native Windows text scaling, screen-reader behavior, or audio delivery. Native and live-call checks remain explicit work.
