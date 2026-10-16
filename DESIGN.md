# On Air desktop interface

Mode: Operate. Echocord is a compact audio control surface. Its maintained React components/styles and this document define the interface; mock data and demonstrations are not product behavior.

## Visual system

Use opaque slate surfaces and warm paper controls. Red denotes active Discord transmission; orange denotes faults. Self-host Sofia Sans for UI and Sofia Sans Condensed for names/headings, with tabular readouts and Cyrillic coverage. Keep source, font and icon licenses with their assets.

| Role | Values |
| --- | --- |
| Ground / chrome / panel / hover / inset | #1d2226 / #181c1f / #262c31 / #313940 / #14181b |
| Lines / strong line / card border | #323a40 / #47515a / #343c43 |
| Primary / secondary / muted ink | #f2efe7 / #b4bdc2 / #939ca2 |
| Action / hover / action ink | #ebe5d5 / #fffaf0 / #191c1e |
| Live / live ink | #ff4b3e / #ff8c82 |
| Standby / fault / fault ground / border | #f3c552 / #ff8b4a / #33241b / #6b4128 |
| Five source colours, then neutral | #3987e5 / #199e70 / #9085e9 / #d55181 / #008300 / #6b757b |
| Gain lane / track / thumb / clip | #9aa4aa / #3a434a / #ebe5d5 / #ffd166 |
| Card / control radius | 10px / 6px |

Names are 16px; section titles 15px; body 14px; secondary text 12.5px; readouts 12px; tags 11.5px. Focus uses a 2px paper ring with 2px offset. The static Echo trail logo is described in [BRAND.md](docs/BRAND.md); it must not glow like a live signal.

## Window and cards

Default 440×640, minimum 420×420, with remembered size. A 36px custom title bar holds brand, bot status, settings, minimize and Close; no maximize. Sources and Destination stack vertically and become two columns from 700px. Card lists scroll independently; the whole document does not scroll. Short windows compress spacing/diagnostics while keeping full-card controls reachable.

A source shows identity, state/remove, Capture/Stop, mix-only Mute and a combined gain/meter. A destination shows identity, state/remove, output controls, Mute/volume and progressive diagnostics. Discord uses Connect/Cancel/Disconnect and On air; a playback device uses Play/Stop and Playing/Stopped. Keep real server/device imagery and accessible fallbacks. Source faults stay on their cards; removing an active output uses inline confirmation.

Faults keep the controls mounted and usable. Retry uses the ordinary Capture, Connect or Play verb; a missing identity opens selection instead. Cleanup uncertainty shows Unconfirmed with its full accessible description and offers Stop/Disconnect, never a new start. Recovery shows the native explanation and permits Disconnect. Keep one wrapping note per card, prioritizing operation error, fault and then recovery. Keyboard focus survives phase changes. App-level announcements name the affected cards and combine shared failures.

Summaries list problems before active states; jump buttons include the state in their accessible names. Bot checking, rejected tokens and failed checks have distinct labels, with Check again available in bot setup. Errors stay attached to their own card until dismissed or another command targets it.

Source and destination numbering is independent: hide numbers/jump navigation with zero or one card; show matching contiguous one-based card/jump ordinals with multiple cards. Removal compacts display numbers without changing IDs, actions or histories. A single source keeps its on/muted/off summary without a numeric count. Jump labels identify section, ordinal and full identity; Discord room labels include server context.

Colours remain separate from ordinals. New sources use the lowest free coloured slot 0–4, then neutral 5 while all five are occupied. Live operations preserve surviving colours. On restore, reserve every valid coloured assignment before repairing neutral/out-of-range slots with available colours in saved order. Destination card numbers stay neutral; their state remains in existing status badges.

Wide layouts with one or two sources expand useful measured histories. Compact History diagnostics retain observed sparklines. Completed identity changes can briefly show the previous identity; pending work must not claim completion. Use a usable fallback when advanced container/scroll-state features are unavailable.

## Faders and measured data

Use one −60…0dB axis for gain and post-gain level. The bottom is Off, gain has no boost, arrows change 0.5 dB, Page keys 6 dB, and Home/End reach limits. Accessible values name dB. Source clipping and shared-mix overload are separate indications.

Source meters are after gain and before source mute, so muted activity remains visible under hatching. Output meters observe actual native post-volume/mute handoff; idle text says Not capturing/Not sending/Not playing as appropriate. On air means the local connection/sending state, not verified listener receipt.

Simple and History are persisted settings. Histories use bounded observed samples; do not invent past data. Paint meters at most 20 Hz, level histories 10 Hz and network readouts/plots 2 Hz. Suspend visual work while hidden, gap/reset stale history on return, and preserve reduced motion. Scalar telemetry must not rerender the control tree.

Label gateway ping as gateway, and buffer/drop/underrun measurements with their local units and boundary. They do not measure voice or listener latency. Omit unsupported transport/profile facts rather than adding permanent Unavailable placeholders or fictional values.

A live meter with no recent observation says No recent data; unavailable data is not silence. Audio observations expire after one second and gateway readings after 2.5 seconds. Repeated control snapshots do not extend those ages or add history samples. Output diagnostics distinguish drops and gaps over the last 60 seconds from frame totals since start; only the recent window carries a warning colour. A counter reset or observation gap invalidates the delta baseline instead of inventing a rate.

## Sheets and onboarding

Sheets fill the window below the title bar, with stable header/footer, a scrolling body, initial focus, Tab trapping, Escape and focus return. The underlying surface is inert. Source pickers separate Applications and Inputs; choices show selected/unavailable states and real identities. Edits are immediate and targeted, with no global Apply workflow.

Bot setup puts the token field first, focuses it and supports Enter to submit. A collapsed Create and invite a bot guide explains creating the application, copying a bot token and inviting with View Channels/Connect/Speak. Native help links use fixed allowlisted destinations and indicate browser opening; failures show a copyable public URL. Saving validates/encrypts a token without joining voice. Replacement is labelled distinctly and respects active Discord lifetimes.

Bot details show real profile/shared-gateway facts, discovered room counts and reservation status. Room discovery is not proof of access; permissions are checked when connecting. Do not display hardcoded Connect/Speak grants or unexplained OK badges. Settings shows the package-derived app version and only wired controls.

Failed preference writes keep live controls applied. A persistent bottom strip says Changes not saved, offers Details and Retry save, and spans both columns without moving card tops. Settings pins the full reason and retry in its footer. The warning clears only after saving succeeds; polite status announcements distinguish saving from rejected controls. Settings also links to the offline license viewer: filter document titles, deliberately select a document, and read/copy its plain text. Filtering and retrying preserve the open document; Escape returns to Settings and restores focus to Licenses.

Background saving is separate from accepting the live control change. A save retry remains pending while the native writer works, and a previous warning stays visible until the latest setup is saved.

Test relevant states, keyboard behavior and the five documented sizes in [TESTING.md](docs/TESTING.md). Browser screenshots do not establish native accessibility, display scaling or physical audio behavior.
