# Current state

The alpha's audible recording-input → Discord route, perceived low delay and tray behavior are user-confirmed. Numeric listener latency, comprehensive recovery and five-hour stability remain unmeasured (EC-006).

Integrated since alpha: responsive off-thread engine startup and truthful connection stages; reviewed startup/configuration cancellation fixes; protected per-user Windows token storage and private setup API; server icon/category metadata; tested frontend route-apply/setup adapters. `make check`, Windows build, 90 native core tests, encrypted bot discovery/UI/tray smoke, and synthetic storage/engine checks passed. No new live voice test was run.

The owner accepted Signal Bridge as the implementation direction. Implement the current single-route surface now, polishing compact fit and simplifying graph-like connectors. Main controls, route identity, diagnostics and pending/error actions must fit without scrolling; selection lists may scroll inside drawers. Broader aesthetic redesign can follow real use. EC-013 owns the UI; EC-018 adds actual post-gain telemetry; EC-019 isolates frontend metering. Multi-input/output scenes remain future studies, not shipped controls.

Capture/mixing research is in CAPTURE.md. Next bounded experiment is EC-017: process-tree capture and independent local-playback suppression. No new capture type, mixer, virtual driver or multi-destination implementation is present.

Use make build/check/app-run. Normal Windows launches use encrypted local app data; development/test overrides are documented in DEVELOPMENT.md. Source/builds stay in WSL. Private feedback, review prototypes, screenshots, credentials and restart details remain ignored under .trash. Accepted workers are cleaned up; main is the integration and restart authority. Resume from tasks/BOARD.md and private LOCAL.md if present.
