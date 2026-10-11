# Task board

Only the coordinator edits scheduling state. Individual task files hold evidence and handoff. Dependencies must be done before a task becomes active.

## Delivered multiple-room epoch

Session close is complete: EC-036 restores joining progress styles, EC-037 records the unproven Firefox distortion report without an audio change, EC-038 reviews, and EC-039 delivers the corrected build. The owner confirms multi-server operation. The owner confirmed the updated joining display and the running owner session is preserved. Discord-as-input is on hold until the owner reopens it. EC-040 is the current small UI epoch.

EC-030–033 are integrated and independently reviewed. Four total destinations can include multiple Discord rooms in distinct servers under the existing bot and at most three playback devices. Controls, reservations, cleanup and histories are independent; all outputs restore idle. Final integrated checks passed 120 portable library, nine probe, 132 Windows library and 124 UI checks in the final closing build plus staged build/native shell/worker gates. The native empty-source/four-room overlap is corrected and verified at 125% scaling. Owner confirms multi-server operation; controlled listener/failure/latency measurements remain open.

## Next pickup order

1. **EC-040 — current small UI epoch:** matching source/destination card and jump-control numbering; hide numbering with one card, reveal matching numbers with multiple cards. EC-041 supplies design and implementation review checkpoints.
2. **EC-006 / EC-037:** suitable coordinated listening and controlled delivery/recovery/resource/latency/five-hour acceptance remain open. No audio experiment belongs to this UI epoch.
3. **EC-035 — owner hold until further notice:** Discord-room input research is retained in EC-034; do not dispatch proof, receive implementation or lifecycle design without the owner reopening this work.

The joining visual fix is available through the existing launcher for manual review. Preserve any owner session that is running when work resumes; do not infer runtime state from this record.

Original application playback remains an OS route; independent local mute, desktop capture and a new virtual driver are outside the release. The approved UI and original owner/prototype artifacts are preserved. Current behavior and exact validation boundaries are in STATE and the task records.

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
| EC-030 | Multiple Discord rooms | done | EC-020; reviewed gateway/voice design | native worker / a1; coordinator integration | main | [task](EC-030.md), 3c2bad2; integrated/native gates passed |
| EC-031 | Multiple-room control UI | done | approved UI; EC-030 contract | UI worker / a1; coordinator integration | main | [task](EC-031.md), 2a333b2 + 3c6b33d; 111 checks and native correction passed |
| EC-032 | Independent multi-room review | done | owner-authorized epoch | reviewers / r1 gateway, r2 controller | read-only | [task](EC-032.md); native/UI exact-hash GO and final integration evidence |
| EC-033 | Multiple-room integration | done | EC-030/031/032 checkpoints | coordinator / a1 | main | [task](EC-033.md); staged release activated and app open |
| EC-034 | Discord room source feasibility research | done | owner next-priority request | research worker / r1; coordinator verification | read-only | [task](EC-034.md); no receive implementation/live proof |
| EC-035 | Discord room source proof and lifecycle design | blocked (owner hold) | EC-033 delivery, EC-034 | unassigned | — | [task](EC-035.md) |
| EC-036 | Joining progress presentation | done | owner feedback, EC-033 | UI worker / a1; coordinator integration | main | [task](EC-036.md), 94bea10; 124 UI checks and native shell gates |
| EC-037 | Idle/capture audio interference investigation | done | owner feedback, EC-033 | native investigation worker / a1 | main | [task](EC-037.md), fb044e2; no proven cause or audio change |
| EC-038 | Independent session-close review | done | EC-036/037 checkpoints | reviewer / r1 | read-only | [task](EC-038.md); exact-hash GO |
| EC-039 | Minor fixes and session close | done | EC-036/037/038 | coordinator / a1 | main | [task](EC-039.md); corrected executable installed, owner-confirmed UI |
| EC-040 | Consistent progressive card/jump numbering | active | owner-selected scope; EC-041 design checkpoint before code | UI worker / a1; coordinator integration | agent/EC-040-a1 | [task](EC-040.md) |
| EC-041 | Numbering design and implementation review | active | design GO; EC-040 implementation checkpoint pending | reviewer / r1 | read-only main; coordinator records evidence | [task](EC-041.md) |

Blocked here means an explicit task dependency or owner hold, not a stalled runtime goal.
