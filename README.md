# Echocord

A small Windows audio-to-Discord utility. Capture a selected application's audio directly, or keep using an existing recording device such as VB-Cable, and send it to one Discord bot/channel.

`Selected application or recording input → Echocord → Discord`

Application capture uses the selected process and its children, which can include other browser tabs/windows. Original local playback continues; independent local mute and an owned virtual audio driver are not included. Recording-device capture remains available for isolated cable routing.

Development, source, Git history, and agent worktrees live in WSL. Windows runs the built application and real audio tests.

## Start or resume

Read [AGENTS.md](AGENTS.md), [project state](docs/STATE.md), and the [task board](tasks/BOARD.md). Product decisions are in the [brief](docs/BRIEF.md). Personal notes and temporary files belong in ignored `.trash/`.

- [Development environment](docs/DEVELOPMENT.md)
- [Library survey](docs/TECHNOLOGY.md)
- [Capture and mixing feasibility](docs/CAPTURE.md)
- [Agent coordination](docs/WORKFLOW.md)
- [Quality gates](docs/QUALITY.md)

Use `make help` for build, check, and UI-preview commands. Setup is in [DEVELOPMENT.md](docs/DEVELOPMENT.md).

Status: a manually testable Windows build with native application capture, compact source/room pickers, clear queued edits, encrypted in-app setup and measured meters. Native tests cover application/browser isolation and lifecycle; the earlier recording-to-Discord route passed owner testing. New app-to-Discord listening and longer-session acceptance remain open. The app starts disconnected; capture and voice joining require explicit Connect. See [manual test flow](docs/FIRST-TEST.md).
