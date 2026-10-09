#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
executable="$repo_root/target/x86_64-pc-windows-msvc/release/echocord.exe"
if [[ ! -f "$executable" ]]; then
  echo 'Build the application with make app-build first.' >&2
  exit 1
fi
mkdir -p "$repo_root/.trash/app"
arguments=(--data-dir "$(wslpath -w "$repo_root/.trash/app")")
token_file="${ECHOCORD_TOKEN_FILE:-$repo_root/.trash/secrets/discord-token}"
if [[ -f "$token_file" ]]; then
  arguments+=(--token-file "$(wslpath -w "$(realpath "$token_file")")")
fi
smoke_report=""
if [[ "${1:-}" == --smoke ]]; then
  shift
  smoke_report="$repo_root/.trash/app/smoke-report-$(date +%s%N).txt"
  arguments+=(--smoke-report "$(wslpath -w "$smoke_report")")
fi
if (( $# != 0 )); then
  echo 'Usage: bash scripts/run-app.sh [--smoke]' >&2
  exit 2
fi
if [[ -n "$smoke_report" ]]; then
  "$executable" "${arguments[@]}"
  python3 - "$smoke_report" <<'PY'
from pathlib import Path
import sys
report = Path(sys.argv[1]).read_text().strip()
if report != "native-ui-ipc-tray-lifecycle: passed":
    raise SystemExit("Native app smoke failed; inspect the private report")
print(report)
PY
else
  exec "$executable" "${arguments[@]}"
fi
