# EC-002 desktop dependency probe

This isolated Tauri 2 executable proves WSL-built frontend assets and Windows
MSVC linkage, then exercises a native WebView window on the Windows host.
It is a dependency probe, with no audio capture or product tray behavior.
The separate UI task owns the React mock and product control contract.

## Reproduce

From the repository or this worker's WSL worktree:

```sh
bash scripts/probe-desktop.sh
bash scripts/probe-desktop.sh --check --run
```

Build-only is the default. `--check` adds Rust formatting and Windows-target
Clippy with warnings denied; `--run` launches the resulting unpackaged executable
through WSL interoperability and validates the ordered telemetry. Flags can be
combined. The frontend build uses only Node's standard library, with no npm
dependencies. `npm ci` uses the isolated package lockfile; Cargo uses `--locked`.
All asset generation, Rust compilation, resource compilation, and linking run
in WSL. No Windows development tools or packages are installed by this recipe.

The script creates a local `clang-cl` alias to `clang` if necessary, sets
`CARGO_TARGET_DIR` to `probes/desktop/src-tauri/target` in the current worktree,
and defaults `XWIN_CACHE_DIR` to `.cache/xwin` in the shared Git repository.
An existing `XWIN_CACHE_DIR` overrides that default. `CARGO_BUILD_JOBS` defaults
to 2; set `ECHOCORD_DESKTOP_JOBS` to 1, 2, or 3 to override it.
The executable is `probes/desktop/src-tauri/target/x86_64-pc-windows-msvc/release/echocord-desktop-probe.exe`.
Raw compiler/runtime logs and PE import output stay in ignored `.trash/desktop/`.
Generated frontend assets and Tauri schemas are also ignored.

For the coordinator's lint wrapper, build frontend assets first and use the
same cache, target-directory, job-limit, and alias environment as the script:

```sh
npm --prefix probes/desktop ci --ignore-scripts --no-audit --no-fund
npm --prefix probes/desktop run build
cargo +1.99.0 fmt --manifest-path probes/desktop/src-tauri/Cargo.toml -- --check
cargo +1.99.0 xwin clippy --locked --release --target x86_64-pc-windows-msvc \
  --manifest-path probes/desktop/src-tauri/Cargo.toml -- -D warnings
```

## Validated versions and features

Validated build tools: Rust 1.99.0, cargo-xwin 0.23.1, LLVM/Clang/LLD 18.1.3,
Node 24.11.1, npm 11.6.2. Rust is selected explicitly by the script, and the
frontend versions are recorded in `package.json` and its lockfile. The parent
development document owns shared tooling setup. This shell requires neither
CMake nor Ninja and does not use or patch the audio worker's dependencies.

| Dependency | Locked version | Selection |
| --- | --- | --- |
| tauri | 2.12.2 | Defaults disabled; `wry`, `compression`, `common-controls-v6`, `custom-protocol` |
| tauri-build | 2.7.1 | Defaults disabled; JSON configuration validated by the actual build |
| tauri-codegen / tauri-macros | 2.7.1 | Transitive, locked |
| tauri-runtime / tauri-runtime-wry | 2.12.1 | Transitive, locked |
| tauri-utils | 2.10.1 | Transitive, locked |
| wry | 0.57.0 | Windows WebView implementation |
| tao | 0.37.1 | Native event loop |
| webview2-com / webview2-com-sys | 0.39.1 | MSVC static WebView2 loader |
| windows | 0.62.2 | Transitive Windows bindings |
| embed-resource | 3.0.12 | WSL native resource compilation |

The configuration explicitly enables `build.windows.staticVCRuntime`.
Installer generation and signing are deferred. The frontend uses Tauri's
global `core.invoke`, two custom Rust commands, local embedded assets, and a
CSP allowing local assets and the IPC transport. No plugin or additional
frontend API capability is needed for these custom commands.

## Measured results, 2026-10-09

`bash scripts/probe-desktop.sh --check --run` completed successfully with two
build jobs. No user interaction or Windows package installation was needed.
Existing WebView2 was checked before launch.

