# Echocord

<!-- impeccable:product-schema 1 -->

## Platform

web

The rendering surface is a web UI inside a native Windows 11 Tauri application, not a hosted website. Windows desktop conventions, keyboard access, compact sizing, and display scaling apply.

## Users and purpose

People running tabletop sessions or other voice meetings route selected local audio into a Discord voice channel while using their normal browser/player. Echocord is an audio bridge and compact control surface, not a media player or soundboard.

## Operating context

Current flow: an explicitly selected Windows recording endpoint feeds one Discord bot/channel. Capture and joining start only on explicit action. Users hear the bot through Discord; local monitoring is not required. The alpha's live routing, perceived delay, and tray behavior have been confirmed in user testing; five-hour stability is not yet established.

## Capabilities and constraints

Rust owns audio, recovery, and credentials; React/TypeScript/Mantine owns controls. Development/builds remain in WSL with small Windows runtime needs. Preserve input isolation, bounded recovery, reliable cancel/quit, and a visible distinction between selected settings and applied routing. No silent fallback to a microphone or another destination.

Settings and onboarding should support ordinary desktop users, with platform-standard local data and protected credentials. Future capability research covers process/desktop capture, an optional virtual endpoint, and multiple inputs feeding one common mix sent to multiple destinations. These are not current capabilities.

## Product principles

- Make source, destination, activity, and connection progress immediately understandable.
- Keep essential audio controls available during slow operations.
- Ship coherent user-facing iterations without prototype/debug/test instructions in the product.
- Use deliberate design and review for substantial feature sets.
- Keep personal information and raw evidence out of version control.

## Open decisions

Research supports a bounded Windows process-capture experiment; independent local suppression still needs measurement. One Discord bot can target only one voice channel per server at a time. Full mixing, a virtual driver, other receivers, and public distribution are later work. The current redesign must preserve working audio behavior. The Signal Bridge direction is selected for refinement, with final visuals still under review. Retain contextual selection drawers; improve input/output separation, slimmer meters and main-surface diagnostics. Validate a coherent extension to multiple inputs and destinations around one shared mix.
