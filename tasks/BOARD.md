# Task board

Only the coordinator edits scheduling state. Individual task files hold evidence and handoff. Dependencies must be done before a task becomes active.

| ID | Task | State | Depends on | Owner / attempt | Branch | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| EC-001 | Preserve discovery and establish WSL workflow | done | — | coordinator / a1 | main | [task](EC-001.md), integration 777ada2 |
| EC-002 | Prove complete Windows dependency build from WSL | active | EC-001 | coordinator + desktop worker / a1 | main; agent/EC-002-desktop-a1 | [task](EC-002.md) |
| EC-003 | Prove isolated capture to Discord | blocked | EC-002 | unassigned | — | [task](EC-003.md) |
| EC-004 | Design UI states and typed control contract | active | EC-001 | UI worker / a1 | agent/EC-004-a1 | [task](EC-004.md) |
| EC-005 | Integrate minimal desktop app and recovery | blocked | EC-003, EC-004 | unassigned | — | [task](EC-005.md) |
| EC-006 | Validate failures and five-hour session | blocked | EC-005 | unassigned | — | [task](EC-006.md) |
| EC-007 | Select vetted upstream skills and docs | blocked | EC-002 | unassigned | — | [task](EC-007.md) |

Blocked here means an explicit task dependency, not a stalled runtime goal.