| Check | Observed result |
| --- | --- |
| Frontend generation and JavaScript syntax | Passed in WSL; assets embedded in the executable |
| Rust formatting | Passed |
| Windows-target release Clippy, `-D warnings` | Passed |
| Windows MSVC release compile/link | Passed using cargo-xwin and the shared SDK cache |
| Native window creation | Rust reported visible, nonzero native window dimensions |
| Frontend execution | Embedded JavaScript called `frontend_ready` over IPC |
| IPC response delivery | JavaScript received the Rust acknowledgement and returned it in `frontend_complete` |
| Native closure | Close-requested and destroyed events observed in order |
| Runtime termination | Event loop exited, all proof flags were true, executable returned 0 |
| Process cleanup | Subsequent Windows process check found no desktop probe process |

The frontend waits for document load, two animation callbacks, and 1.5 seconds
before requesting closure. A 25-second Rust watchdog exits with code 3 if
startup or completion stalls. Closing the window before the round trip or
missing its destruction produces a nonzero exit. The verifier also requires
all lifecycle milestones in order; an executable merely launching is insufficient.

These are IPC and native lifecycle observations. No native window screenshot or
visual review was performed. Rendering quality, keyboard navigation, display
scaling, tray behavior, production UI IPC, installers, and audio remain outside
this result. Independent review and integration belong to the coordinator.

## Actual linked DLL inventory

`llvm-readobj --coff-imports` on the successful release executable reported
these 23 distinct import names (sanitized from the actual PE output):

```text
ADVAPI32.dll
api-ms-win-core-synch-l1-2-0.dll
api-ms-win-core-winrt-error-l1-1-0.dll
api-ms-win-crt-convert-l1-1-0.dll
api-ms-win-crt-heap-l1-1-0.dll
api-ms-win-crt-locale-l1-1-0.dll
api-ms-win-crt-math-l1-1-0.dll
api-ms-win-crt-runtime-l1-1-0.dll
api-ms-win-crt-stdio-l1-1-0.dll
api-ms-win-crt-string-l1-1-0.dll
bcryptprimitives.dll
combase.dll
comctl32.dll
dwmapi.dll
gdi32.dll
imm32.dll
kernel32.dll
ntdll.dll
ole32.dll
oleaut32.dll
shell32.dll
shlwapi.dll
user32.dll
```

There is no `VCRUNTIME140.dll` or `WebView2Loader.dll` import. This executable
ran directly from its WSL output directory without staging companion DLLs.
The WebView2 loader is statically linked for MSVC; the existing WebView2 runtime
is still required. This import list does not enumerate transitive dependencies,
dynamically loaded modules, or the WebView2 child process's own DLLs.

## Failures and limitations

The initial compiler pass rejected a non-exhaustive `RunEvent::WindowEvent`
pattern. Adding the required `..` fixed it. No Tauri dependency source patch or
Windows build fallback was required.

Successful release linking emits LNK4099 warnings because SDK/CRT libraries
reference PDB files absent from the xwin cache. Linkage and runtime succeeded,
but CRT debug-symbol information is incomplete. These warnings are preserved
in the private build log, not suppressed. Clippy's successful warnings-denied
check does not perform this final link. Microsoft describes
[LNK4099](https://learn.microsoft.com/en-us/cpp/error-messages/tool-errors/linker-tools-warning-lnk4099)
as missing debug information for the linked object/library.

Primary API/build references: [Tauri Windows cross-compilation](https://v2.tauri.app/distribute/windows-installer/),
[frontend Rust commands](https://v2.tauri.app/develop/calling-rust/),
[App::run_return](https://docs.rs/tauri/2.12.2/tauri/struct.App.html#method.run_return),
[WebviewWindow lifecycle APIs](https://docs.rs/tauri/2.12.2/tauri/webview/struct.WebviewWindow.html),
and [MSVC WebView2 loader source](https://docs.rs/webview2-com-sys/0.39.1/src/webview2_com_sys/lib.rs.html).
