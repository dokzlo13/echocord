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

Route the music application's playback to VB-Cable's playback endpoint in Windows. In Echocord, choose its recording endpoint and a Discord room in their drawers, apply the selection, and press Connect. During a session, route edits remain queued until Apply & reconnect. Listen through the regular Discord client. Test volume, Mute output, Disconnect, and another connection after the short connection-start cooldown. Closing the window keeps the session in the tray; Open restores it and Quit ends the application.

## Verification and remaining acceptance

The combined Windows executable cross-builds in WSL. Production Rust/TypeScript checks, portable state/protocol/PCM tests, and fixture UI tests pass. Native checks cover real backend configuration/discovery, frontend IPC, close-to-tray/reopen/quit, and synthetic engine configuration rejection without capture or gateway access.

Explicitly selected native capture passed real-callback, paced-read, mute/draining, stop, and same-device reopen checks. The measured source was silent; this does not prove audible stereo delivery. Process supervision passed blocked-pipe, ignored-Stop, and parent-failure scenarios.

User testing confirmed music delivery, very good perceived latency, and tray behavior. Formal isolation checks, measured listener delay, participant-change/DAVE behavior, and recovery acceptance remain open. Five-hour stability, clean-machine packaging, installer/signing, and comprehensive native accessibility remain later work. The current executable uses Visual C++ runtime and WebView2 already available on the development host.

Capture deliberately requires a supported 48 kHz configuration. Unsupported/missing devices produce an error, never a default-microphone fallback. Recovery is bounded and preserves the selected route. Disconnect waits for observed engine exit; Quit also closes Windows job ownership.

`make check` checks production code. `make app-smoke` rebuilds and tests native UI without joining voice. `make engine-smoke` exercises the actual worker with synthetic empty credentials. `make ui-dev` is explicitly a fixture preview. Obsolete standalone experiments have been retired; production regression tools remain.
