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
| `make probe-audio` / `make probe-desktop` | Cross-build isolated Windows audio or Tauri probe |
| `make check-audio` / `make check-desktop` | Rust formatting and Windows Clippy; audio also checks dependency-cache integrity |
| `make run-audio` | Run offline codec/crypto/device enumeration; no recording or Discord connection |
| `make run-desktop` | Run the self-closing Windows shell and verify native/frontend telemetry |
| `make smoke-windows` | Explicitly run both native probes |
| `make ui-dev` | Preview mocked controls on localhost |
| `make ui-build` | Typecheck and build the mock UI |
| `make ui-check` / `make ui-test` | Full UI gate or browser tests alone |

`make check` does not open a Windows window or connect to Discord. Native runtime checks are separate. Browser tests use fixtures, not credentials or real audio. Build scripts keep SDK/cache files and artifacts in WSL; run commands use host interop. The original toolchain-only probe remains available with `bash scripts/probe-windows.sh --run`.

The audio probe covers CPAL enumeration, native Opus encode/decode, Songbird PCM preparation, and offline DAVE key generation. The desktop probe covers embedded frontend execution, IPC, and native window lifecycle. They are separate executables; combined product integration, capture, live Discord, latency, and clean-machine packaging remain later checks. See each probe README for features, results, and limitations.

cargo-xwin documents cross-compilation and WSL execution, including CMake/Ninja support. Tauri documents a cross-compilation route but cautions that it is less tested than native builds. This project investigates that route to keep development in WSL. Start with an executable; NSIS packaging can follow, and MSI packaging is not a prerequisite.

Sources: [cargo-xwin](https://github.com/rust-cross/cargo-xwin), [Tauri Windows build](https://v2.tauri.app/distribute/windows-installer/), [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Secrets and live tests

Use a locally configured bot only during coordinated tests, with other clients using that bot stopped. Enter the token locally into a secret store or ignored runtime mechanism; do not paste it into chat, commits, or test logs. CI/static validation should not require the bot token or live Discord. Keep raw logs, recordings, original notes, local paths, machine inventory, and account details under ignored `.trash/`; only sanitized generic evidence belongs in Git. Gitignore is an exclusion mechanism, not encryption or backup. Remote backup/CI hosting is a separate configuration decision.
