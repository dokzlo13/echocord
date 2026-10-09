# Echocord

A small Windows audio-to-Discord utility. The first release replaces Kenku.fm in an existing VB-Cable setup.

`Browser → VB-Cable playback → VB-Cable recording → Echocord → Discord`

Development, source, Git history, and agent worktrees live in WSL. Windows runs the built application and real audio tests.

## Start or resume

Read [AGENTS.md](AGENTS.md), [project state](docs/STATE.md), and the [task board](tasks/BOARD.md). Product decisions are in the [brief](docs/BRIEF.md). Personal notes and temporary files belong in ignored `.trash/`.

- [Development environment](docs/DEVELOPMENT.md)
- [Library survey](docs/TECHNOLOGY.md)
- [Agent coordination](docs/WORKFLOW.md)
- [Quality gates](docs/QUALITY.md)

Use `make help` for build, check, and UI-preview commands. Setup is in [DEVELOPMENT.md](docs/DEVELOPMENT.md).

Status: the first Windows alpha passed a real audio/Discord user test; usability and setup improvements are in progress. It starts disconnected; capture and voice joining require an explicit Connect action. See [first test](docs/FIRST-TEST.md).
