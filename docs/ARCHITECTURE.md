# Current architecture

Echocord mixes up to eight selected Windows applications/recording inputs into one Discord destination and up to three explicit Windows playback destinations. Rust owns capture, PCM, mixing, recovery and credentials. Tauri owns window/tray lifecycle; React renders controls and scalar telemetry. Source, dependencies and builds stay in WSL. [REVIEW.md](REVIEW.md) records design decisions and their history; this document describes the current implementation.

## Control and process ownership

`crates/core/src/controller.rs` serializes intentions and publishes authoritative card/configuration state. Source discovery has its own status, generation and refresh command, independent of Discord credentials/network. A timed-out scan retains occupancy until actual completion; stale results cannot replace a newer outcome. Bot/room discovery authenticates without joining voice. Only explicit destination Connect starts a Discord session.

The executable has three worker modes before WebView initialization:

- Persistent, tokenless `--audio-worker`: owns all capture sessions and the common mix. It runs without a destination and drains sources normally while Discord is idle.
- Disposable `--engine-worker`: owns Serenity/Songbird and one explicit Discord session. Connect attaches a stable shared-mix reader; Disconnect cancels startup/recovery and waits for observed worker exit. Sources keep capturing.
- Disposable, tokenless `--playback-worker`: one selected render endpoint per worker. Its native callback reads an independent bus cursor, applies atomic gain/mute and reports post-control callback samples. Play/Stop, open failure and cleanup affect that output.

All workers are parent-owned Windows jobs with kill-on-close. Bounded typed control/status JSON carries no PCM. The parent creates an unnamed pagefile-backed shared-memory ring but never maps its samples. Only narrowly inherited handles reach the audio/Discord/playback workers. Launch, capture open and cleanup work stay outside the controller loop; generations/revisions fence late results. Graceful stop, forced termination and exit confirmation are bounded. Unconfirmed cleanup stays visible and prevents unsafe reuse.

The audio ring contains 1,920 stereo frames with aligned atomic samples and per-slot sequence checks. Destination attachment starts at the current producer cursor; a stale producer yields silence. The Discord reader owns its pacing; playback is paced by the actual device callback. No filesystem audio transport or frontend PCM is introduced.

`model.rs` generates committed `ui/src/generated.ts`. The shell exposes snapshot/command IPC, independent scalar telemetry/visibility events and a private credential command. Tokens never appear in snapshots or shared card commands. Connection stages and elapsed UI logs describe observed transitions, not estimated completion or listener delivery.

## Sources and common mix

CPAL recording capture requires supported 48 kHz mono/stereo input and duplicates mono to stereo. Direct Windows process-loopback capture opens the selected process and its children across playback endpoints at explicit 48 kHz stereo float on a dedicated MTA thread. Both use bounded source queues. Silent/discontinuity packets, packet release, late activation storage, cancellation and cleanup remain native responsibilities.

Application identities include PID and creation time, verified through held process handles. Discovery uses visible window-owning processes without parent ascent and excludes Echocord and shell/desktop roots. A selected hidden app can remain valid while its exact process lives. Browser capture can include other tabs/windows; root exit/restart requires explicit refresh/reselection. There is no default microphone, desktop or alternate-source fallback. Duplicate recording selections and identifiable overlapping application trees are rejected; opening/retiring owners retain reservations until cleanup completes.

The audio worker nonblockingly drains up to eight sources on a common 5 ms / 240-frame timeline. Each gain is a precise native dB value from −60 to 0, or Off; mute removes that source from the sum while capture and its meter continue. Sources sum at unity and clamp to −1…1 after summation. There is no automatic normalization, boost, DSP routing matrix or drift resampler. Bounded queue/drop behavior is implemented; long-session clock/resource behavior remains acceptance work.

Normal source exit/open/capture failures affect their own card and do not leave Discord or stop other sources. Unconfirmed physical cleanup can terminate the audio worker as containment, with the failure exposed. Source Add/Replace/Capture/Stop/Remove never request a destination disconnect. Room changes currently dispose and reconnect the destination, including same-server changes; seamless moves are not claimed. Disconnect/quit cancels network recovery, and removal/permission rejection does not cause repeated rejoin attempts.

## Metering and destination

