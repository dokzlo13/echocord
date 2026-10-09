# Vetted upstream skills

Skill files are imported from real upstream projects at immutable revisions, not generated locally. Project requirements override generic website/mobile recommendations. Review scope and tools before using a skill. Both imported directories retain upstream instructions unchanged.

| Skill | Source revision | License | Selection evidence |
| --- | --- | --- | --- |
| frontend-design | [anthropics/skills](https://github.com/anthropics/skills/tree/9d630808e4add0a7146de4af9384155d5dee350a/skills/frontend-design), `9d630808e4add0a7146de4af9384155d5dee350a` | Apache-2.0, included LICENSE.txt | Subject-specific design, token plan, restraint, screenshot critique. Maintained official repository; [design update PR](https://github.com/anthropics/skills/pull/1713) has recorded peer approvals. |
| playwright-cli | [microsoft/playwright-cli](https://github.com/microsoft/playwright-cli/tree/b85c7a736bb473bf55b584e54a09ffa698d6d871/skills/playwright-cli), `b85c7a736bb473bf55b584e54a09ffa698d6d871` | Apache-2.0; root upstream LICENSE copied alongside the skill | Official maintained browser tooling, keyboard/screenshot/test workflows; [recent skill change](https://github.com/microsoft/playwright-cli/pull/472). Use the already pinned Playwright 1.64.0 CLI capability rather than adding an alpha or unpinned global runtime. |

Review date: 2026-10-09. Repository reputation and adoption are signals, not per-skill quality ratings. The skills registry reports substantial adoption for both candidates; it also displays unresolved scanner warnings for the Microsoft listing whose report bodies were unavailable during research. Imported instructions are human-reviewed text; runtime evaluation uses the project's existing locked Playwright package. Do not describe this as an independent security certification.

## Project applicability

Use frontend-design to refine a compact Windows utility: source/destination clarity, readable type, restrained hierarchy, and immediately available mute. Do not add a hero, a marketing page, remote font dependency, framework replacement, or decorative motion solely because general web guidance suggests one.

Use playwright-cli for the local fixture preview, explicit screenshots, semantic control interaction, and keyboard checks. Launch an isolated browser; keep snapshots, screenshots, traces, profiles, and storage under ignored `.trash/`. `scripts/browser-review.sh` runs the locked project CLI from that directory. Do not attach to personal browser profiles, export authentication storage, upload artifacts, or substitute WebMCP calls for user-facing control verification. Desktop sizes take precedence over the skill's optional mobile preference. Browser checks do not establish native WebView/tray/audio behavior.

Codex discovers `.agents/skills`. `.claude/skills` links to the same directory in the WSL checkout, and CLAUDE.md imports shared rules. A fresh harness session should verify discovery; direct skill paths remain usable immediately.

## Deferred candidates and references

- OpenAI curated playwright skills: inspected catalog is deprecated; one wrapper fetches an unpinned CLI, while the interactive variant requires a persistent JS tool not guaranteed across harnesses. Existing locked tooling is sufficient.
- Anthropic webapp-testing: adds a second Python browser workflow and uses readiness advice that does not fit the current Playwright tests.
- Vercel React guidance: retain [selective references](https://github.com/vercel-labs/agent-skills/tree/063bee94c3f4df8453406c830b0a7df0f2860278/skills/react-best-practices); much of its server/Next.js guidance is irrelevant. Defer vendoring pending complete redistribution notices ([upstream issue](https://github.com/vercel-labs/agent-skills/issues/334)).
- Anthropic web-artifacts-builder: targets a different framework/style stack and an HTML artifact rather than this desktop application.

Useful primary documentation: [Mantine theme](https://mantine.dev/theming/theme-object/), [Mantine source 9.7.1](https://github.com/mantinedev/mantine/tree/9.7.1), [Playwright visual comparisons](https://playwright.dev/docs/test-snapshots), [Playwright source v1.64.0](https://github.com/microsoft/playwright/tree/v1.64.0), [Tauri desktop tests](https://v2.tauri.app/develop/tests/), [Windows typography](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/typography), and [Windows text scaling](https://learn.microsoft.com/en-us/windows/apps/develop/input/text-scaling). Preserve links and targeted integration notes rather than copying entire documentation sites.
