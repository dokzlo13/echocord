# Current state

Updated 2026-10-09. First local Windows build is ready for controlled user testing. Production slices are integrated and known review findings resolved. Next action: real-call feedback using FIRST-TEST.md; see the task board for remaining acceptance.

Read BRIEF for product scope, TECHNOLOGY for library findings, DEVELOPMENT for build setup, WORKFLOW for coordination, and tasks/BOARD.md for scheduling. Personal context, where present, is under ignored `.trash/` and is not part of this public project record.

Confirmed: isolated VB-Cable recording input, one Discord output, tray operation, minimal recovery, low delay as priority. Multiple sources/destinations and driver-free capture are future scope. Exact latency/retry numbers are proposals pending prototype evidence.

Code and development dependencies live in WSL; keep Windows additions small. Full Windows development tooling is outside the current build strategy.

Use plain Git task records with one coordinator and separate WSL worktrees for implementation workers. No background fleet or remote backup is implied by these files. Third-party skill research follows dependency validation; no custom or placeholder skills are installed.

The generic Windows executable builds entirely in WSL with the pinned Rust/MSVC target and runs on the Windows host. Formatting and Clippy pass for that probe. No Windows development packages were installed. This does not prove the audio or Tauri dependency graph.

The combined app builds in WSL and runs on Windows. Production checks, 42 fixture browser tests, native configured UI/tray/IPC smoke, empty-credential engine startup rejection, and selected-endpoint capture/mute/stop/reopen pass. The observed capture input was silent, so audible music and listener latency remain unverified. All live voice joining is explicit in the UI.

`make build`, `make check`, and `make app-run` are the main commands. Credentials, local preferences, launchers, and raw evidence stay in ignored `.trash/`. No remote or active CI service is configured. The five-hour release gate and wider packaging/accessibility checks remain later work.
