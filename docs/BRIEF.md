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

Future source types: playback-device/desktop loopback, application capture, or an Echocord virtual audio device. The virtual device is a driver project and not part of the MVP. Linux is a later platform; macOS is distant. Other receivers such as Mumble, TeamSpeak, or Zoom need individual feasibility research.

No media player, browser embedding, sound pad, playlist manager, custom decoder engine, or complex routing-matrix UI.

## Delivery sequence

1. Durable project context, task coordination, library survey, and WSL build setup.
2. Cross-compile and run a minimal Windows dependency probe; verify actual native dependencies.
3. Prove VB-Cable capture → Discord with bounded buffering and diagnostics.
4. Add the minimal UI, persistence, tray behavior, and recovery.
5. Failure-injection tests and a real five-hour session; revise provisional budgets using evidence.
