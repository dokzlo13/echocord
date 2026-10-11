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

Names are 16px; slot titles 15px; body 14px; secondary text 12.5px; readouts 12px; tags 11.5px. Focus uses a 2px label-paper ring with 2px offset. Source colours remain assigned to native source identities; removing a source never reassigns surviving colours. Display numbers are list ordinals, independent of colour slots.

## Shell, slots and cards

Default 440×640, minimum 420×420, resizable and size remembered. A 36px custom title bar holds brand, bot identity/status, settings, minimize and close-to-tray; no maximize. The two slots are Sources above Destination, becoming side-by-side at 700px. Each header has a summary, numbered jump chips when useful and Add. Up to eight sources feed four total destinations: Discord rooms in different servers and at most three Windows playback devices. Destination Add explains capacity and per-server reservations.

Source and destination numbering is independent: zero or one card has no number or jump navigation; two or more cards reveal matching one-based card markers and jump buttons. Removing cards compacts visible ordinals and returning to one hides them again. Numbers never replace native IDs, React/telemetry keys, source colour slots or server/device imagery. Destination card markers use readable neutral ink; state remains in existing badges. A single source retains its On/Muted/Off header summary without a numeric count; a single destination retains its state on the card. Accessible card/jump names include section and identity, plus the ordinal when shown; room jump names retain server context.

Both lists scroll independently. Their heights follow content, preserve access to whole-card controls, and give spare height to useful destination telemetry. Short windows compress spacing and diagnostics. In wide layouts, one or two source cards expand into observed level histories and fill their list, matching the accepted prototype. Compact History diagnostics retain measured sparklines. Completed source/room changes briefly show the observed previous identity; pending work must not claim completion. Use fallback layout when advanced container/scroll-state features are unavailable. Do not scroll the whole document.

A source card has identity, state/remove, Capture/Stop, mix-only Mute, and a combined gain/meter. Destination cards have identity, status/remove, destination Mute/volume and progressive diagnostics. Discord uses Connect/Cancel/Disconnect and On air; playback uses Play/Stop and Playing/Stopped with local device facts. Identity is one actionable unit opening the corresponding full-window picker. Normal source faults stay on their own cards; live destination removal asks for inline confirmation. The shared mix exists in Rust but has no separate Mix bar.

## Fader and telemetry

One -60…0dB axis combines a thin gain lane, round thumb/ceiling and post-gain meter. Gain retains half-dB precision; the bottom is Off. Arrows change 0.5 dB, Page keys 6 dB, Home/End reach limits; accessible values name dB. Native pre-gain clipping is an amber indication. Source meters are after gain but before source mute so activity stays visible under hatching; destination levels are actual post-volume/mute observations at native handoff. Idle labels say Not capturing/Not sending.

Simple and History are real persisted settings. History uses bounded rings of observed values, no invented past. Paint through refs/canvas at most 20 Hz and stop visual work while document/native visibility is hidden. Reduced motion disables decorative transitions. Telemetry must not rerender the control tree.

The outlined On air badge has a level-responsive red dot; it represents the actual connection/sending state, not proof of listener receipt. Muting retains the connection badge with no signal glow. Destination diagnostics expand or compact with available height. Queue/buffer and drop counts name their local measurement boundary. Gateway ping is labelled gateway if available; do not label it voice or listener latency. Unknown ping, people, permissions, voice-host or encryption facts are unavailable/omitted, never sample values from the prototype. Stage logs use observed transitions and elapsed times, not estimates.

## Sheets and targeted behavior

Full-window sheets sit below the title bar, with back/title, searchable lists, stable footer actions, Escape and focus return. The underlying surface is inert. Applications and recording inputs are separate source-picker tabs. Current/In mix/unavailable rows are explicit; browser scope and continuing original playback remain clear. Source activity/icons are real when available, with honest generic fallbacks.

No global Apply or queued-change band remains. Adding a source captures immediately without joining Discord; capture/stop and gain/mute operate independently per source. Replacing or removing a source does not leave the Discord room. Room selection is targeted; this implementation may reconnect even within the same server and must show that gap honestly. All outputs restore idle; neither voice connection nor playback starts automatically. Restored recording sources reopen when Restore setup is enabled. Restored applications wait for explicit safe reattachment; ambiguous/stale identities require selection.

The bot chip opens real configuration/status and available metadata; token entry stays ephemeral and protected storage stays backend-owned. Settings expose only wired controls: Simple/History, Restore setup and Keep running in tray. Connection details replace the old prose-heavy Audio details sheet. No inactive future-routing controls, synthetic statistics or prototype scenario navigation ship.

## Review and acceptance

The owner approved the prototype and explicitly authorized direct source adoption. The coordinator records technical decisions, independent review and integration in EC-020/023–029. Compare actual production output at 440×640, 455×455, 420×420, 760×440 and 1000×640 with the accepted reference. Test keyboard/focus, full-sheet sizing, multiple-card density, source faults, connection cancellation, native titlebar/tray and telemetry isolation. Native/audio evidence and owner listening feedback remain distinct.

## Destination continuation

EC-027 compares production against the actual accepted prototype at equivalent states and logical sizes. That comparison takes precedence over earlier prose that limited expanded source history to a lone source. Fonts, tokens and measured compact geometry match; two-source expansion, compact history trends and observed swap notices are restored as deliberate fidelity corrections.

The reviewed output extension retains this visual language: up to four cards, multiple Discord rooms under the existing bot in distinct servers, and at most three Windows playback devices. Destination type selection belongs inside the full-window picker. Playback requires explicit Play/Stop and uses local device identity, availability and post-gain/mute output telemetry; it must not display Discord On air or gateway facts. Red remains reserved for Discord On air. New and restored output cards stay idle. A concise routing note explains feedback when playback returns into a captured recording input. No routing matrix, implicit monitoring or default-device fallback is introduced.

EC-031 extends the same cards and picker to multiple Discord rooms. Per-server reservations explain conflicts and retained cleanup; changing one room never replaces another card. Shared gateway status belongs in the existing bot chip/details. Per-card controls, idle restoration, histories and compact geometry remain independent; no multiple-account interface is introduced.
