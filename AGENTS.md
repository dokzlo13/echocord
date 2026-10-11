# Working on Echocord

Read README.md and the relevant docs/DEVELOPMENT.md, docs/ARCHITECTURE.md, docs/TESTING.md and docs/RELEASING.md before changing their areas. Product behavior is described in PRODUCT.md; UI rules are in DESIGN.md. These tracked files and the code are the project context; no private files or particular agent harness are prerequisites.

## Product boundaries

Echocord streams explicitly selected local Windows audio to Discord as a bot, with optional selected playback outputs. Rust owns audio buffers, mixing, recovery and credentials. The frontend carries controls and scalar telemetry, never PCM. Preserve exact source identity, independent controls, one room per bot/server, bounded work and idle output restoration. No automatic room join, broader/default source fallback or unrelated routing change.

## Contributions

Use the same documented bootstrap/check/build commands as human contributors. Windows native and Linux/WSL cross-build paths are supported for development; native audio behavior requires Windows validation. Keep changes bounded and reviewable, update affected public docs/changelog, and report checks actually run plus limitations.

For substantial work, establish design and interfaces before implementation, then obtain independent review. Delegate independent bounded tasks with an explicit checkout/branch and file scope; spawning an agent does not isolate the filesystem. Designate one owner for shared manifests, lockfiles and IPC contracts. Integrate scoped commits linearly and retire accepted worktrees after verifying no intended changes remain. Never discard unidentified work.

For UI changes, manually read the vetted .agents/skills/frontend-design/SKILL.md and relevant .agents/skills/impeccable/SKILL.md guidance. For browser review, read .agents/skills/playwright-cli/SKILL.md. Follow .agents/SOURCES.md licenses/applicability; the compact desktop product and current task override generic website advice. Keep raw test artifacts untracked.

Do not read, print, commit or upload bot tokens or personal runtime data. Automated tests use fixture data and no Discord credentials. Preserve running app sessions; do not stop them or overwrite an in-use executable as housekeeping. Live capture, playback and Discord tests require explicit coordinated scope.

## Releases

Follow docs/RELEASING.md. Keep Cargo, Tauri and UI versions consistent; update CHANGELOG.md and verify the intended tag. Use checked CI-produced Windows assets with full dependency notices, corresponding sources and checksums. Never bypass a failed gate, publish a personal-machine binary containing private paths, force-move a published version tag, or omit licensing files. Publishing requires explicit maintainer authorization; ordinary implementation work does not authorize a release.
