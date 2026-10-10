# First local Windows test

Build and run from WSL:

```sh
make setup-wsl
make app-install
make build
make app-run
```

Normal launches use local application data for preferences and the encrypted bot token. First-run bot setup accepts a token through Test & Save and validates it before encrypted persistence. Credentials use current-user Windows DPAPI; they stay out of snapshots and Git. No voice connection starts automatically. Keep other applications using the same bot stopped during the test.

For isolated development runs, set `ECHOCORD_DATA_DIR` to an ignored local directory. An explicit `ECHOCORD_TOKEN_FILE` override selects a different encrypted credential destination; plaintext files are rejected. These overrides are optional, not part of ordinary setup.

For application capture, open the app playing your audio. In Echocord's source picker, choose Applications, refresh if needed, and select the running instance. This captures the app's process tree without a virtual cable. Browser capture can include other tabs/windows; keep the Discord call in another app. Original local playback continues, and the measured Windows session-mute path also mutes captured audio. There is no independent local-mute control.

For the existing cable route, send the music application's playback to VB-Cable in Windows, then choose its recording endpoint under Recording inputs. Neither mode silently switches to another source.

Choose the Discord room, apply the selection, and press Connect. During a session, edits remain queued until Apply & reconnect; the picker can refresh sources while streaming. Listen through the regular Discord client. Check volume, Mute/Unmute, Stop, and reconnect after the short startup cooldown. If the captured app exits/restarts, refresh and select its new instance. A fresh Echocord launch also requires selecting an application again; recording-device preferences still restore. Closing the window keeps the session in the tray; Open restores it and Quit ends the application.

## Verification and remaining acceptance

The combined Windows executable cross-builds in WSL. Production Rust/TypeScript checks, portable state/protocol/PCM tests, and fixture UI tests pass. Native checks cover real backend configuration/discovery, frontend IPC, close-to-tray/reopen/quit, and synthetic engine configuration rejection without capture or gateway access.

The recording adapter retains its earlier real-callback, paced-read, mute/draining, stop and same-device reopen checks; the original native input probe used silence. Application-capture fixtures additionally passed selected root/child versus unrelated isolation, stereo, silence/resume, a two-tab browser root, owned playback-endpoint switching, process exit/restart, bounded queues and cancellation. Those are automated native results, not a live Discord listening test. Process supervision previously passed blocked-pipe, ignored-Stop and parent-failure scenarios.

Owner testing confirmed the earlier recording-route music delivery, perceived low delay, tray behavior and live meters. The next manual feedback is application-to-Discord delivery and the refined controls. Measured listener delay, participant-change/DAVE behavior, endpoint removal/sleep-resume, broad recovery and five-hour stability remain open. Clean-machine packaging, installer/signing and comprehensive native accessibility remain later work. The executable uses Visual C++ runtime and WebView2 available on the development host.

Recording-device capture requires a supported 48 kHz configuration; application capture supplies 48 kHz stereo through Windows. Unsupported/missing sources produce an error, never a default microphone or desktop fallback. Recovery is bounded and preserves the selected route. Disconnect waits for observed engine exit; Quit also closes Windows job ownership.

`make check` checks production code. `make app-smoke` rebuilds and tests native UI without joining voice. `make engine-smoke` exercises the actual worker with synthetic empty credentials. `make ui-dev` is explicitly a fixture preview. Obsolete standalone experiments have been retired; production regression tools remain.
