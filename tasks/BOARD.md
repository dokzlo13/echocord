# Task board

Only the coordinator edits scheduling state. Individual task files hold evidence and handoff. Dependencies must be done before a task becomes active.

| ID | Task | State | Depends on | Owner / attempt | Branch | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| EC-001 | Preserve discovery and establish WSL workflow | done | — | coordinator / a1 | main | [task](EC-001.md), integration 777ada2 |
| EC-002 | Prove complete Windows dependency build from WSL | done | EC-001 | coordinator + desktop worker / a1 | main | [task](EC-002.md), eb23ad1 + d35714b |
| EC-003 | Live PCM and Windows capture slice | active | EC-002, EC-008 | audio worker / a1 | agent/EC-003-a1 | [task](EC-003.md) |
| EC-004 | Design UI states and typed control contract | review | EC-001 | UI worker / a1, draft checkpoint | agent/EC-004-a1 | [task](EC-004.md); implementation awaits EC-007/008 |
| EC-005 | Integrate minimal desktop app and recovery | blocked | EC-003, EC-004 | unassigned | — | [task](EC-005.md) |
| EC-006 | Validate failures and five-hour session | blocked | EC-005 | unassigned | — | [task](EC-006.md) |
| EC-007 | Select vetted upstream skills and docs | active | EC-002 | research workers / a1 | main, research only | [task](EC-007.md) |
| EC-008 | Advisory architecture and UI direction review | done | EC-002 | advisory reviewer / a1 | main | [decisions](../docs/REVIEW.md) |
| EC-009 | Windows engine-process ownership | active | EC-008 | process worker / a1 | agent/EC-009-a1 | [task](EC-009.md) |
| EC-010 | Session controller and Discord engine | active | EC-008 | runtime worker / a1 | agent/EC-010-a1 | [task](EC-010.md) |

Blocked here means an explicit task dependency, not a stalled runtime goal.
