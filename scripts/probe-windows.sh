#!/usr/bin/env bash
# Build-only by default. --run also executes the probe on the Windows host.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
run_probe=false
if [[ "${1:-}" == --run ]]; then
  run_probe=true
  shift
fi
if (( $# != 0 )); then
  echo 'Usage: bash scripts/probe-windows.sh [--run]' >&2
  exit 2
fi
export PATH="$HOME/.local/bin:$PATH"
for dependency in cargo cargo-xwin clang lld-link llvm-rc; do
  command -v "$dependency" >/dev/null || { echo "Missing WSL tool: $dependency" >&2; exit 1; }
done
# Ubuntu can ship clang without the clang-cl alias; argv[0] selects CL mode.
if ! command -v clang-cl >/dev/null; then
  mkdir -p "$repo_root/.tools/bin"
  ln -sfn "$(command -v clang)" "$repo_root/.tools/bin/clang-cl"
  export PATH="$repo_root/.tools/bin:$PATH"
fi
export XWIN_CACHE_DIR="$repo_root/.cache/xwin"
cargo xwin build --locked --release --target x86_64-pc-windows-msvc \
  --manifest-path "$repo_root/probes/windows-build/Cargo.toml"
if [[ "$run_probe" == true ]]; then
  "$repo_root/probes/windows-build/target/x86_64-pc-windows-msvc/release/echocord-build-probe.exe"
fi
