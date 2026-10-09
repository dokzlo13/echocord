# Candidate first-build architecture

Status: initial proposal, superseded where noted by the adopted [advisory decisions](REVIEW.md). In particular, an owned engine process is the cancellation boundary, and Rust model types now generate the frontend contract. Keep this document as the rationale for the small code boundaries, not a second source of implementation decisions.

## First testable outcome

A Windows desktop utility selects a recording endpoint and Discord server/channel, connects only on explicit action, sends captured audio with volume/mute, closes to a tray icon, and quits cleanly. It should handle ordinary disconnects without changing the selected source or destination. The first user test establishes practical behavior; it does not certify a five-hour release gate.

## Candidate code boundaries

Keep the probes as reproducible diagnostics. Put production Rust audio, control state, and Discord integration behind a small library surface with Windows-specific capture isolated by target configuration. A Tauri shell owns commands, tray lifecycle, settings, and snapshot delivery. React renders typed state and dispatches commands. Avoid one crate per module or a general routing/plugin framework.

One Rust representation should generate the TypeScript command/snapshot types. Compare a small type-generation tool with schema-based validation before implementation. Tests should exercise serialized names and union variants, not rely on hand-maintained lookalike types. The current prototype contract is input to this decision.

## Audio and connection candidates

CPAL opens only the explicitly chosen recording device. Prefer a supported 48 kHz configuration, otherwise preserve the actual rate and convert deliberately. A bounded preallocated queue separates capture callbacks from blocking/async work. Keep allocation, locks, networking, and UI serialization out of the callback where feasible.

Songbird consumes raw PCM and owns Opus/DAVE transmission. Probe its read-ahead, EOF, starvation, and cancellation behavior before selecting the live adapter. Naively returning unlimited silence from a Read implementation can permit unlimited read-ahead; blocking indefinitely can prevent cancellation. Queue age, under/overruns, and clock mismatch need explicit policy. A finite memory bound alone does not guarantee low latency.

Connection state and capture health are distinct. Explicit disconnect/quit invalidates pending attempts. Minimal retries preserve selections, volume, and mute. Device loss must not select a microphone or another endpoint. Bot removal and permission rejection should not trigger repeated rejoin attempts. The shared-control owner must serialize conflicting commands and prevent stale async completions from restoring a disconnected session.

The prototype can reject source/destination changes while connected, requiring disconnect before reconfiguration. Automatic startup connection is outside initial scope. Decide whether input metering needs capture while disconnected; do not silently open a recording stream merely to populate a device list.

## Credentials and first-run behavior

Backend-only local token loading, redacted errors/logs, and no token in frontend snapshots or Git. A private development credential file is acceptable for the first local build; generic end-user credential configuration and OS-backed persistence are separate decisions. Entering secrets in chat is unnecessary.

Read-only bot metadata discovery and voice joining should be separate operations. Joining and transmitting are explicit UI actions. Avoid silently replacing another client's active use of the same bot. First-run state should clearly show missing credentials, loading/discovery failure, or unavailable source instead of pretending the app is ready.

## Validation before user handoff

Require Windows cross-build plus native startup/closure, typed frontend checks, meaningful state/cancellation tests, and deterministic PCM adapter tests. Verify a local captured stream without logging audio or device identities to Git. Exercise bot metadata authentication privately without automatically joining a call. Report what still needs the user's real-call test, especially delivery, latency, and long-session behavior.

Review questions: smallest maintainable code layout; live PCM pacing/cancellation; where to bound retries versus library auto-reconnect; single-source IPC types; secure configuration UX; and the minimum credible validation before a first user test. UI/skill research should influence implementation before this proposal is adopted.
