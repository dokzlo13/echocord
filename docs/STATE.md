# Current state

The alpha's real Discord routing, browser audio through a recording endpoint, perceived latency, and tray behavior were confirmed in user testing. Numeric end-to-end latency and five-hour stability remain unmeasured.

The current bounded iteration targets connection responsiveness/progress, a reviewed audio-control-surface redesign and room navigation, secure local setup where feasible, repository hygiene, and capture/mixing feasibility research. See tasks/BOARD.md. No driver-free capture or mixing implementation is promised in this iteration.

Current code: one portable Rust core plus Tauri shell; an owned Windows engine process contains capture and voice. Rust generates frontend types. Source/builds stay in WSL; private context, credentials, and raw evidence stay in ignored .trash. Only sanitized requirements/results belong in Git.

Use make build/check/app-run. Obsolete standalone prototypes and dependency probes have been retired from the active tree; production regression helpers remain. Adopted design is reviewed before UI implementation. Workers run in isolated worktrees; the coordinator owns integration, manifests and task state.
