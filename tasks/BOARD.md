# Task board

Only the coordinator edits scheduling state. Individual task files hold evidence and handoff. Dependencies must be done before a task becomes active.

## Next pickup order

1. **EC-021:** a bounded usability pass on picker hierarchy, queued-change clarity and meter motion. Keep the accepted direction; avoid reopening a broad redesign or letting polish indefinitely postpone capture work.
2. **EC-017:** the next major feature is better audio capture without requiring a virtual cable for supported applications. Prove isolation/local-playback behavior, review source selection, then integrate a usable capture path; research alone is not the intended delivery endpoint. Preserve recording-device capture.
3. **EC-020:** multiple sources feeding one common mix and multiple destinations, after capture evidence and architecture review.

EC-006 remains separate recovery/long-session validation, scheduled when an appropriate live test is available. The current epoch delivers bounded EC-021 polish followed by integrated, manually testable EC-017 application capture. Native experimentation may run alongside the polish design; each substantial change passes design and independent review before integration.


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
| EC-017 | Driver-free app capture: experiment and integration | active | EC-014 | capture worker / a1; coordinator integration | agent/EC-017-a1 | [task](EC-017.md) |
| EC-018 | Native post-gain meter | done | EC-012 | coordinator; implementation integrated | main | [task](EC-018.md) |
| EC-019 | Isolated telemetry presentation | done | EC-012 | coordinator; implementation integrated | main | [task](EC-019.md) |
| EC-020 | One shared mix and destination fan-out | blocked | EC-017, reviewed architecture | unassigned; later iteration | — | [task](EC-020.md) |
| EC-021 | Picker clarity, queued changes and meter motion | active | EC-013, EC-018, EC-019 | UI worker / a1; coordinator integration | agent/EC-021-a1 | [task](EC-021.md) |
| EC-022 | Application-source selection | ready | EC-021; accepted EC-017 source-contract milestone | UI worker / a1 planned | agent/EC-022-a1 planned | [task](EC-022.md) |

Blocked here means an explicit task dependency, not a stalled runtime goal.
