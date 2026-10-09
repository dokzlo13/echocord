# WSL development environment

## Boundary

Keep the authoritative checkout on the WSL Linux filesystem. Keep dependencies, SDK caches, build output, and agent worktrees there. Use the Windows host to run the produced executable, access WASAPI/VB-Cable, and exercise the native UI. Do not create a second Windows source checkout.

For the Tauri runtime, verify existing WebView2 before installing anything; copy an executable and proven runtime dependencies to a small staging directory only if direct WSL launch proves insufficient. No Windows Rust, Node, Visual Studio, or CMake installation is planned.

## Project tool versions

The initial build recipe uses Rust 1.99.0 with rustfmt, Clippy, and x86_64-pc-windows-msvc support; cargo-xwin 0.23.1, CMake 4.4.4, and Ninja 1.13.2 via uv; LLVM 18; and Node 24.11.1. These are project tool versions, not a machine inventory or claim to be latest. Keep host-specific inventory in ignored `.trash/`.

Reproduce WSL tooling:

```sh
rustup toolchain install 1.99.0 --profile minimal --component rustfmt --component clippy --target x86_64-pc-windows-msvc
uv tool install cargo-xwin==0.23.1
uv tool install cmake==4.4.4
uv tool install ninja==1.13.2
```

Install Clang, LLD, and LLVM tools from suitable distro packages matching the validated LLVM version. CMake/Ninja matter for native dependencies even though the generic Rust probe does not use them.

## Build proof

```sh
bash scripts/probe-windows.sh
bash scripts/probe-windows.sh --run
```

The script uses a project-local clang-cl alias when Ubuntu only exposes clang, keeps downloaded SDK/CRT files in ignored `.cache/xwin`, and produces an executable under the probe's ignored target directory. The SDK remains in WSL. The run option invokes the Windows executable through WSL interoperability.

The initial probe only verifies Rust/MSVC cross-linking and launch. EC-002 must still establish CPAL/Songbird/Opus/DAVE/Tauri compatibility and actual runtime DLL needs. Avoid treating Wine or a browser mock as validation of Windows audio.

cargo-xwin documents cross-compilation and WSL execution, including CMake/Ninja support. Tauri documents a cross-compilation route but cautions that it is less tested than native builds. This project investigates that route to keep development in WSL. Start with an executable; NSIS packaging can follow, and MSI packaging is not a prerequisite.

Sources: [cargo-xwin](https://github.com/rust-cross/cargo-xwin), [Tauri Windows build](https://v2.tauri.app/distribute/windows-installer/), [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Secrets and live tests

Use a locally configured bot only during coordinated tests, with other clients using that bot stopped. Enter the token locally into a secret store or ignored runtime mechanism; do not paste it into chat, commits, or test logs. CI/static validation should not require the bot token or live Discord. Keep raw logs, recordings, original notes, local paths, machine inventory, and account details under ignored `.trash/`; only sanitized generic evidence belongs in Git. Gitignore is an exclusion mechanism, not encryption or backup. Remote backup/CI hosting is a separate configuration decision.
