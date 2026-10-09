# Working on Echocord

Start with `docs/STATE.md`, `tasks/BOARD.md`, and your assigned task. Read `docs/BRIEF.md` before changing product behavior. If `.trash/LOCAL.md` exists, read it for private local context without copying its contents into tracked files. User corrections override older documents; reconcile the documents when decisions change. Keep accepted requirements separate from proposals and measured results.

## Product and environment

- MVP: one Windows recording device (VB-Cable), one Discord bot destination, no local monitoring. Preserve audio isolation.
- Source, dependencies, builds, and agent worktrees stay in WSL. Keep Windows additions small and runtime-focused. Do not introduce a Windows source checkout or full development toolchain as an assumed prerequisite.
- Rust owns audio and recovery. The UI is a control surface; do not move sample buffers through frontend IPC.
- Keep the committed tree public-ready. Store personal notes, machine inventory, local paths, session transcripts, credentials, and raw test evidence in ignored `.trash/`. Publish only generic requirements and sanitized evidence. Never force-add private files or treat `.trash/` as permission to delete its contents. Ignoring a file does not remove it from existing history; review staged changes and commit identity before committing.

## Coordinated work

Use an orchestrator with delegated implementation and review for independent, bounded tasks. Model selection is a local harness preference; keep account-specific settings and thread identifiers out of Git.

One coordinator owns `tasks/BOARD.md` and integration. Each worker gets a task ID, dependencies, acceptance criteria, a branch/worktree, and an explicit file scope. Read `docs/WORKFLOW.md` before dispatching workers. Shared manifests, lockfiles, and IPC contracts need a designated owner. Never assume a spawned agent has an isolated checkout; verify its working directory.

Checkpoint task evidence and next actions in Git before handoff. A chat summary or running agent is not durable project state. After interruption, inspect worktrees and running processes before retrying or reassigning work.

## Validation and design

Read `docs/QUALITY.md` for applicable gates. Report checks actually run and limitations. Mark tasks done only after integration and relevant checks; Linux checks do not prove Windows audio behavior.

Use only researched existing skills after the stack is validated. Record source, pinned revision, license, maintenance/adoption evidence, and applicability before importing a skill. Do not generate placeholder skills or copy third-party material without checking its license. UI acceptance belongs in `docs/QUALITY.md` until suitable skills are selected. Keep this file short; task-specific detail belongs in the task or relevant document.

For UI work, read the vetted `.agents/skills/frontend-design/SKILL.md`; for browser review/testing, read `.agents/skills/playwright-cli/SKILL.md`. Pins, licenses, applicability limits, and private artifact handling are in `.agents/SOURCES.md`. Project desktop/privacy requirements take precedence over generic skill advice.
