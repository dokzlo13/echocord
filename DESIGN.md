# On Air · Stack

Mode: Operate. This is the owner-approved mixing-era design, adopted from the accepted working prototype. It replaces Signal Bridge. The production implementation reuses the prototype's components, styles and layout; simulation and fictional measurements are not product behavior. Core/API and platform choices are reviewed implementation decisions, separate from the approved appearance.

## Visual contract

Dark, opaque slate with label-paper controls; red denotes an active Discord destination and orange denotes faults. No game-themed decoration, remote fonts, translucent window or dashboard boxes. Self-host Sofia Sans for UI and Sofia Sans Condensed for names/headings/brand, with tabular live numbers and Cyrillic coverage. Keep font/icon licenses with the adopted assets.

| Role | Value |
| --- | --- |
| Ground / chrome / panel / hover / inset | #1d2226 / #181c1f / #262c31 / #313940 / #14181b |
| Lines / strong line / card border | #323a40 / #47515a / #343c43 |
| Primary / secondary / muted ink | #f2efe7 / #b4bdc2 / #939ca2 |
| Action / hover / action ink | #ebe5d5 / #fffaf0 / #191c1e |
| Live / live ink | #ff4b3e / #ff8c82 |
| Standby / fault / fault ground / fault border | #f3c552 / #ff8b4a / #33241b / #6b4128 |
| Source slots 1–5, then neutral | #3987e5 / #199e70 / #9085e9 / #d55181 / #008300 / #6b757b |
| Gain lane / track / thumb / clip | #9aa4aa / #3a434a / #ebe5d5 / #ffd166 |
| Card / control radius | 10px / 6px |

Names are 16px; slot titles 15px; body 14px; secondary text 12.5px; readouts 12px; tags 11.5px. Focus uses a 2px label-paper ring with 2px offset. Source colours always carry channel numbers; removed sources do not reassign surviving colours.

## Shell, slots and cards

Default 440×640, minimum420×420, resizable and size remembered. A 36px custom title bar holds brand, bot identity/status, settings, minimize and close-to-tray; no maximize. The two slots are Sources above Destination, becoming side-by-side at700px. Each header has a summary, numbered jump chips when useful and Add. This epoch supports up to eight sources and one destination; destination Add explains its limit.

Both lists scroll independently. Their heights follow content, preserve access to whole-card controls, and give spare height to useful destination telemetry. Short windows compress spacing and diagnostics; wide lone-source views may show observed level history. Use fallback layout when advanced container/scroll-state features are unavailable. Do not scroll the whole document.

A source card has identity, state/remove, Capture/Stop, mix-only Mute, and a combined gain/meter. Destination cards have identity, connection badge/remove, Connect/Cancel/Disconnect, destination Mute/volume and progressive diagnostics. Identity is one actionable unit opening the corresponding full-window picker. Normal source faults stay on their own cards; live destination removal asks for inline confirmation. The shared mix exists in Rust but has no separate Mix bar.

## Fader and telemetry

One -60…0dB axis combines a thin gain lane, round thumb/ceiling and post-gain meter. Gain retains half-dB precision; the bottom is Off. Arrows change0.5dB, Page keys6dB, Home/End reach limits; accessible values name dB. Native pre-gain clipping is an amber indication. Source meters are after gain but before source mute so activity stays visible under hatching; destination levels are actual post-volume/mute observations at native handoff. Idle labels say Not capturing/Not sending.

Simple and History are real persisted settings. History uses bounded rings of observed values, no invented past. Paint through refs/canvas at most20Hz and stop visual work while document/native visibility is hidden. Reduced motion disables decorative transitions. Telemetry must not rerender the control tree.

The outlined On air badge has a level-responsive red dot; it represents the actual connection/sending state, not proof of listener receipt. Muting retains the connection badge with no signal glow. Destination diagnostics expand or compact with available height. Queue/buffer and drop counts name their local measurement boundary. Gateway ping is labelled gateway if available; do not label it voice or listener latency. Unknown ping, people, permissions, voice-host or encryption facts are unavailable/omitted, never sample values from the prototype. Stage logs use observed transitions and elapsed times, not estimates.

## Sheets and targeted behavior

Full-window sheets sit below the title bar, with back/title, searchable lists, stable footer actions, Escape and focus return. The underlying surface is inert. Applications and recording inputs are separate source-picker tabs. Current/In mix/unavailable rows are explicit; browser scope and continuing original playback remain clear. Source activity/icons are real when available, with honest generic fallbacks.

No global Apply or queued-change band remains. Adding a source captures immediately without joining Discord; capture/stop and gain/mute operate independently per source. Replacing or removing a source does not leave the Discord room. Room selection is targeted; this implementation may reconnect even within the same server and must show that gap honestly. Rooms restore idle; no automatic voice connection. Restored recording sources reopen when Restore setup is enabled. Restored applications wait for explicit safe reattachment; ambiguous/stale identities require selection.

The bot chip opens real configuration/status and available metadata; token entry stays ephemeral and protected storage stays backend-owned. Settings expose only wired controls: Simple/History, Restore setup and Keep running in tray. Connection details replace the old prose-heavy Audio details sheet. No inactive future-routing controls, synthetic statistics or prototype scenario navigation ship.

## Review and acceptance

The owner approved the prototype and explicitly authorized direct source adoption. The coordinator records technical decisions, independent review and integration in EC-023–026. Compare actual production output at440×640,455×455,420×420,760×440 and1000×640 with the accepted reference. Test keyboard/focus, full-sheet sizing, multiple-card density, source faults, connection cancellation, native titlebar/tray and telemetry isolation. Native/audio evidence and owner listening feedback remain distinct.
