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

## Discord destination and live controls

For Discord, set up the bot once from the bot chip or the Discord tab in Add destination. Paste its token into Test & save; Rust validates before protected persistence and never returns the token in snapshots. Then choose the Discord tab in Add destination and pick the voice room. Connect belongs to its card and remains explicit. A destination can connect with no capturing source, but the room receives silence.

Listen through the regular Discord client. Check individual source gain/Mute/Stop and destination volume/Mute separately. Source edits and ordinary source faults should keep the destination connected and surviving sources audible. Destination Mute keeps the connection but its measured post-volume/mute meter falls silent. If the amber mix overload indicator lights, lower source gains; there is no automatic normalization.

Cancel during joining/recovery and Disconnect leave voice without stopping capture. Cleanup-incomplete requires Retry disconnect before another Connect. Changing a room briefly reconnects, even within the same server; follow the actual leaving/joining state. Live removal of the destination asks for confirmation. There is one Discord destination and up to three playback destinations, with no global Apply or queued changes.

The On air badge and native output meter do not prove listener receipt. Gateway is Discord control-connection RTT, unavailable until measured; buffer is local queued audio. Neither is listener delay. Connection details expose available native counters and truthful unavailable states.

## Windows playback destinations

Open Add destination → Playback. Choose a specific Windows playback device, then press Play on its new card. Bot setup is unnecessary. Headphones, speakers and already installed virtual-cable playback endpoints are ordinary choices. Up to three device outputs can run alongside the Discord room, each with its own gain, Mute, Stop and status. An unavailable or unsupported device stays stopped with a reason; Echocord never switches to a default device. Initial support requires 48 kHz stereo PCM.

Source edits affect the common mix; muting/stopping one destination affects only that output. Test two different output cards, then stop one while the other continues. A failed output should leave other outputs and capture intact. A device card's meter measures samples submitted by its callback before Windows/device processing; it does not prove physical audibility.

Original application playback still follows Windows routing. Sending both the original app and its captured mix to the same audible device can double the sound. To listen only to the mix, the owner-tested workaround is to route the original app to a different unused endpoint, then explicitly play Echocord's mix on the desired device. Echocord does not change those Windows routes or provide independent local mute.

To feed another application's recording input through an existing virtual cable, select that cable's playback endpoint as the Echocord destination and its recording endpoint in the receiving app. Keep unrelated/original audio out of that cable if the receiver should hear only Echocord's mix. Do not feed the mix back into a recording endpoint currently captured as a source. Windows endpoint names/IDs do not establish cable pairing automatically; choose the route deliberately. No new virtual driver is installed.

## Restore, window and tray

Restore setup defaults on. On relaunch, recording inputs reopen, application cards wait for one explicit Capture against their saved process identity, and all destinations return idle. Playback never resumes automatically. If that app instance ended, refresh and pick the current instance. No app-name or reused-PID match occurs automatically. The older single-source release cleared application selection on startup and stored no application cards, so an upgrade can restore room/output preferences without an app card; add that application once to save a mixing-era card. Disabling restore starts with empty cards. Gain/mute, meter style and tray preferences persist.

The window defaults to 440×640, supports a 420×420 minimum and remembers size. Full-window pickers and separate card lists scroll; the document does not. Test native drag/minimize/resize and keyboard focus at the intended display scaling.

Close keeps capture/streaming running in the tray by default. Open restores the window; tray Mute applies to all destination cards, Stop all outputs stops playback and leaves voice while capture continues, and Quit ends all worker lifetimes. Disabling Keep running in tray makes Close quit.

## Evidence and remaining acceptance

Owner testing confirmed the earlier recording and application-to-Discord routes with low perceived delay, tray operation and meters. Credential-free native fixtures cover selected application/child/browser isolation, stereo, silence/resume, source identity/exit, bounded queues/cancellation and shared two-source mixing through a stable reader. Browser checks cover card operations, restore, failures, keyboard/fit and actual SDK IPC wiring. These are bounded results; exact checks are in EC-017/023–026 and STATE.

The new mix still needs coordinated listening. Quantitative listener delay, participant-change/DAVE behavior, physical endpoint removal/sleep-resume, broad recovery/resource behavior and five-hour stability remain open. Clean-machine packaging, signing/installer and comprehensive native accessibility are separate work. WebView2 and Visual C++ runtime availability are distribution requirements; a Linux check cannot prove Windows audio behavior.

`make check` checks production code and fixture UI behavior without joining voice. `make app-smoke` rebuilds and tests native UI/IPC/tray without sending Connect or joining voice; normal startup restoration may reopen saved recording inputs. `make engine-smoke` checks actual disposable-worker startup using synthetic empty credentials. `make ui-dev` is explicitly a fixture preview. Native mixing regression helpers use owned synthetic sources and remain developer tools, not app setup steps.
