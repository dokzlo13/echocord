# Task board

Only the coordinator edits scheduling state. Individual task files hold evidence and handoff. Dependencies must be done before a task becomes active.

## Active multi-room epoch

The owner requests planning and implementation of multiple room targets. EC-030 owns native design/contracts/implementation, EC-031 the existing UI extension, EC-032 independent review and EC-033 integration. The owner confirmed different-server rooms with the existing bot. Design/review covers one shared gateway, independent room workers and one room per server. The delivered playback build remains the baseline and must stay undisturbed while the new version is staged.

Next owner priority: EC-034 researches a Discord room as an aggregate participant-audio source for live playback/cross-server relay. Per-participant mute is later scope. This follows the current multiple-destination delivery; receive support and feedback/lifecycle design need their own checkpoint.

## Delivered destination continuation

The shared mix now feeds one Discord room and up to three explicit Windows playback devices. Each output has independent gain/mute/start/stop, session-fenced telemetry and native lifetime. Outputs restore idle; no driver or automatic playback is introduced. EC-027 also restores the actual prototype's wide two-source histories, compact measured sparklines and completed-change notices, with four-output control containment at all five supported sizes. EC-020/027–029 are integrated and independently reviewed.

## Next pickup order

1. **Owner feedback:** review the corrected UI; choose Add destination → Playback, then Play; test independent output gain/mute/Stop alongside source capture. The real native test covered one unused endpoint, not concurrent distinct hardware or live Discord listening.
2. **EC-030:** focused shared-gateway/disposable-voice-driver design for multiple Discord rooms, with a design/review checkpoint before implementation. Different-guild calls are feasible; same-guild simultaneous rooms need distinct bot identities.
3. **EC-006:** coordinated live Discord-plus-playback listening, physical endpoint/network/sleep recovery, independent clocks/resources and five-hour stability.

Owner testing confirmed the earlier capture routes and perceived low delay. New automatic results are recorded in STATE and tasks, without replacing live acceptance. Independent original-playback suppression, desktop capture and a new virtual driver remain outside this release. The current app and saved preferences are left for owner review; inspect processes before another test.

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
| EC-020 | Destination fan-out and shared-mix extensions | done | EC-023; reviewed native architecture | native worker / a1; coordinator integration | main | [task](EC-020.md) |
| EC-021 | Picker clarity, queued changes and meter motion | done | EC-013, EC-018, EC-019 | UI worker / a1; coordinator integration | main | [task](EC-021.md), d417bc3; combined/native checks passed |
| EC-022 | Application-source selection | done | EC-021; accepted EC-017 source-contract milestone | UI worker / a1; coordinator integration | main | [task](EC-022.md), 8921a87; 53 browser tests and native app checks passed |
| EC-023 | Persistent capture and shared native mix | done | EC-017, EC-022; architecture review checkpoint | native worker / a1; coordinator integration | main | [task](EC-023.md) |
| EC-024 | Adopt approved On Air prototype | done | approved design, EC-021/022; EC-023 contract milestone | UI worker / a1; coordinator integration | main | [task](EC-024.md) |
| EC-025 | Independent mixing review | done | owner-authorized epoch | reviewer / r1 | read-only | [task](EC-025.md) |
| EC-026 | Native window and integration | done | approved shell; EC-023 contract milestone | coordinator / a1 | main | [task](EC-026.md) |
| EC-027 | Prototype fidelity and destination UI | done | EC-024/026; EC-020 contract checkpoint | UI worker / a1; coordinator integration | main | [task](EC-027.md) |
| EC-028 | Independent destination/fidelity review | done | owner-authorized continuation | reviewer / r1 | read-only | [task](EC-028.md) |
| EC-029 | Destination continuation integration | done | EC-020/027/028 checkpoints | coordinator / a1 | main | [task](EC-029.md) |
| EC-030 | Multiple Discord rooms | active | EC-020; gateway/voice design review before code | native worker / a1 | agent/EC-030-a1 | [task](EC-030.md) |
| EC-031 | Multiple-room control UI | review | approved UI; EC-030 contract checkpoint | UI worker / a1; coordinator integration | main | [task](EC-031.md), 2a333b2; native/combined gates pending |
| EC-032 | Independent multi-room review | active | owner-authorized epoch | reviewer / r1 | read-only | [task](EC-032.md) |
| EC-033 | Multiple-room integration | active | EC-030/031/032 checkpoints | coordinator / a1 | main | [task](EC-033.md) |
| EC-034 | Discord room source feasibility and design | active | research independent; implementation after EC-033 and review | research worker / r1 | read-only | [task](EC-034.md) |

Blocked here means an explicit task dependency, not a stalled runtime goal.
