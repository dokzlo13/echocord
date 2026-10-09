# Current state

Updated 2026-10-09. EC-001 complete, integrated in 777ada2. Next milestone: prove the full Windows dependency stack from WSL.

Read BRIEF for product scope, TECHNOLOGY for library findings, DEVELOPMENT for build setup, WORKFLOW for coordination, and tasks/BOARD.md for scheduling. Personal context, where present, is under ignored `.trash/` and is not part of this public project record.

Confirmed: isolated VB-Cable recording input, one Discord output, tray operation, minimal recovery, low delay as priority. Multiple sources/destinations and driver-free capture are future scope. Exact latency/retry numbers are proposals pending prototype evidence.

Code and development dependencies live in WSL; keep Windows additions small. Full Windows development tooling is outside the current build strategy.

Use plain Git task records with one coordinator and separate WSL worktrees for implementation workers. No background fleet or remote backup is implied by these files. Third-party skill research follows dependency validation; no custom or placeholder skills are installed.

The generic Windows executable builds entirely in WSL with the pinned Rust/MSVC target and runs on the Windows host. Formatting and Clippy pass for that probe. No Windows development packages were installed. This does not prove the audio or Tauri dependency graph.

No working audio stream, Tauri UI, live-call test, or active CI yet. EC-002 dependency probe and EC-004 UI design are active in separate scopes; see the board and task briefs for ownership. EC-007 selects real vetted upstream skills after EC-002; no synthetic skill is required to start a mock UI design.
