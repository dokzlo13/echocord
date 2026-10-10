#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
app_target_dir="$(realpath -m -- "${ECHOCORD_APP_TARGET_DIR:-$repo_root/target}")"
check_app=false
if [[ "${1:-}" == --check ]]; then
  check_app=true
  shift
fi
if (( $# != 0 )); then
  echo 'Usage: bash scripts/build-app.sh [--check]' >&2
  exit 2
fi
export PATH="$HOME/.local/bin:$PATH"
for dependency in cargo cargo-xwin clang lld-link llvm-rc cmake ninja python3 patch node npm; do
  command -v "$dependency" >/dev/null || { echo "Missing WSL tool: $dependency" >&2; exit 1; }
done
if ! command -v clang-cl >/dev/null; then
  mkdir -p "$repo_root/.tools/bin"
  ln -sfn "$(command -v clang)" "$repo_root/.tools/bin/clang-cl"
  export PATH="$repo_root/.tools/bin:$PATH"
fi
export XWIN_CACHE_DIR="$repo_root/.cache/xwin"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-Ctarget-feature=-crt-static"
python3 scripts/prepare-opus.py
cargo run --locked -p echocord-core --bin export-bindings -- --check ui/src/generated.ts
npm --prefix ui run build
node scripts/build-app-icon.mjs
if [[ "$check_app" == true ]]; then
  exec cargo xwin clippy --locked --release -j 2 --target x86_64-pc-windows-msvc \
    --target-dir "$app_target_dir" -p echocord --bin echocord -- -D warnings
fi
cargo xwin build --locked --release -j 2 --target x86_64-pc-windows-msvc \
  --target-dir "$app_target_dir" -p echocord --bin echocord
