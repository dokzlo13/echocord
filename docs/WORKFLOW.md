# Agent coordination

## Durable state

Use Git-tracked Markdown initially. The board is the authoritative scheduling record; individual task files hold scope, decisions, evidence, and handoff. `docs/STATE.md` is a short restart index, not a second task database. Chat threads and runtime goals are convenient execution context, not the source of truth.

One coordinator schedules and integrates work. Start with two or three workers only where tasks are independent. Model and effort preferences belong in local harness configuration. No project file keeps a process running after its session closes.

Verify tools/model access before dispatch. With no delegation support, state the limitation and preserve task ownership; do not simulate a fleet in prose. Keep runtime thread identifiers and account details in ignored local records.

## Task lifecycle

States: ready, active, review, done, blocked. Ready requires completed dependencies. The coordinator owns board edits. Each task has one active worker; task evidence is written by that worker and reconciled by the coordinator at integration.

Before dispatch, commit the task brief and board assignment with owner, attempt ID, branch/worktree, dependencies, allowed files, and acceptance criteria. Use a separate Git worktree/branch per code-writing worker. Verify the actual working directory: agent spawning alone does not imply filesystem isolation. Research-only agents can share a read-only source view.

Task branch convention: `agent/EC-002-a1`. Worktrees may live under ignored `.worktrees/` on the WSL filesystem. Keep manifests/lockfiles and shared IPC contracts under one named owner. Agree interfaces before parallel implementation.

Workers deliver coherent commits, changed paths, commands and outcomes, outstanding risks, and the next action. The coordinator reviews changes, integrates them sequentially, runs relevant combined checks, and records the integration commit before marking done. Independent review should target actual risks and acceptance behavior, not wording/style churn.

## Restart and interruption

Read AGENTS, STATE, the integration-branch board, and the assigned task. Inspect Git status, recent commits, worktrees, and any still-running tools. A worker's copy of the board can be stale.

Before reassignment, establish that the previous worker has stopped and inspect its uncommitted work. Issue a new attempt ID. Reconcile already-integrated commits before replaying work. Checkpoint at meaningful milestones and before yielding; do not rely on an end-of-session memory dump.

Keep a task record useful to someone with no chat history. Preserve rejected alternatives only when their rationale prevents rework. Local commits survive new sessions but are not off-machine backup. No remote is configured initially; publishing or selecting one is a separate user decision.

## Why not beads yet

Markdown needs no service and travels with code commits. It works here because one coordinator serializes scheduling. It has no atomic distributed claim mechanism; do not use it as one.

Consider beads when missed dependencies, duplicate assignments, or independent schedulers/machines cause real coordination problems. Current beads uses Dolt storage and has its own setup/sync lifecycle; it is more than an in-Git text tracker. Retain stable EC task IDs if migrating. Check the chosen release because its topology/defaults are changing.

Sources: [beads v1.3.1](https://github.com/gastownhall/beads/releases/tag/v1.3.1), [installation](https://github.com/gastownhall/beads/blob/v1.3.1/docs/getting-started/installation.md), [Git worktrees](https://git-scm.com/docs/git-worktree).

## Harness portability

AGENTS.md holds shared rules. CLAUDE.md imports it using `@AGENTS.md`; no duplicated rulebook. Verify instruction loading in a fresh session before treating harness integration as tested.

Research existing skills after the dependency stack is validated. Prefer maintained upstream skills with credible adoption/review signals and actual desktop-UI relevance. Stars alone are insufficient evidence. Record source URL, revision, license, inspection findings, and evaluation results. Import only selected, license-compatible resources into a shared skills folder with thin harness entry points where needed. Until then, use project quality criteria and official documentation; do not manufacture replacement skills or empty skill folders.

Sources: [Codex instructions](https://learn.chatgpt.com/docs/agent-configuration/agents-md), [Codex subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents), [Codex skills](https://learn.chatgpt.com/docs/build-skills), [Claude memory](https://code.claude.com/docs/en/memory), [Claude skills](https://code.claude.com/docs/en/skills).
