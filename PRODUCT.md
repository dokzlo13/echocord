# Echocord

<!-- impeccable:product-schema 1 -->

## Platform

web

The web UI runs inside a Windows Tauri desktop application. It is not a hosted service. Desktop keyboard access, compact resizing, native window/tray behavior and Windows display scaling apply. Linux/WSL development can run portable checks and cross-build Windows; other native audio runtimes are not shipped.

## Purpose

Stream selected local application or recording-device audio into Discord as a bot. Users control capture, mixing and each output independently without building a bot program or configuring a hosted service.

## Capabilities

Up to eight application process trees/recording inputs feed one native mix and four total destinations: Discord rooms in distinct servers under one bot, and at most three explicitly selected Windows playback devices. Sources have Capture/Stop, mix-only Mute and gain; Discord outputs have Connect/Cancel/Disconnect, volume and Mute; playback uses Play/Stop, volume and Mute.

Adding a source captures before any output connects. First launch is empty. Restore setup reopens recording inputs, leaves applications waiting for explicit Capture against their exact saved identity, and restores all outputs idle. Restarted apps require refresh/reselection. No broad/default capture, executable-name fallback or implicit monitor is used.

Bot configuration validates and encrypts a token locally using Windows DPAPI. Saving it never joins a room. Setup should explain the Discord Developer Portal, bot-only installation and View Channel/Connect/Speak requirements. Unknown permission or transport facts remain unavailable rather than appearing as granted capabilities.

Advanced audio settings provide automatic Sync correction (on by default) and Limit loud peaks (off by default). Correction handles small clock-rate differences at capture/mix and physical-playback crossings; the optional common-mix limiter adds about 3.7 ms and can reduce already-loud material without an overload warning. Both choices persist independently. Detailed timing and recent recovery observations belong in a separate diagnostic sheet, preserving the main cards' simple controls.

## Constraints and principles

Rust owns PCM, mixing, worker lifetimes, recovery and credentials. The UI is a control surface. Preserve selected identities and independent peers across ordinary faults and edits; expose unconfirmed cleanup honestly.

Application capture includes child processes and can include other browser tabs/windows. Original local playback continues; source mute affects only the shared mix. The app does not set Windows default routes or independently silence original application playback. Explicit local output can double sound or feed back if routed into a captured recording input.

The On Air layout uses compact actionable cards, independent list scrolling, full-window sheets and combined dB faders/meters. Default 440×640, minimum 420×420, two columns from 700px. Simple/History meters are observed data. Close keeps running in the tray by default; users can change that behavior or Quit explicitly.

## Status

Version 0.4.0 adds optional audio processing controls and measured timing diagnostics to bounded Discord recovery, responsive live controls and background preference saving. Recording conversion, versioned settings migration and direct downloads with offline licenses remain supported. It is AI-generated and AI-maintained software, provided as is without guarantees. Local meters are not listener-delivery proof. Broad native accessibility/scaling, physical recovery, measured listener latency, sustained resource use and long-session reliability remain validation work. Discord participant-audio input, Stage channels, multiple bot identities, a virtual driver and non-Windows audio support are outside current scope.
