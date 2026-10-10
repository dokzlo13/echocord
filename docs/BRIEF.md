# Product brief

## Accepted current requirements

Echocord mixes up to eight explicitly selected application process trees or Windows recording endpoints into one shared native mix and four total destinations: Discord rooms in distinct servers under the existing bot, and at most three Windows playback devices. It is a compact utility for 2–5 hour tabletop/voice sessions alongside a browser, player, Discord and other desktop tools. Playback is explicit per destination. No automatic monitor, media player, soundboard, routing matrix or DSP beyond gain/mute is required.

Adding a source starts capture and metering before any voice connection. Each source has Capture/Stop, mix-only Mute and precise dB gain; Discord has Connect/Cancel/Disconnect, Mute and volume; playback devices have Play/Stop, Mute and volume. Source add/change/remove/stop and ordinary faults do not leave Discord or silence surviving sources. Room changes are targeted and currently briefly disconnect/reconnect, including within the same server. There is no global Apply or queued-route workflow. Gain has no boost; users can lower individual sources when the shared mix overload indicator lights.

First launch is empty. Restore setup defaults on: saved recording cards reopen, application cards wait for explicit Capture against their exact saved process identity, and every destination restores idle. Adding an output never starts it automatically. Exited/restarted applications require refresh/reselection. No executable-name match, default microphone, broader capture or alternate room/channel fallback is allowed. Disabling restore starts with empty cards. Source/destination gain/mute and ordinary settings persist; bot setup never automatically joins voice.

Capture remains restricted to the selected process tree or recording device. Browser capture may include other tabs/windows; keep the voice call in another app. Original local playback continues. Tested Windows session mute also suppressed captured samples, so no independent local-mute control ships. Owner-reported routing to an unused existing playback endpoint is an OS-routing workaround, not a new Echocord control. A custom virtual interface/driver is postponed.

Rust owns samples, mixing, process ownership, credentials and recovery. Tauri + React + TypeScript is the validated desktop stack. The approved On Air prototype is directly adopted through native controls, local Sofia Sans fonts and Lucide icons; a component-library migration is not required. Source, dependencies, builds and worktrees stay in WSL, with small Windows runtime additions.

Use bounded recovery without changing source/channel identity or overriding intent. Preserve gain/mute through recovery; silence is valid input. Disconnect/Quit cancels network recovery. Do not repeatedly rejoin after removal or permission rejection. Expose normal source failures on their cards and unconfirmed native cleanup as a real containment failure.

The default window is 440×640, minimum 420×420, resizable with remembered size. Sources and Destination have independently scrolling card lists and become two columns from 700 px. Identities open full-window pickers. Keep whole-card actions reachable in compact/error states; the document does not scroll. The shared mix has no separate Mix bar. Connection details replace the old Audio details sheet.

Close keeps capture/streaming in the tray by default; Keep running in tray can make Close quit. Provide native Open, mute for all destination cards, stop for all outputs and explicit Quit. Settings expose persisted Simple/History meters, Restore setup and Keep running in tray. UI meters/graphs use bounded measured telemetry, not fictional prototype values. Unknown gateway/profile/connection facts stay unavailable or omitted; no voice RTT, participant or listener-delivery claim is invented.

## Confirmed experience and measured evidence

Owner testing confirmed the earlier recording and application-to-Discord routes with low perceived delay and working meters/tray behavior. The owner also reported retaining capture while routing original playback to an unused existing endpoint. This is feedback on those setups, not a controlled latency benchmark or a general independent-mute guarantee.

Native fixtures establish selected process-tree/browser isolation, stereo/silence/lifecycle behavior, bounded queues and shared two-source mixing with source/destination gain/mute and a stable reader. Browser checks and independent review establish direct prototype adoption, card/sheet behavior and telemetry isolation. The task records retain exact checks/results. Neither category establishes live listening on the new mix or five-hour stability.

## Acceptance targets requiring measurements

- Local pipeline p95 ≤60 ms, listener delay target ≤300 ms, investigation of repeated >500 ms, and mute submission ≤100 ms remain proposed targets, not proven capabilities.
- Validate sustained resources, broad recovery, physical endpoint removal, sleep/resume, native accessibility/display scaling and a separate five-hour normal session.
- Keep measurement boundaries and uncertainty explicit; local queue occupancy or pre-Discord meters do not measure listener delay.

[QUALITY.md](QUALITY.md) owns the applicable gates. The current executable is the test subject; integration results and remaining acceptance are recorded in STATE and EC-006/023–033.

## Future scope

The owner identifies a Discord voice room as the next input priority after multiple destinations: aggregate participants into one source stream and relay it to Windows playback or another Discord room; individual participant mute can follow later. Live capture/relay is requested, not persisted recordings. EC-034 completed receive/DAVE feasibility research; EC-035 queues a bounded native proof and reviewed design for shared server reservations, native audio input and feedback prevention. This capability is not implemented.

The common mix already feeds independently controlled Discord and playback readers. Multiple Discord rooms use a reviewed shared gateway with independently disposable voice drivers. One Discord bot can be in only one voice channel per server at a time; distinct same-server destinations require a reviewed identity/lifecycle design. Desktop/endpoint loopback capture, a custom virtual endpoint/driver, other receivers and later operating-system support are separate work. No inactive future-routing controls ship.

## Historical MVP and superseded decisions

The first release proved `browser → VB-Cable playback endpoint → VB-Cable recording endpoint → Echocord → Discord bot → voice channel`, one input and one destination. It established WSL cross-builds, native dependencies, protected storage, tray ownership and the initial live route. Native application capture then removed the virtual-cable requirement for selected apps.

Earlier Signal Bridge releases used Mantine, compact contextual pickers, capture only after Connect and a draft/Apply & reconnect workflow. Owner feedback and the accepted On Air mixing iteration replace those decisions. Their original review/task records remain useful history; they are not instructions for current product behavior. Long-session and quantitative listener acceptance have not been closed by the redesign.
