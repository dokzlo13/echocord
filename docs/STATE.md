# Active mixing epoch

Owner confirmed the application-capture delivery and approved the On Air Stack redesign. The current epoch adopts the actual private prototype, adds multiple sources feeding one shared native mix, and separates capture from Discord connection. EC-023 owns native contracts/core, EC-024 frontend adoption, EC-025 independent review and EC-026 shell/integration. Previous global Apply, fixed single-source layout and tiny picker decisions are superseded by DESIGN/BRIEF.

The reviewed core uses a persistent tokenless audio worker plus disposable Discord worker, joined by a bounded native shared-memory ring; parent/frontend remain PCM-free. One destination is retained. Implementation and validation are active, so the new behavior is not yet a delivered build. Prior release checks and owner-confirmed behavior follow for context.

# Current state

The capture delivery epoch is complete. The Windows release supports one selected application process tree or one recording input feeding one Discord destination. EC-017/022 add native driver-free application capture and its reviewed source picker; EC-021 adds compact contextual source/room pickers, readable queued changes and meter-bar smoothing. DESIGN.md is the current visual authority. The app starts disconnected; source selection/apply and Connect stay explicit.

Application capture uses held PID/creation-time identities, includes child processes, and never widens to desktop or another source. Browser capture can include other tabs/windows; keep the call in another app. An exited/restarted app requires refresh/reselection, and application selection resets on a fresh Echocord launch. Recording preferences remain compatible. Original local playback continues; tested session mute also suppressed captured samples, so no independent local mute or owned virtual driver is shipped.

Final integrated checks passed: standard make check (portable formatting/Clippy/tests, generated types, frontend lint/type/build, 53 browser tests and Windows application Clippy); 108 native Windows core tests; Windows release build; fresh-data and encrypted-configuration native UI/IPC/local discovery/visibility/minimize/tray smoke; and actual engine-worker configuration rejection/observed exit. Native synthetic/browser fixtures proved selected-tree isolation, stereo, silence/resume, owned endpoint switching, bounded queues, identity/exit handling and cancellation. No live Discord session was started during this epoch.

Earlier owner testing confirmed audible recording-route delivery, perceived low delay, tray behavior and live meters. New application-to-Discord listening and perceived smoothing await owner feedback. Physical original-playback audibility, OS endpoint removal/sleep-resume, listener latency, comprehensive recovery/resource profiling and five-hour stability remain unmeasured or incomplete in EC-006. Automated results are not owner acceptance of those behaviors.

Rust owns PCM, recovery and credentials; React remains the control surface. Telemetry stays isolated from control renders and pauses while hidden. Input levels are pre-gain; mix levels observe actual post-gain reader handoff, not Discord receipt. Standard per-user DPAPI credentials/settings are preserved. Source, dependencies and builds remain in WSL; Windows runs the executable.

Main is linear, accepted worker branches/worktrees are removed, and only the main checkout remains. Redundant agent artifacts were removed after preserving a small current evidence set; original owner notes, credentials, settings and design references remain ignored. Read private LOCAL.md for local launch/evidence details. Use make build/check/app-run; FIRST-TEST.md describes the new manual flow.

Next: gather feedback on the delivered app, then review EC-020's shared-mix/fan-out architecture before implementation. EC-006 remains separate live recovery/long-session acceptance. Resume from BOARD and task evidence; no running worker or preview is needed.
