#!/usr/bin/env bash
# Build the isolated desktop probe in WSL; --run also checks the Windows lifecycle.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
run_probe=false
check_probe=false
for argument in "$@"; do
  case "$argument" in
    --run) run_probe=true ;;
    --check) check_probe=true ;;
    *) echo 'Usage: bash scripts/probe-desktop.sh [--check] [--run]' >&2; exit 2 ;;
  esac
done
export PATH="$HOME/.local/bin:$PATH"
for dependency in cargo cargo-xwin clang lld-link llvm-rc llvm-readobj node npm; do
  command -v "$dependency" >/dev/null || { echo "Missing WSL tool: $dependency" >&2; exit 1; }
done
if ! command -v clang-cl >/dev/null; then
  mkdir -p "$repo_root/.tools/bin"
  ln -sfn "$(command -v clang)" "$repo_root/.tools/bin/clang-cl"
  export PATH="$repo_root/.tools/bin:$PATH"
fi
common_dir="$(git rev-parse --path-format=absolute --git-common-dir)"
export XWIN_CACHE_DIR="${XWIN_CACHE_DIR:-$(dirname -- "$common_dir")/.cache/xwin}"
export CARGO_TARGET_DIR="$repo_root/probes/desktop/src-tauri/target"
export CARGO_BUILD_JOBS="${ECHOCORD_DESKTOP_JOBS:-2}"
case "$CARGO_BUILD_JOBS" in
  1|2|3) ;;
  *) echo 'ECHOCORD_DESKTOP_JOBS must be 1, 2, or 3.' >&2; exit 2 ;;
esac
probe_dir="$repo_root/probes/desktop"
manifest="$probe_dir/src-tauri/Cargo.toml"
evidence_dir="$repo_root/.trash/desktop"
mkdir -p "$evidence_dir"
npm --prefix "$probe_dir" ci --ignore-scripts --no-audit --no-fund
npm --prefix "$probe_dir" run build
if [[ "$check_probe" == true ]]; then
  cargo +1.99.0 fmt --manifest-path "$manifest" -- --check
  cargo +1.99.0 xwin clippy --locked --release --target x86_64-pc-windows-msvc \
    --manifest-path "$manifest" -- -D warnings > "$evidence_dir/clippy.log" 2>&1 || {
      echo 'Windows-target Clippy failed; see .trash/desktop/clippy.log.' >&2; exit 1;
    }
fi
echo "Building Windows MSVC desktop probe in WSL ($CARGO_BUILD_JOBS jobs)…"
cargo +1.99.0 xwin build --locked --release --target x86_64-pc-windows-msvc \
  --manifest-path "$manifest" > "$evidence_dir/build.log" 2>&1 || {
    echo 'Desktop build failed; see .trash/desktop/build.log.' >&2; exit 1;
  }
executable="$CARGO_TARGET_DIR/x86_64-pc-windows-msvc/release/echocord-desktop-probe.exe"
llvm-readobj --coff-imports "$executable" > "$evidence_dir/pe-imports.log"
sed -n 's/^  Name: / /p' "$evidence_dir/pe-imports.log" | LC_ALL=C sort -fu > "$evidence_dir/runtime-dlls.txt"
echo 'Linked PE import DLLs (not a transitive or dynamically loaded module inventory):'
cat "$evidence_dir/runtime-dlls.txt"
if [[ "$run_probe" == true ]]; then
  echo 'Launching the unpackaged executable through WSL interoperability…'
  "$executable" > "$evidence_dir/windows-run.log" 2>&1 || {
    echo 'Windows probe failed; see .trash/desktop/windows-run.log.' >&2; exit 1;
  }
  node "$probe_dir/verify-run.mjs" "$evidence_dir/windows-run.log"
fi
