# Echocord

A small Windows utility that mixes selected application audio and recording inputs, then sends the shared mix to Discord rooms in different servers and selected Windows playback devices.

`Selected applications / recording inputs → native shared mix → Discord + selected playback devices`

Add a source to start capture and see its level before connecting Discord. Each source has Capture/Stop, mix-only Mute and gain. Each destination has its own Mute and volume: Connect/Cancel/Disconnect for Discord, Play/Stop for a Windows device. Source changes take effect on their card and keep the bot connection; room changes briefly reconnect. Up to eight sources feed four total destinations: Discord rooms under the same bot, one room per server, and at most three Windows playback devices. New and restored outputs stay idle until explicitly started.

Application capture includes the selected process and its children, which can include other browser tabs/windows. Original playback continues. Local playback is explicit and optional; it does not suppress original application playback. No independent local-mute control or custom virtual driver ships. An existing cable can be a recording source or playback destination; avoid routing the mix back into a captured input.

Development, dependencies, builds and agent worktrees live in WSL. Windows runs the built application and native audio tests.

## Start or resume

Read [AGENTS.md](AGENTS.md), [project state](docs/STATE.md) and the [task board](tasks/BOARD.md). Product decisions are in the [brief](docs/BRIEF.md); the adopted On Air Stack design is in [DESIGN.md](DESIGN.md). Personal notes and temporary evidence belong in ignored `.trash/`.

- [Development environment](docs/DEVELOPMENT.md)
- [Capture and mixing boundaries](docs/CAPTURE.md)
- [Current architecture](docs/ARCHITECTURE.md)
- [Agent coordination](docs/WORKFLOW.md)
- [Quality gates](docs/QUALITY.md)

Use `make help` for supported commands. Setup is in [DEVELOPMENT.md](docs/DEVELOPMENT.md), and the current app flow is in [FIRST-TEST.md](docs/FIRST-TEST.md).

Status: On Air cards, full-window pickers, capture independent of Discord, protected bot setup, shared native mixing, multiple independently controlled Discord rooms and explicit playback fan-out are implemented. Owner testing confirmed the earlier recording and application-to-Discord routes with low perceived delay. Automated native fixtures cover isolation and multi-source behavior; live listening on the new mix, measured listener latency and five-hour stability remain separate acceptance. Final integrated build/check evidence is recorded in STATE and the task board.
