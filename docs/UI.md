# Desktop control UI

The production React/TypeScript surface directly adopts the owner-approved On Air prototype's cards, sheets, combined fader, layout and styles; [DESIGN.md](../DESIGN.md) is the visual authority. It uses native buttons/ranges, local Sofia Sans/Sofia Sans Condensed fonts and Lucide icons. Prototype scenarios, navigation and fictional data are excluded from production.

## Cards and immediate operations

Sources and Destination form independently scrolling vertical lists with content-weighted height. At 700 px wide they become side-by-side. Slot headers show source state summaries, numbered jump chips where useful and Add. Surviving source colours stay assigned. The current limit is eight sources and one destination; destination Add explains its limit.

The source identity opens its picker. Adding a source starts native capture and metering before Discord Connect. Capture/Stop is independent of mix-only Mute and gain. Gain is −60…0 dB with no boost; Off is null on the wire. The combined native range changes by 0.5 dB with arrows, 6 dB with Page keys, and supports Home/End with dB accessible values. Source add/replace/stop/remove and ordinary source faults keep the destination connected. A source failure remains on its card. Duplicate/identifiable overlapping captures are rejected.

The destination identity opens the room picker. Connect/Cancel/Disconnect, Mute and volume apply to that destination. Muting preserves the connection. Connect is allowed with no capturing source, in which case the output is silent. Live removal asks for inline confirmation. Cleanup-incomplete exposes Retry disconnect and blocks another Connect. Room changes disclose a brief reconnect, including within the same server, and the badge follows actual leaving/joining state. Global Apply, queued changes, the Mix bar and old Audio details sheet are retired.

Operation errors remain visible inside the active picker/settings sheet or affected card, preserving authoritative native phases and reachable Stop/Disconnect. A removal followed by a save failure is still reported after the card disappears. No arbitrary IPC or secret-bearing exception is echoed.

## Pickers, setup and restore

Full-window sheets below the title bar have back/title, search, bounded list scrolling and footer actions. Search receives focus; keyboard selection, Escape, focus trapping and return to the triggering identity are supported. The underlying control surface is inert.

Applications and Inputs have separate tabs. Local refresh works without bot configuration/network; current, In mix and unavailable rows are explicit. Native instance details distinguish same-name applications. Browser capture includes the selected tree's tabs/windows; keep the call in another app. Original playback continues. Generic source-kind glyphs are used where no native executable icon exists. Room search groups actual servers/categories and uses supplied server icons or initials.

First launch has empty cards and offers bot setup. Test & save sends an ephemeral password value through a private command; Rust validates before current-user DPAPI persistence. Tokens never return in snapshots. Bot replacement is disabled during a voice session. The bot sheet displays supplied name/ID/avatar and known server catalog data; permissions are checked on connection rather than invented as badges. Unknown profile/network details stay unavailable or omitted.

Restore setup defaults on. Saved recording inputs reopen; restored application cards wait for explicit Capture with exact saved identity validation. An exited/restarted app requires refresh/reselection, never silent executable-name matching. The destination restores idle and never joins automatically. Source/destination gain/mute and settings persist. Simple/History, Restore setup and Keep running in tray are wired settings; unsupported preference controls are omitted.

## Control and telemetry boundary

Rust generates `ui/src/generated.ts`. The frontend sends intentions and owns no samples, process lifetimes or retry policy.

- `get_snapshot` and `echocord://snapshot` provide authoritative control state/revisions.
- `echocord://telemetry` carries bounded `MixTelemetry` scalars independently of control snapshots.
- `dispatch` accepts `{ command }` for targeted card/settings intentions.
- `configure_bot` accepts `{ token }`, validates/saves in Rust and returns no credential data.
- `get_visibility` and `echocord://visibility` report native shown/non-minimized state.

The shell filters scalar-only updates from control publication; the adapter preserves control-store identity across telemetry-only revisions and fences stale reads/disposed subscriptions. Refs/canvas paint meters at most 20 Hz, level history at 10 Hz and network values/plots at 2 Hz. Histories use bounded 60-second level and 2-minute network rings. Painting pauses for native/document hiding; visibility/gap resets avoid presenting old data as recent, and a new destination session resets its history. Reduced motion suppresses decorative transitions.

Source meters observe native post-gain peaks before source mute, so muted activity continues under hatching. Pre-gain source clipping and summed mix overload are separate native amber indications. Destination meters observe actual post-volume/mute reader handoff before Discord transport and are idle when not sending. The On air badge reports connection state, not listener receipt.

Gateway is measured Discord control-connection RTT, nullable until observed; it is not voice-server or listener delay. Buffer is local queued audio at the known 48 kHz rate. Drop and underrun counts are native counters. Graphs draw observed samples only. Listener counts, voice RTT, voice host/reconnect details and encryption negotiation are not simulated when unavailable.

## Window and acceptance

The custom 36px title bar provides native drag, bot/settings, minimize and Close; there is no maximize. Default size is 440×640, minimum 420×420, with safe remembered size. Close keeps capture/streaming in the tray by default; disabling Keep running in tray makes it quit. Tray Open/Mute/Disconnect/Quit retain native ownership semantics.

EC-024 records 45 passing browser/adapter/production-IPC checks and independent reference review at 440×640, 455×455, 420×420, 760×440 and 1000×640, including minimum-size History/errors. These checks establish browser behavior and faithful source adoption, not native display scaling, screen-reader behavior or live audio experience. EC-023–026 and [QUALITY.md](QUALITY.md) separate integrated Windows checks, automated native audio evidence and remaining owner/live acceptance.

Fixture mode is development-only and excluded from release assets. Browser tests use synthetic catalogs and measurements without live credentials/services. Raw captures, profiles and reports remain private. Vetted skill provenance is in [.agents/SOURCES.md](../.agents/SOURCES.md); earlier rejected UI research is historical rather than current design authority.
