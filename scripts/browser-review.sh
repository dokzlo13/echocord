#!/usr/bin/env bash
# Use the pinned project browser tooling with all session artifacts kept private.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="$repo_root/ui/node_modules/playwright/cli.js"
if [[ ! -f "$runtime" ]]; then
  echo 'Install the pinned UI dependencies with make ui-install first.' >&2
  exit 1
fi
mkdir -p "$repo_root/.trash/browser-review"
cd "$repo_root/.trash/browser-review"
exec node "$runtime" cli "$@"
