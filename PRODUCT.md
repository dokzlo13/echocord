# Echocord

<!-- impeccable:product-schema 1 -->

## Platform

web

The rendering surface is a web UI inside a native Windows Tauri application, not a hosted website. Desktop keyboard access, compact sizing, native window/tray behavior and display scaling apply.

## Users and purpose

People running tabletop sessions or other voice meetings mix selected local audio into a Discord voice channel while using their normal browser/player. Echocord is an audio bridge and compact control surface, not a media player or soundboard.

## Operating context

Up to eight explicitly selected application process trees or recording endpoints feed one common 48 kHz stereo mix and one Discord destination. Adding a source starts capture and metering without credentials or a voice connection. Source Capture/Stop, gain and mix-only Mute are independent of destination Connect/Cancel/Disconnect, volume and Mute. Edits are immediate and targeted; source changes do not leave the room. A room change currently disposes and reconnects the destination, even within one server.

Owner testing confirmed the earlier recording route and application-to-Discord delivery with low perceived delay. Native fixtures cover isolation and shared-mix behavior. Those results do not establish live mixed delivery, numeric listener latency, resource behavior over hours or five-hour stability.

## Capabilities and constraints

Rust owns PCM, mixing, recovery and credentials. A persistent tokenless audio worker and disposable Discord worker have separate native lifetimes; the parent and frontend never handle sample buffers. React/TypeScript with native controls, local Sofia Sans fonts and Lucide icons adopts the actual approved On Air prototype. Development and builds remain in WSL with small Windows runtime requirements.

First launch has empty source/destination slots. Restore setup is enabled by default: saved recording inputs reopen, application cards wait for explicit Capture, and the destination returns idle. App attachment validates the exact saved process identity; an exited/restarted app requires refresh and selection. No executable-name match, default microphone or alternate room is chosen silently. Disabling restore starts with empty cards. Gain/mute and ordinary settings persist, protected credentials remain backend-owned, and window size is remembered separately.

Source discovery is independent of Discord configuration/network. Application capture includes child processes and can include other browser tabs/windows; keep the voice call in another app. Original playback continues. Tested Windows session mute also silenced captured samples, so no independent local-mute control is offered. No local monitor, desktop loopback or owned virtual driver ships.

Settings expose Simple/History meters, Restore setup and Keep running in tray. Closing keeps capture/streaming running by default; disabling the tray setting makes Close quit. The tray provides Open, destination Mute, Disconnect and Quit. Only explicit Connect joins Discord; restoring a room or saving a bot token never joins it.

## Product principles

- Make source activity, destination state and connection progress immediately understandable.
- Keep source Stop and destination Cancel/Disconnect reachable during failures and slow operations.
- Use measured telemetry with clear boundaries; unknown values stay unavailable.
- Ship coherent product controls without prototype scenario navigation or simulated measurements.
- Use design and independent review for substantial changes; keep private evidence out of Git.

## Open decisions and remaining acceptance

On Air Stack is the accepted and implemented surface: actionable cards, independently scrolling lists, full-window sheets and combined dB faders/meters. Global Apply, the queued-change band, Mix bar and old Audio details sheet are retired. It defaults to 440×640, with a 420×420 minimum and two columns from 700 px.

Destination fan-out remains future work; one bot can occupy only one voice channel per server at a time. A virtual driver, other receivers and public distribution are separate work. Native accessibility/display scaling, broad recovery/resource checks, live mixed delivery and long-session acceptance remain explicit gates rather than inferred capabilities.
