#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
case "${1:-}" in
  process-probe|capture-probe|engine-probe|storage-probe) tool="$1" ;;
  *) echo 'Usage: bash scripts/build-native-tool.sh process-probe|capture-probe|engine-probe|storage-probe' >&2; exit 2 ;;
esac
if (( $# != 1 )); then exit 2; fi
export PATH="$repo_root/.tools/bin:$HOME/.local/bin:$PATH"
if ! command -v clang-cl >/dev/null; then
  mkdir -p "$repo_root/.tools/bin"
  ln -sfn "$(command -v clang)" "$repo_root/.tools/bin/clang-cl"
fi
export XWIN_CACHE_DIR="$repo_root/.cache/xwin"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-Ctarget-feature=-crt-static"
python3 scripts/prepare-opus.py
cargo xwin build --locked --release -j 2 --target x86_64-pc-windows-msvc \
  --target-dir "$repo_root/target" -p echocord-core --bin "$tool"
