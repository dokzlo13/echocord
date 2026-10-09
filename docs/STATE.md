# Current state

Updated 2026-10-09. Foundation, dependency feasibility, advisory triage, and upstream-skill evaluation are complete. Production slices are active; see the task board. Adopted decisions are in REVIEW.md and UI-DIRECTION.md.

Read BRIEF for product scope, TECHNOLOGY for library findings, DEVELOPMENT for build setup, WORKFLOW for coordination, and tasks/BOARD.md for scheduling. Personal context, where present, is under ignored `.trash/` and is not part of this public project record.

Confirmed: isolated VB-Cable recording input, one Discord output, tray operation, minimal recovery, low delay as priority. Multiple sources/destinations and driver-free capture are future scope. Exact latency/retry numbers are proposals pending prototype evidence.

Code and development dependencies live in WSL; keep Windows additions small. Full Windows development tooling is outside the current build strategy.

Use plain Git task records with one coordinator and separate WSL worktrees for implementation workers. No background fleet or remote backup is implied by these files. Third-party skill research follows dependency validation; no custom or placeholder skills are installed.

The generic Windows executable builds entirely in WSL with the pinned Rust/MSVC target and runs on the Windows host. Formatting and Clippy pass for that probe. No Windows development packages were installed. This does not prove the audio or Tauri dependency graph.

No working integrated streaming app, live-call test, or active CI yet. The old mock is preserved as draft evidence. Rust owns generated UI types; separate workers implement live PCM/capture, Windows process supervision, parent/child session control, and the reviewed production UI. The coordinator owns manifests, shell, integration, and native verification. The next deliverable is a meaningful Windows build for real-user testing, not the five-hour release gate. Keep live connection/transmission explicit in the app.
