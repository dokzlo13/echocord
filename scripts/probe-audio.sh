#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
run_probe=false
lint_probe=false
case "${1:-}" in
  --run) run_probe=true; shift ;;
  --lint) lint_probe=true; shift ;;
esac
if (( $# != 0 )); then
  echo 'Usage: bash scripts/probe-audio.sh [--run|--lint]' >&2
  exit 2
fi
export PATH="$HOME/.local/bin:$PATH"
for dependency in cargo cargo-xwin clang lld-link llvm-rc cmake ninja python3 patch; do
  command -v "$dependency" >/dev/null || { echo "Missing WSL tool: $dependency" >&2; exit 1; }
done
if ! command -v clang-cl >/dev/null; then
  mkdir -p "$repo_root/.tools/bin"
  ln -sfn "$(command -v clang)" "$repo_root/.tools/bin/clang-cl"
  export PATH="$repo_root/.tools/bin:$PATH"
fi
export XWIN_CACHE_DIR="$repo_root/.cache/xwin"
target_dir="$repo_root/probes/audio-dependencies/target"
python3 "$repo_root/scripts/prepare-opus.py"
if [[ "$lint_probe" == true ]]; then
  python3 "$repo_root/scripts/test_prepare_opus.py"
  cargo fmt --manifest-path "$repo_root/probes/audio-dependencies/Cargo.toml" --check
  exec cargo xwin clippy --locked --release -j 3 --target x86_64-pc-windows-msvc \
    --target-dir "$target_dir" \
    --manifest-path "$repo_root/probes/audio-dependencies/Cargo.toml" -- -D warnings
fi
cargo xwin build --locked --release -j 3 --target x86_64-pc-windows-msvc \
  --target-dir "$target_dir" \
  --manifest-path "$repo_root/probes/audio-dependencies/Cargo.toml"
if [[ "$run_probe" == true ]]; then
  "$target_dir/x86_64-pc-windows-msvc/release/echocord-audio-dependency-probe.exe"
fi
