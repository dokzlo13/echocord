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
| `make app-run` | Run the built app, initially disconnected |
| `make app-smoke` | Build and verify native UI/tray/IPC without voice joining |
| `make engine-smoke` | Exercise owned worker startup with synthetic credentials |
| `make ui-dev` | Local fixture preview for development |
| `make bindings` | Regenerate the TypeScript control contract |

`make build` builds the production Windows app; `make check` checks its core, generated types, frontend, and Windows integration without opening a window or connecting voice. Use `make app-run` for the real app, `make app-smoke` for native UI checks, and `make engine-smoke` for synthetic worker-startup checks. Browser tests use fixtures. Source, caches, and build artifacts remain in WSL. Retired prototypes and one-off dependency probes are preserved in Git history, not the working tree.

Production regression helpers remain available for focused native diagnostics. The combined app uses consistent dynamic CRT linking because bundled Opus requests that runtime. WebView2 and Visual C++ runtime availability remain distribution requirements. Native capture and combined startup have been checked; initial audible Discord delivery and perceived latency were user-confirmed; measured listener latency and long sessions remain open in [acceptance](FIRST-TEST.md).

cargo-xwin documents cross-compilation and WSL execution, including CMake/Ninja support. Tauri documents a cross-compilation route but cautions that it is less tested than native builds. This project investigates that route to keep development in WSL. Start with an executable; NSIS packaging can follow, and MSI packaging is not a prerequisite.

Sources: [cargo-xwin](https://github.com/rust-cross/cargo-xwin), [Tauri Windows build](https://v2.tauri.app/distribute/windows-installer/), [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Runtime storage

A normal executable launch uses `ProjectDirs` local application data: `%LOCALAPPDATA%\Echocord\data` on Windows. The token is encrypted for the current Windows user with DPAPI; settings are ordinary JSON. The setup backend validates the token before saving and never joins voice; the redesigned setup form is still pending. Rust reads tokens; frontend snapshots contain no secret.

`make app-run` uses those defaults. Set `ECHOCORD_DATA_DIR` to an ignored WSL directory for isolated development storage. `ECHOCORD_TOKEN_FILE` explicitly overrides the encrypted credential destination. Plaintext credential files are not accepted. The executable also accepts `--data-dir`, `--token-file`, and `--settings-file`. Keep all private paths and fixtures outside Git. Tests and probes use explicit private directories and synthetic values.

## Secrets and live tests

Use a locally configured bot only during coordinated tests, with other clients using that bot stopped. Enter the token locally into a secret store or ignored runtime mechanism; do not paste it into chat, commits, or test logs. CI/static validation should not require the bot token or live Discord. Keep raw logs, recordings, original notes, local paths, machine inventory, and account details under ignored `.trash/`; only sanitized generic evidence belongs in Git. Gitignore is an exclusion mechanism, not encryption or backup. Remote backup/CI hosting is a separate configuration decision.