Source peaks are measured on the common timeline after source gain and before source mute. Native raw input clipping is latched separately. Mix overload is observed before the summed samples clamp, so an amber indication remains useful after destination attenuation.

The Discord worker applies destination volume/mute and meters actual completed native reader handoff. This meter is null while idle and does not establish Discord receipt. Songbird handles Opus and voice transport; the parent/frontend receive no audio samples. Queue/buffer and drop/underrun values name local pipeline behavior, not total or listener latency. Gateway heartbeat RTT is nullable, session-fenced and labelled gateway; no voice RTT or listener estimate is substituted.

## Playback output boundary

Playback enumerates actual WASAPI render endpoints by opaque API identity, independently of Discord/source discovery. Duplicate selections are rejected. Only supported 48 kHz stereo PCM formats are opened, with no default-device fallback. Each device callback nonblockingly reads bounded chunks from its own cursor; it does not sleep, allocate or use a control mutex. A slow output drops only its own stale backlog. Local meters observe callback output before Windows/device processing, not physical audibility; queue counters describe local shared-mix backlog, not total device latency.

Per-output session identifiers fence telemetry and let the frontend clear a restarted output without clearing its neighbors. The endpoint reservation remains through opening and stopping until exit is observed. No playback starts on add/restore; explicit Play is required. Native failures and cleanup status stay on the affected card. Identifiable application process roots containing Echocord are excluded; Windows IDs/names cannot prove cable pairing, so the UI explains the feedback condition without claiming automatic detection.

## Settings and credentials

Normal launches use platform-local data. The versioned settings format migrates prior recording/destination/output preferences without changing credentials. Restore setup defaults on: recording cards reopen, application cards remain stopped until explicit Capture validates their exact identity, and all destinations restore stopped/disconnected. Playback never starts on add or restore. Stale app identities require selection; executable-name/PID reuse matching is forbidden. Disabling restore starts with empty cards. Future-version settings are preserved rather than overwritten. Window geometry is a small separate shell sidecar, persisted on close/quit.

Current-user Windows DPAPI protects tokens in a bounded versioned envelope with atomic replacement. Plaintext/malformed runtime credential files are rejected. In-app setup validates before saving and is serialized against a voice session; no automatic Connect follows setup or launch. Restored capture activation occurs after the desktop single-instance gate, so a rejected second launch cannot open duplicate captures.

## Frontend and shell

The actual approved On Air prototype's card/sheet/fader/styles are adopted in React with native buttons/ranges, self-hosted fonts and licensed icons. Each edit targets its card; no local route draft or global Apply remains. Separate lists scroll without document scrolling. Full-window sheets preserve focus and Escape return; local action failures remain readable without inventing capture/connection phase changes.

The shell sends full control snapshots only when a borrowed control key changes, excluding scalar telemetry/revision but retaining cards, connection, settings, metadata and catalogs. The adapter additionally isolates telemetry from React control renders. Refs/canvas paint at most 20 Hz, with bounded rings, hidden/minimized pause, fresh history after visibility gaps and destination-session resets. No fictional history is filled in.

The custom window defaults to 440×640 with a 420×420 minimum, remembers safe size and has drag/minimize/close without maximize. Close keeps capture/streaming in the tray by default; when Keep running in tray is disabled it quits. Explicit tray Quit closes all worker ownership lifetimes. Tray Mute applies to all destination cards; its checked state means all current cards are muted. Stop all outputs leaves source capture running.

## Evidence boundary and evolution

Portable tests cover control, protocol, persistence, mixing and ownership logic. Credential-free native fixtures cover source-tree/browser isolation, stereo, silence, source lifecycle, bounded queues and two-source common mixing with gain/mute/replacement through a stable reader. UI checks cover targeted controls, restore, failure/cleanup, size/focus and telemetry isolation. Current commands/results are in EC-020/023–029 and STATE; native device evidence and live/long-session acceptance are distinguished.

Owner testing confirmed the earlier recording and application-to-Discord routes with low perceived delay. That does not establish live mixed delivery, numeric listener latency, physical endpoint removal/sleep-resume, comprehensive resource/recovery behavior or five-hour stability. [QUALITY.md](QUALITY.md) and EC-006 retain those gates. Multiple Discord rooms, desktop capture and a custom driver remain future work. The current fan-out uses independent shared-bus readers and one independently disposable playback worker per device.
