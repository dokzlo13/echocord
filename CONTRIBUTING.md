# Contributing to Echocord

Contributions from people and coding agents are welcome: bug reports, documentation, tests, accessibility improvements, build fixes, and focused code changes all help.

Start with the [development guide](docs/DEVELOPMENT.md). A fresh clone and the documented tools are enough; no personal setup or private project history is required. Linux/WSL can run portable checks and cross-build Windows. Native audio and desktop validation require Windows.

## Before a change

Search [issues](https://github.com/dokzlo13/echocord/issues) and open a discussion in an issue for substantial product or architecture changes. Describe the problem, the proposed behavior, and how it will be tested. Small, scoped fixes can go straight to a pull request.

Read [architecture](docs/ARCHITECTURE.md), [testing](docs/TESTING.md), and [DESIGN.md](DESIGN.md) for interface work. Keep sample buffers and recovery in Rust. Preserve explicit source/output selection, independent controls, and idle output restoration. Do not add broader capture or automatic voice joining as a convenience fallback.

## Build and check

```sh
python scripts/dev.py setup --browser
python scripts/dev.py check
python scripts/dev.py build
```

Use `python3` on Linux/WSL. For a UI/core-only contribution, `check --portable` skips Windows compilation; explain that limit in the PR. Run relevant focused tests during development, then the applicable combined checks before submission. Never use a real bot token in CI or fixtures.

## Pull requests

Keep each change reviewable. Explain the user-visible problem and resulting behavior, the checks actually run, and any remaining limitations. UI changes should include a small screenshot set using generic fixture data. Build/release changes should exercise the same commands that contributors and CI use.

Do not commit credentials, recordings, account identifiers, machine paths, local profiles, or generated build outputs. Preserve existing license notices; a new dependency needs a compatible license and release-notice coverage. Update documentation and the Unreleased changelog when behavior or commands change.

Code is reviewed regardless of whether it was written manually or with AI assistance. Be candid about test coverage and uncertainty. Avoid unrelated rewrites or speculative fixes in a focused PR.

## Releases

Maintainers follow [RELEASING.md](docs/RELEASING.md). Versioned tags trigger checked Windows release assets; published tags and artifacts are not replaced in place. Coding-agent contribution and release rules are in [AGENTS.md](AGENTS.md).
