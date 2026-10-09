# Current architecture

Echocord is a Windows recording-device to Discord utility. Rust owns capture, audio transport, recovery and credentials. A Tauri shell owns the window/tray lifecycle; React renders a control surface. Source, dependencies and builds stay in WSL. The initial alternatives and review rationale are in [REVIEW.md](REVIEW.md); this document describes implemented boundaries.

## Control and process ownership

`crates/core/src/controller.rs` serializes commands and publishes snapshots. Selections and output preferences are ordinary local settings. Discovery authenticates the bot and fetches accessible rooms without joining voice. Only explicit Connect starts a session. Generation identifiers discard stale async results; requested Disconnect cancels pending startup and waits for observed process exit.

The shell executable also runs in `--engine-worker` mode, before initializing any WebView. The parent launches and owns that child through a Windows job with kill-on-close behavior. Bounded typed JSON flows over stdin/stdout; PCM never crosses this pipe or frontend IPC. Launch and pipe work are outside the controller event loop, so slow process creation does not block commands or snapshots. Cancelled startup cannot send Init to a late child. Graceful stop has a short allowance, followed by forced termination and an explicit exit-confirmation deadline. Unconfirmed cleanup remains visible and blocks another session.

`model.rs` generates the committed TypeScript contract through `export-bindings`. The shell provides snapshot/command IPC and a separate credential-configuration command. Tokens are absent from snapshots and shared commands. Connection stages report observed checking, preflight, gateway, voice join and capture progress; they are not time estimates.

## Audio path

`audio.rs` opens only the selected Windows recording endpoint through CPAL. The current adapter requires supported 48 kHz mono or stereo input, duplicates mono, and produces stereo f32 PCM. It never falls back to a default microphone. A bounded preallocated queue separates capture callbacks from the paced reader. Callbacks convert samples and update atomic telemetry without network/UI work, logging or allocation. Stale-frame dropping bounds local queue age; paced silence prevents unlimited decoder read-ahead.

`engine.rs` owns Serenity/Songbird and feeds their raw PCM adapter. Songbird handles Opus and encrypted voice transport. Volume/mute apply inside Rust; metering is before volume/mute and cannot establish listener delivery. Silence is valid input. Capture failure and connection health remain separate. Permission rejection/removal are terminal; bounded gateway/device recovery preserves the original route and output state. No local monitor is opened.

## Settings and credentials

Normal launches use platform-local application data. Development/test launches can explicitly override the data directory or individual paths. New tokens use current-user Windows DPAPI in a bounded versioned envelope with atomic replacement. Every runtime credential path requires encrypted data; plaintext and malformed envelopes are rejected. Configuration validates the bot before saving and is serialized against Connect. No automatic voice connection follows setup or launch.

## UI and evolution

The frontend receives snapshots and sends intentions; it owns no sample buffers or retry policy. Route drafts must remain visually distinct from the active route and take effect only through an explicit apply workflow. Closing the window keeps streaming in the tray; Quit closes process ownership.

Future capture and shared-mix work is research, not implemented routing functionality. Keep recording capture as an adapter while establishing process/desktop capture isolation, sample-clock handling, and Discord destination constraints before extending the current single-source/single-destination model.

## Evidence boundary

Portable tests cover state, protocol, ownership and PCM logic. Cross-compilation checks Windows adapters; native probes cover selected runtime behaviors. User testing confirms the initial audible route, perceived low delay and tray operation. Numeric end-to-end latency, comprehensive recovery and five-hour stability remain open in [QUALITY.md](QUALITY.md) and EC-006.
