# First local Windows test

Build and run from WSL:

```sh
make setup-wsl
make app-install
make build
make app-run
```

Normal launches use local application data and a current-user DPAPI-encrypted bot token. Source/dependencies/builds stay in WSL; Windows runs the executable. No voice connection starts automatically. Keep other clients using the same bot stopped during a coordinated live test.

For isolated development storage, use `ECHOCORD_DATA_DIR` pointing to an ignored local directory. An explicit `ECHOCORD_TOKEN_FILE` selects a different encrypted credential destination; plaintext files are rejected. These overrides are optional and are not ordinary setup prerequisites.

## Sources before Connect

First launch has empty Sources and Destination slots. Open an application playing audio, then Add source. The full-window picker has Applications and Inputs tabs, search and Refresh. Select the intended running process; its card starts capture and shows its level immediately, without a bot token or Discord connection.

Application capture includes the selected process and its children, potentially several browser tabs/windows. Keep the Discord call in another app. Original playback continues; no independent local-mute control is offered. Native session-mute tests also muted captured samples. An owner-reported workaround routes the source app to an unused existing playback endpoint through Windows; Echocord does not configure that routing or ship a driver.

For the existing cable route, send the music application's playback to VB-Cable in Windows, then choose the corresponding recording endpoint under Inputs. Recording capture requires supported 48 kHz mono/stereo. An unsupported/missing input produces an error and never selects a microphone or desktop fallback.

Add more sources as needed, up to eight. Capture/Stop controls each capture lifetime. Source Mute removes it from the shared mix while its meter continues under hatching. Gain uses −60…0 dB or Off, with no boost. Source identity changes take effect on that card, and duplicate/identifiable overlapping captures are rejected. Source exit/restart requires refresh and selection of the new instance.

## Destination and live controls

Set up the bot once from the bot chip or empty destination slot. Paste its token into Test & save; Rust validates before protected persistence and never returns the token in snapshots. Then Add destination and pick the voice room. Connect belongs to its card and remains explicit. A destination can connect with no capturing source, but the room receives silence.

Listen through the regular Discord client. Check individual source gain/Mute/Stop and destination volume/Mute separately. Source edits and ordinary source faults should keep the destination connected and surviving sources audible. Destination Mute keeps the connection but its measured post-volume/mute meter falls silent. If the amber mix overload indicator lights, lower source gains; there is no automatic normalization.

Cancel during joining/recovery and Disconnect leave voice without stopping capture. Cleanup-incomplete requires Retry disconnect before another Connect. Changing a room briefly reconnects, even within the same server; follow the actual leaving/joining state. Live removal of the destination asks for confirmation. There is one destination and no global Apply or queued changes.

The On air badge and native output meter do not prove listener receipt. Gateway is Discord control-connection RTT, unavailable until measured; buffer is local queued audio. Neither is listener delay. Connection details expose available native counters and truthful unavailable states.

## Restore, window and tray

Restore setup defaults on. On relaunch, recording inputs reopen, application cards wait for one explicit Capture against their saved process identity, and the room returns idle. If that app instance ended, refresh and pick the current instance. No app-name or reused-PID match occurs automatically. The older single-source release did not persist application selection, so an upgrade can restore room/output preferences without an app card; add that application once to save a mixing-era card. Disabling restore starts with empty cards. Gain/mute, meter style and tray preferences persist.

The window defaults to 440×640, supports a 420×420 minimum and remembers size. Full-window pickers and separate card lists scroll; the document does not. Test native drag/minimize/resize and keyboard focus at the intended display scaling.

Close keeps capture/streaming running in the tray by default. Open restores the window; tray Mute controls the destination, Disconnect leaves voice, and Quit ends both worker lifetimes. Disabling Keep running in tray makes Close quit.

## Evidence and remaining acceptance

Owner testing confirmed the earlier recording and application-to-Discord routes with low perceived delay, tray operation and meters. Credential-free native fixtures cover selected application/child/browser isolation, stereo, silence/resume, source identity/exit, bounded queues/cancellation and shared two-source mixing through a stable reader. Browser checks cover card operations, restore, failures, keyboard/fit and actual SDK IPC wiring. These are bounded results; exact checks are in EC-017/023–026 and STATE.

The new mix still needs coordinated listening. Quantitative listener delay, participant-change/DAVE behavior, physical endpoint removal/sleep-resume, broad recovery/resource behavior and five-hour stability remain open. Clean-machine packaging, signing/installer and comprehensive native accessibility are separate work. WebView2 and Visual C++ runtime availability are distribution requirements; a Linux check cannot prove Windows audio behavior.

`make check` checks production code and fixture UI behavior without joining voice. `make app-smoke` rebuilds and tests native UI/IPC/tray without sending Connect or joining voice; normal startup restoration may reopen saved recording inputs. `make engine-smoke` checks actual disposable-worker startup using synthetic empty credentials. `make ui-dev` is explicitly a fixture preview. Native mixing regression helpers use owned synthetic sources and remain developer tools, not app setup steps.
