# Current state

The alpha's audible recording-input → Discord route, perceived low delay and tray behavior are user-confirmed. Numeric listener latency, comprehensive recovery and five-hour stability remain unmeasured (EC-006).

Integrated since alpha: responsive off-thread engine startup and truthful connection stages; reviewed startup/configuration cancellation fixes; protected per-user Windows token storage and private setup API; server icon/category metadata; tested frontend route-apply/setup adapters. `make check`, Windows build, 90 native core tests, encrypted bot discovery/UI/tray smoke, and synthetic storage/engine checks passed. No new live voice test was run.

UI is the remaining design seam. The original alpha and first redesign concepts were rejected. The owner selected the vertical Signal Bridge direction for further refinement, with production approval still pending. The refined private preview is ready for review: distinct input/shared-mix/output banks, slim meters, main-surface queue diagnostics, and a separate future-capacity study. Browser checks and an independent structural review passed; production acceptance remains pending. Production retains the alpha layout plus truthful stage text. Setup entry, contextual source/room panels, real output telemetry and isolated meter rendering remain EC-013 work. Read QUALITY.md for the metering/visibility gates; do not rerender the whole React control surface per telemetry frame.

Capture/mixing research is in CAPTURE.md. Next bounded experiment is EC-017: process-tree capture and independent local-playback suppression. No new capture type, mixer, virtual driver or multi-destination implementation is present.

Use make build/check/app-run. Normal Windows launches use encrypted local app data; development/test overrides are documented in DEVELOPMENT.md. Source/builds stay in WSL. Private feedback, review prototypes, screenshots, credentials and restart details remain ignored under .trash. Accepted workers are cleaned up; main is the integration and restart authority. Resume from tasks/BOARD.md and private LOCAL.md if present.
