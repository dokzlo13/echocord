# Product brief

## Agreed requirements

Echocord replaces a general-purpose audio bot client for tabletop gaming sessions. Target sessions last 2–5 hours alongside a virtual tabletop, browser, Discord, and other desktop tools.

The first release uses `browser → VB-Cable playback endpoint → VB-Cable recording endpoint → Echocord → Discord bot → voice channel`. Capture one explicitly selected recording device. Support a locally configured Discord bot. Participants hear streamed music through Discord; local monitoring is unnecessary in the first release.

Provide a minimal utility UI for input selection, bot configuration, server/channel selection, volume, mute, and status. Closing the window keeps streaming in the tray; provide explicit Quit. Prefer low delay and reliable operation. Resources should be sensible and not noticeably worse than Kenku; no exact comparative benchmark is required.

Use minimal automatic recovery without choosing a different device/channel or overriding user intent. Surface significant failures. Preserve mute and volume across recovery. Silence is valid input. User disconnect/quit cancels recovery. Do not repeatedly rejoin after removal or permission rejection.

Rust core, Tauri + React + TypeScript + Mantine, with CSS Modules for limited custom styling, is the preferred stack. Source and agent work remain in WSL. Install necessary development tools in WSL; Windows additions should be small.

## Proposed details requiring prototype evidence

- Show an input level meter and actionable error states; remember the last device/destination.
- Keep connection state separate from capture health.
- Bounded retries after 1, 3, and 10 seconds; same-device return grace period of 30 seconds. These are provisional design parameters.
- Local pipeline p95 ≤60 ms, listener delay target ≤300 ms, investigate repeated >500 ms, mute submission ≤100 ms. These are provisional targets, not proven capabilities or signed-off release gates. Measurement boundaries are in `QUALITY.md`.
- OS-backed token storage, settings file for ordinary preferences, no SQLite initially.
- Start disconnected on a fresh app launch; automatic connection is outside the initial scope. Reconnection within an explicitly started session is a different behavior.

## Future scope

Multiple capture devices mixed into one shared mix; independent destinations including local playback and multiple Discord destinations. Investigate Discord's connection constraints and bot identities before promising simultaneous channels in one server.

Application capture has been added in the current epoch through an explicitly selected process tree. Future source types remain playback-device/desktop loopback or an Echocord virtual audio device. The virtual device is a separate driver project. Linux is a later platform; macOS is distant. Other receivers such as Mumble, TeamSpeak, or Zoom need individual feasibility research.

No media player, browser embedding, sound pad, playlist manager, custom decoder engine, or complex routing-matrix UI.

## Delivery sequence

1. Durable project context, task coordination, library survey, and WSL build setup.
2. Cross-compile and run a minimal Windows dependency probe; verify actual native dependencies.
3. Prove VB-Cable capture → Discord with bounded buffering and diagnostics.
4. Add the minimal UI, persistence, tray behavior, and recovery.
5. Failure-injection tests and a real five-hour session; revise provisional budgets using evidence.

## Post-alpha priorities

The current delivery epoch is authorized to ship a polished Windows build with application audio capture alongside the existing recording-device route. Capture must remain restricted to the explicitly selected application process tree. Source selection must explain browser-wide scope and original local playback behavior; app restart requires explicit reselection when identity changes. Independent local playback suppression is conditional on native evidence, not a promised control. Shared mixing and destination fan-out remain a subsequent iteration.

User testing confirms the core live flow, perceived latency, and tray operation. The next iteration emphasizes a deliberately designed compact audio surface, responsive and visible connection progress, fast grouped room navigation, and production-facing settings/copy. Pending route edits must remain distinct from the active route and require explicit apply. Secure in-app bot setup and standard per-user app data replace dependence on development-only setup.

Research before implementation: process/desktop capture while retaining recording inputs; optional virtual endpoints; suppression of local playback without suppressing the stream; multiple sources into one common mix and multiple destinations. No per-source/per-destination routing matrix or advanced DSP is required. Same-server Discord restrictions and local-mute semantics must be established before promising these features.

Signal Bridge is accepted as the implementation direction, with compact layout polish required and broader visual refinement deferred. Keep clearly separated input/output areas, slim calibrated meters and useful diagnostics on the main surface beside the signal controls. Retain contextual source/destination selection drawers. Preserve how the same structure grows to multiple inputs feeding one shared mix and multiple destinations, without implying independent routing matrices or shipping inactive future controls.

The main surface should fit within the compact application window without internal scrolling, including pending/error controls. Source/target selection lists may scroll in their drawers. Prefer clear spatial grouping over literal graph boxes and connector lines.
