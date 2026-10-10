# Task board

Only the coordinator edits scheduling state. Individual task files hold evidence and handoff. Dependencies must be done before a task becomes active.

## Active destination continuation

The owner reports visible drift from the approved prototype and authorizes research plus implementation of different destinations. EC-027 compares and restores fidelity, EC-020 owns native destination research/architecture/contracts/implementation, EC-028 reviews independently, and EC-029 integrates. The delivered build remains the working baseline. The bounded deliverable is one Discord room plus up to three explicit Windows playback outputs; multiple Discord rooms remain a researched architecture follow-on. Native design review precedes implementation. Preserve the running owner app/capture during development.

## Delivered mixing epoch

The owner-approved On Air Stack prototype is adopted in the Windows release. Up to eight explicit inputs feed one shared native mix; capture works before Connect, source changes are targeted, and the Discord destination has independent gain/mute and lifecycle. EC-023–026 are integrated and reviewed. Global Apply and the previous compact popovers are superseded. DESIGN.md is the current UI authority.

## Next pickup order

1. **Owner feedback:** manually listen to the mixed output, adjust source gains/mutes, stop or replace a source while connected, and assess the adopted UI at compact/wide sizes. Automated native/browser evidence is recorded; new mixed Discord listening awaits owner review.
2. **EC-020:** review destination fan-out on the delivered shared-mix foundation. Account for independent output failure/queues and Discord's one-channel-per-guild-per-bot constraint; no further broad redesign is a prerequisite.
3. **EC-006:** targeted recovery/resource/clock-drift checks and a five-hour live session with appropriate listeners.

The owner confirmed the preceding application-capture route and perceived low delay. This epoch has no live Discord test. Independent local mute, desktop loopback and an owned virtual audio driver remain outside the release. Source mute silences only that source's contribution to Echocord's mix.


| ID | Task | State | Depends on | Owner / attempt | Branch | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| EC-001 | Preserve discovery and establish WSL workflow | done | — | coordinator / a1 | main | [task](EC-001.md), integration 777ada2 |
| EC-002 | Prove complete Windows dependency build from WSL | done | EC-001 | coordinator + desktop worker / a1 | main | [task](EC-002.md), eb23ad1 + d35714b |
| EC-003 | Live PCM and Windows capture slice | done | EC-002, EC-008 | audio worker / a1 | main | [task](EC-003.md), 0583aec; native capture passed |
| EC-004 | Design UI states and typed control contract | done | EC-001 | UI worker / a1 | main | Superseded by reviewed direction and EC-011 implementation |
| EC-005 | Integrate minimal desktop app and recovery | done | EC-003, EC-004, EC-009, EC-010, EC-011 | coordinator / a1 | main | [first user test](../docs/FIRST-TEST.md) confirmed; usability follow-ups EC-012/013 |
| EC-006 | Validate failures and five-hour session | ready | EC-005 | unassigned | — | [task](EC-006.md) |
| EC-007 | Select vetted upstream skills and docs | done | EC-002 | research workers / a1 | main | [sources](../.agents/SOURCES.md), [evaluation](../docs/UI-DIRECTION.md) |
| EC-008 | Advisory architecture and UI direction review | done | EC-002 | advisory reviewer / a1 | main | [decisions](../docs/REVIEW.md) |
| EC-009 | Windows engine-process ownership | done | EC-008 | process worker / a1 | main | [task](EC-009.md), f2a1f82; independent review passed |
| EC-010 | Session controller and Discord engine | done | EC-008 | runtime worker / a1 | main | [task](EC-010.md), 604e043; known findings resolved |
| EC-011 | Production UI using reviewed direction | done | EC-007, EC-008 | UI worker / a1 | main | [task](EC-011.md), eaa2337; 42 browser tests and native smoke passed |
| EC-012 | Responsive connection progress | done | EC-005 | coordinator + runtime worker / a1 | main | [task](EC-012.md) |
| EC-013 | Accepted Signal Bridge implementation | done | EC-012 | coordinator + UI worker / a4 | main | [task](EC-013.md) |
| EC-014 | Capture and routing feasibility | done | — | research worker / a1 | main | [task](EC-014.md) |
| EC-015 | Repository and handoff cleanup | done | EC-005 | coordinator / a1 | main | [task](EC-015.md) |
| EC-016 | Protected storage and setup API | done | EC-005 | coordinator + storage worker / a1 | main | [task](EC-016.md) |
| EC-017 | Driver-free app capture: experiment and integration | done | EC-014 | capture worker / a1; coordinator integration | main | [task](EC-017.md), cd28fa9 + 9c0d887; integrated/native checks passed |
| EC-018 | Native post-gain meter | done | EC-012 | coordinator; implementation integrated | main | [task](EC-018.md) |
| EC-019 | Isolated telemetry presentation | done | EC-012 | coordinator; implementation integrated | main | [task](EC-019.md) |
| EC-020 | Destination fan-out and shared-mix extensions | active | EC-023 done; design/review checkpoint before implementation | native worker / a1 | agent/EC-020-a1 | [task](EC-020.md) |
| EC-021 | Picker clarity, queued changes and meter motion | done | EC-013, EC-018, EC-019 | UI worker / a1; coordinator integration | main | [task](EC-021.md), d417bc3; combined/native checks passed |
| EC-022 | Application-source selection | done | EC-021; accepted EC-017 source-contract milestone | UI worker / a1; coordinator integration | main | [task](EC-022.md), 8921a87; 53 browser tests and native app checks passed |
| EC-023 | Persistent capture and shared native mix | done | EC-017, EC-022; architecture review checkpoint | native worker / a1; coordinator integration | main | [task](EC-023.md) |
| EC-024 | Adopt approved On Air prototype | done | approved design, EC-021/022; EC-023 contract milestone | UI worker / a1; coordinator integration | main | [task](EC-024.md) |
| EC-025 | Independent mixing review | done | owner-authorized epoch | reviewer / r1 | read-only | [task](EC-025.md) |
| EC-026 | Native window and integration | done | approved shell; EC-023 contract milestone | coordinator / a1 | main | [task](EC-026.md) |
| EC-027 | Prototype fidelity and destination UI | active | EC-024/026; EC-020 contract checkpoint | UI worker / a1 | agent/EC-027-a1 | [task](EC-027.md) |
| EC-028 | Independent destination/fidelity review | active | owner-authorized continuation | reviewer / r1 | read-only | [task](EC-028.md) |
| EC-029 | Destination continuation integration | active | EC-020/027/028 checkpoints | coordinator / a1 | main | [task](EC-029.md) |
| EC-030 | Multiple Discord rooms | ready | EC-020; gateway/voice design review before code | unassigned | — | [task](EC-030.md) |

Blocked here means an explicit task dependency, not a stalled runtime goal.
