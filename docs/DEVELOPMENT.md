# WSL development environment

## Boundary

Keep the authoritative checkout on the WSL Linux filesystem. Keep dependencies, SDK caches, build output, and agent worktrees there. Use the Windows host to run the produced executable, access WASAPI/VB-Cable, and exercise the native UI. Do not create a second Windows source checkout.

For the Tauri runtime, verify existing WebView2 before installing anything; copy an executable and proven runtime dependencies to a small staging directory only if direct WSL launch proves insufficient. No Windows Rust, Node, Visual Studio, or CMake installation is planned.

## Project tool versions

The initial build recipe uses Rust 1.99.0 with rustfmt, Clippy, and x86_64-pc-windows-msvc support; cargo-xwin 0.23.1, CMake 4.4.4, and Ninja 1.13.2 via uv; LLVM 18; and Node 24.11.1. These are project tool versions, not a machine inventory or claim to be latest. Keep host-specific inventory in ignored `.trash/`.

Install Git, GNU Make, Python 3.12+, `patch`, rustup, uv, and the project's Node version first. Then install the pinned user-level WSL tools:

```sh
make setup-wsl
```

Install Clang, LLD, and LLVM tools from suitable distro packages matching the validated LLVM version. CMake/Ninja matter for native dependencies even though the generic Rust probe does not use them.

## Development commands

The root Makefile is the command entry point. Run `make` or `make help` to list targets. Missing prerequisites fail explicitly. Install UI dependencies and the browser before aggregate checks:

```sh
make ui-install
make ui-browser-install
make build
make check
```

| Command | Purpose |
| --- | --- |
| `make build` | Build the Windows app in WSL |
| `make check` | Production Rust, bindings, frontend, and Windows checks |
| `make app-run` | Run the built app; restore capture preferences but never auto-connect Discord |
| `make app-smoke` | Build and verify native UI/tray/IPC without voice joining |
| `make engine-smoke` | Exercise gateway/voice worker rejection and observed exit with synthetic private setup |
| `make ui-dev` | Local fixture preview for development |
| `make bindings` | Regenerate the TypeScript control contract |

`make build` builds the production Windows app; `make check` checks its core, generated types, frontend, and Windows integration without opening a window or connecting voice. Use `make app-run` for the real app, `make app-smoke` for native UI checks, and `make engine-smoke` for synthetic gateway/voice-worker rejection and observed-exit checks. App smoke sends no Connect and joins no voice, but normal startup restoration may capture saved recording inputs. Browser tests use fixtures. The frontend uses the adopted On Air source with native controls and self-hosted fonts; Mantine is retired. Worker previews/tests must use distinct ports, and owned previews stop before cleanup. Source, caches, and build artifacts remain in WSL. Retired prototypes and one-off dependency probes are preserved in Git history, not the working tree.

To stage a replacement while an older app is running, set `ECHOCORD_APP_TARGET_DIR` to a separate WSL directory for both `make app-build` and `make app-run`/`make app-smoke`. This explicitly redirects native app artifacts; the default remains the repository's `target/`. Keep the running executable intact until its session ends, then activate the reviewed replacement. The single-instance guard still applies, including smoke runs; a different target or data directory does not bypass it. Native probe commands retain their normal target.

Production regression helpers remain available for focused native diagnostics. The combined app uses consistent dynamic CRT linking because bundled Opus requests that runtime. WebView2 and Visual C++ runtime availability remain distribution requirements. Native capture and combined startup have been checked; earlier recording and application-to-Discord delivery with low perceived delay were owner-confirmed; live mixed delivery, measured listener latency and long sessions remain open in [acceptance](FIRST-TEST.md).

The `capture-probe` supports `--application-suite` for owned synthetic application/child/isolation checks and `--mixing-suite` for credential-free two-source/common-reader checks; build it with `bash scripts/build-native-tool.sh capture-probe`. The read-only `--playback-list` reports endpoint support, default roles and active sessions; keep that identifying inventory private. `--playback-suite ENDPOINT_ID` exercises owned signal/fan-out only on an explicitly selected non-default endpoint with no active session, rechecking before playback and never changing endpoint volume/routing. Its `--browser-suite` requires an explicitly dedicated disposable browser profile and two synthetic tone URLs; see EC-017 for the measurement boundaries. These are developer regression helpers, not app setup prerequisites. Keep raw output/profile data ignored and never use a personal browser profile for a fixture.

Concurrent worktrees should use separate portable Cargo target directories. Sharing unqualified executable outputs such as `target/debug/export-bindings` can run a different branch's cached generator even when Cargo considers the current branch fresh. Final integrated checks use the normal commands after worker builds have stopped.

cargo-xwin documents cross-compilation and WSL execution, including CMake/Ninja support. Tauri documents a cross-compilation route but cautions that it is less tested than native builds. This project investigates that route to keep development in WSL. Start with an executable; NSIS packaging can follow, and MSI packaging is not a prerequisite.

Sources: [cargo-xwin](https://github.com/rust-cross/cargo-xwin), [Tauri Windows build](https://v2.tauri.app/distribute/windows-installer/), [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Runtime storage

A normal executable launch uses `ProjectDirs` local application data: `%LOCALAPPDATA%\Echocord\data` on Windows. The token is encrypted for the current Windows user with DPAPI; versioned ordinary JSON stores source/destination cards, dB gain/mute and app preferences. The shell remembers safe window size in a separate sidecar. Prior recording/destination/output preferences migrate; future-version files are preserved. In-app Test & Save validates the token before encrypted persistence and never joins voice. Rust reads tokens; frontend snapshots contain no secret. Restore setup defaults on: recording cards reopen, app cards wait for explicit exact-identity attachment, and rooms stay idle. Turning restore off starts with empty cards. A source added through the picker captures before any destination connects. Keep running in tray defaults on; turning it off makes Close quit.

`make app-run` uses those defaults. Set `ECHOCORD_DATA_DIR` to an ignored WSL directory for isolated development storage. `ECHOCORD_TOKEN_FILE` explicitly overrides the encrypted credential destination. Plaintext credential files are not accepted. The executable also accepts `--data-dir`, `--token-file`, and `--settings-file`. Keep all private paths and fixtures outside Git. Tests and probes use explicit private directories and synthetic values.

## Secrets and live tests

Use a locally configured bot only during coordinated tests, with other clients using that bot stopped. Enter the token locally into a secret store or ignored runtime mechanism; do not paste it into chat, commits, or test logs. CI/static validation should not require the bot token or live Discord. Keep raw logs, recordings, original notes, local paths, machine inventory, and account details under ignored `.trash/`; only sanitized generic evidence belongs in Git. Gitignore is an exclusion mechanism, not encryption or backup. Remote backup/CI hosting is a separate configuration decision.
