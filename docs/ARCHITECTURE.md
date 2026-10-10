# Current architecture

Echocord bridges one selected Windows application or recording device to one Discord destination. Rust owns capture, audio transport, recovery and credentials. A Tauri shell owns the window/tray lifecycle; React renders a control surface. Source, dependencies and builds stay in WSL. The initial alternatives and review rationale are in [REVIEW.md](REVIEW.md); this document describes implemented boundaries.

## Control and process ownership

`crates/core/src/controller.rs` serializes commands and publishes snapshots. Selections and output preferences are ordinary local settings. Local source discovery has its own status, generation and refresh command; it works without credentials/network and while streaming. A timed-out native scan retains occupancy until actual completion, preventing accumulating blocked scans; late results cannot overwrite the timeout. Discord discovery separately authenticates the bot and fetches rooms without joining voice. Only explicit Connect starts a session. Generation identifiers discard stale async results; requested Disconnect cancels pending startup and waits for observed process exit.

The shell executable also runs in `--engine-worker` mode, before initializing any WebView. The parent launches and owns that child through a Windows job with kill-on-close behavior. Bounded typed JSON flows over stdin/stdout; PCM never crosses this pipe or frontend IPC. Launch and pipe work are outside the controller event loop, so slow process creation does not block commands or snapshots. Cancelled startup cannot send Init to a late child. Graceful stop has a short allowance, followed by forced termination and an explicit exit-confirmation deadline. Unconfirmed cleanup remains visible and blocks another session.

`model.rs` generates the committed TypeScript contract through `export-bindings`. The shell provides snapshot/command IPC and a separate credential-configuration command. Tokens are absent from snapshots and shared commands. Connection stages report observed checking, preflight, gateway, voice join and capture progress; they are not time estimates.

## Audio path

`audio.rs` dispatches the selected source to the recording or application adapter. CPAL recording capture still requires supported 48 kHz mono/stereo input, duplicates mono, and produces stereo f32 PCM. `application.rs` uses direct Windows process-loopback APIs for the selected process and its children across playback endpoints. It initializes 48 kHz stereo float on a dedicated MTA thread, with fixed packet scratch and explicit silent/discontinuity handling. No default microphone, desktop mix, exclusion mode or alternate source is opened as a fallback.

Application identities include PID plus creation time and are verified through held process handles before capture. Discovery uses visible window-owning processes without parent ascent; the app's own tree and shell/desktop roots are excluded. An already-selected hidden app remains valid while its exact process is alive. Root exit is terminal; a restarted process requires refresh/reselection. COM activation storage survives late callbacks, opening is cancellable, and capture-thread cleanup has a bounded wait with a sticky failure. Failed cleanup exits the owned worker rather than reopening another capture.

Both adapters feed the same bounded 1,920-frame queue and paced stereo reader. Audio work stays off the UI/network path. Stale-frame dropping bounds age since native sample exposure; it does not measure hidden WASAPI backlog or listener delay. Paced silence prevents unlimited decoder read-ahead. Application capture leaves original playback in place; tested Windows session mute also suppressed captured samples, so no independent local-suppression control is exposed.

`engine.rs` owns Serenity/Songbird and feeds their raw PCM adapter. Songbird handles Opus and encrypted voice transport. Volume/mute apply inside Rust. Input metering is pre-gain; mix metering observes completed post-gain reader handoff, and neither establishes listener delivery. Silence is valid input. Capture failure and connection health remain separate. Permission rejection/removal and application exit are terminal; bounded gateway/recording-device recovery preserves the original route and output state. Startup handoffs preserve failed stop causes while honoring user cancellation. No local monitor is opened.

## Settings and credentials

Normal launches use platform-local application data. Development/test launches can explicitly override the data directory or individual paths. New tokens use current-user Windows DPAPI in a bounded versioned envelope with atomic replacement. Every runtime credential path requires encrypted data; plaintext and malformed envelopes are rejected. Configuration validates the bot before saving and is serialized against Connect. No automatic voice connection follows setup or launch.

Recording selection, destination and output preferences restore normally. Application selection is cleared on a fresh Echocord launch because a process identity is session-bound; no executable-name matching silently chooses a replacement process.

## UI and evolution

The frontend receives snapshots and sends intentions; it owns no sample buffers or retry policy. Its control store stays stable across meter-only observations; isolated telemetry consumers update DOM refs at bounded rates and pause for document/native hidden states. Actual input/post-gain levels remain native measurements; queue occupancy is not listener delay. Route drafts must remain visually distinct from the active route and take effect only through an explicit apply workflow. Closing the window keeps streaming in the tray; Quit closes process ownership.

The model remains one source and one destination. Desktop loopback, an owned virtual driver, multiple-source clock alignment and destination fan-out are future work. The next shared-mix design must handle overlapping process trees, independent queues and Discord's one-channel-per-guild-per-bot constraint before extending routing.

## Evidence boundary

Portable tests cover state, protocol, ownership and PCM logic. Native tests/probes cover application/child isolation, browser-tree scope, stereo, silence, owned playback-endpoint switching, source exit/restart, cancellation and bounded queues. Owner testing confirms the earlier audible recording route, perceived low delay and tray operation. New application-to-Discord delivery, physical local audibility, endpoint removal/sleep-resume, numeric end-to-end latency, comprehensive recovery and five-hour stability remain explicit acceptance in [QUALITY.md](QUALITY.md) and EC-006.
