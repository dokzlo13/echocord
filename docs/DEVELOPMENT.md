# Development

Echocord builds a Windows x64 application. You can contribute from native Windows, Linux, or WSL using the same repository and Python entry point. Linux/WSL cross-compilation produces a Windows executable; it does not create a Linux audio application.

## Tools

| Tool | Requirement | Purpose |
| --- | --- | --- |
| Git | Current supported release | Clone and contribute |
| Python | 3.12 or newer, with venv/pip | Bootstrap, verification, packaging |
| Rustup | [Official installer](https://rustup.rs/) | Installs the exact Rust toolchain/target in `rust-toolchain.toml` |
| Node.js | Version in [`.node-version`](../.node-version), currently24.11.1 | Locked frontend dependencies, tests, assets |
| Windows C++ tools | MSVC x64 C++ Build Tools + Windows SDK | Native Windows compilation and bundled Opus |
| Linux C++ tools | C/C++ build essentials, Clang/LLD/LLVM | Portable tests and Windows cross-compilation |

The repository pins Rust 1.99.0 and the Windows MSVC target. Bootstrap creates a repository-local Python tool environment containing pinned CMake 4.4.4 and Ninja 1.13.2; Linux/WSL also gets cargo-xwin0.23.1. These are build tools, not application runtime requirements.

### Native Windows

Install Python, [Node 24.11.1](https://nodejs.org/dist/v24.11.1/), Git, and Rustup. Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) with **Desktop development with C++**, including the **MSVC x64/x86 build tools** and a **Windows SDK**. The Visual Studio IDE itself is not required. The build script discovers the installed Visual Studio environment; an ordinary PowerShell terminal is sufficient.

The application also needs [WebView2 Evergreen Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) and the [Visual C++ v14 Redistributable x64](https://aka.ms/vc14/vc_redist.x64.exe) when run. Microsoft/Tauri's [Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows) explain the platform setup.

```powershell
git clone https://github.com/dokzlo13/echocord.git
cd echocord
python scripts/dev.py setup --browser
python scripts/dev.py check
python scripts/dev.py build
.\target\x86_64-pc-windows-msvc\release\echocord.exe
```

If your installation exposes only the Python launcher, use `py -3` in place of `python` and make sure it selects Python 3.12 or newer.

### Linux and WSL

Use a recent distribution with Python 3.12+. For Ubuntu 24.04 or newer:

```sh
sudo apt-get update
sudo apt-get install -y git curl build-essential clang lld llvm pkg-config python3 python3-venv
```

Install Rustup and the Node version in `.node-version`, using the official installers or a version manager. Then:

```sh
git clone https://github.com/dokzlo13/echocord.git
cd echocord
python3 scripts/dev.py setup --browser
python3 scripts/dev.py check
python3 scripts/dev.py build
```

cargo-xwin obtains the Windows SDK/import libraries and cross-compiles with Clang/LLD; a Windows compiler installation is not needed for this path. Downloaded Microsoft SDK components remain subject to their upstream license terms. See [cargo-xwin](https://github.com/rust-cross/cargo-xwin) for details.

In WSL, keep the checkout and dependency/build caches on the Linux filesystem for build performance. The resulting EXE can run on the Windows host. On ordinary Linux, copy the resulting release package to a supported Windows machine for runtime testing. WASAPI, the tray, DPAPI, and actual voice/audio behavior cannot be validated by Linux portable tests alone.

## Daily commands

Run `python scripts/dev.py --help` for the complete command list; substitute `python3` on Linux/WSL.

| Command | Purpose |
| --- | --- |
| `python scripts/dev.py setup --browser` | Prepare verified dependencies and test browser |
| `python scripts/dev.py check` | Portable checks plus Windows compilation checks |
| `python scripts/dev.py check --portable` | Core/UI checks without Windows compilation |
| `python scripts/dev.py build` | Build the Windows release executable with embedded licenses and a packaging receipt |
| `python scripts/dev.py app-test` | Run application unit tests on native Windows |
| `npm --prefix ui run dev` | Start the isolated fixture UI preview |
| `python scripts/dev.py version` | Verify versions agree across release metadata |
| `python scripts/package-third-party.py` | Generate release licenses and corresponding sources |
| `python scripts/dev.py package` | Package the checked EXE, notices, and checksums |

Do not run frontend builds and app builds concurrently in the same checkout: both update `ui/dist`. Parallel workers should use isolated checkouts/targets and distinct preview ports. The Makefile and shell wrappers are conveniences around the same build entry point.

On Linux/WSL, for UI/core work without a Windows cross-build toolchain, install Rustup, Node, Python and a native C linker, run `python scripts/prepare-opus.py`, `python scripts/prepare-cpal.py`, `npm --prefix ui ci`, and `python scripts/dev.py browser-install`, then `check --portable`. Native Windows core tests still require MSVC even with `--portable`. On Linux, Playwright may also need distribution browser libraries; use its documented `install --with-deps chromium` setup on a suitable development machine.

## Dependencies and generated files

Commit Cargo/npm lockfiles and use locked installs. `scripts/prepare-opus.py` verifies/materializes the `libopus_sys` cross-build patch; `scripts/prepare-cpal.py` does the same for the Windows silent-capture correction. Bootstrap/check/build prepares both. Never edit a global Cargo registry cache. Patching is Python-only, so Unix `patch` is not a Windows prerequisite. [Patch notes](../patches/README.md) explain scope and removal conditions.

Rust control types generate `ui/src/generated.ts`; regenerate through the export-bindings command when changing the contract, then run the drift check. Frontend fixture data must remain separate from the production native adapter. Audio samples never pass through frontend IPC.

Release builds remap source paths and avoid distributing personal machine paths. The application build generates full notices and corresponding sources, embeds an offline license catalog, and records a receipt binding the executable to that inventory and the lockfiles. Packaging rejects a stale or missing receipt; see [RELEASING.md](RELEASING.md). Plain frontend previews use a clearly labeled license placeholder. Build outputs, dependency caches, browser artifacts, and local runtime profiles are not source files.

## Runtime configuration and tests

A normal Windows launch stores encrypted credentials and versioned preferences under `%LOCALAPPDATA%\Echocord\data`. Development and CI do not need a bot token. Use a separate disposable data directory for native fixtures; the app accepts `--data-dir`, `--settings-file`, and `--token-file` overrides. Plaintext credential files are not accepted as protected storage.

Do not test against somebody's running session or personal browser profile. Use owned synthetic sources and explicit devices for audio checks, and coordinate any real Discord join. Read [TESTING.md](TESTING.md) for the validation boundaries.
