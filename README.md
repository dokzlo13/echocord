# Echocord

A small Windows audio-to-Discord utility. The first release replaces Kenku.fm in an existing VB-Cable setup.

`Browser → VB-Cable playback → VB-Cable recording → Echocord → Discord`

Development, source, Git history, and agent worktrees live in WSL. Windows runs the built application and real audio tests.

## Start or resume

Read [AGENTS.md](AGENTS.md), [project state](docs/STATE.md), and the [task board](tasks/BOARD.md). Product decisions are in the [brief](docs/BRIEF.md). Personal notes and temporary files belong in ignored `.trash/`.

- [Development environment](docs/DEVELOPMENT.md)
- [Library survey](docs/TECHNOLOGY.md)
- [Capture and mixing feasibility](docs/CAPTURE.md)
- [Agent coordination](docs/WORKFLOW.md)
- [Quality gates](docs/QUALITY.md)

Use `make help` for build, check, and UI-preview commands. Setup is in [DEVELOPMENT.md](docs/DEVELOPMENT.md).

Status: local Windows build with the accepted compact UI, encrypted in-app setup and isolated metering. The initial audio/Discord route passed user testing; longer-session acceptance remains open. It starts disconnected; capture and voice joining require an explicit Connect action. See [first test](docs/FIRST-TEST.md).
