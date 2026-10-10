# Echocord

<!-- impeccable:product-schema 1 -->

## Platform

web

The rendering surface is a web UI inside a native Windows 11 Tauri application, not a hosted website. Windows desktop conventions, keyboard access, compact sizing, and display scaling apply.

## Users and purpose

People running tabletop sessions or other voice meetings route selected local audio into a Discord voice channel while using their normal browser/player. Echocord is an audio bridge and compact control surface, not a media player or soundboard.

## Operating context

Current flow: one explicitly selected application process tree or Windows recording endpoint feeds one Discord bot/channel. Capture and joining start only on explicit action. The recording route's audible delivery, perceived delay, tray behavior and meters were confirmed in owner testing. Application capture has native synthetic/browser evidence and awaits owner listening feedback; five-hour stability is not established.

## Capabilities and constraints

Rust owns audio, recovery, and credentials; React/TypeScript/Mantine owns controls. Development/builds remain in WSL with small Windows runtime needs. Preserve input isolation, bounded recovery, reliable cancel/quit, and a visible distinction between selected settings and applied routing. No silent fallback to a microphone or another destination.

Settings use platform-standard local data and current-user encrypted credentials. Local source discovery is independent of Discord configuration/network. Application capture includes child processes and can include other browser tabs/windows; keep the voice call in another app. Original playback continues, and no independent local-mute control is offered because tested session mute also silenced captured samples. Exited/restarted applications require explicit reselection, and application selection resets on a fresh Echocord launch. Desktop loopback, a virtual endpoint, multiple inputs and multiple destinations remain future capabilities.

## Product principles

- Make source, destination, activity, and connection progress immediately understandable.
- Keep essential audio controls available during slow operations.
- Ship coherent user-facing iterations without prototype/debug/test instructions in the product.
- Use deliberate design and review for substantial feature sets.
- Keep personal information and raw evidence out of version control.

## Open decisions

The implemented Signal Bridge surface uses compact contextual source/room pickers, labelled queued changes, slim measured meters and main-surface diagnostics. Broader visual refinement follows real use. The next major architecture review is multiple sources feeding one shared mix and destination fan-out; one Discord bot can target only one voice channel per server at a time. A virtual driver, other receivers and public distribution remain separate work. Owner app-capture feedback, recovery/long-session checks and native accessibility/resource measurements are still open.
