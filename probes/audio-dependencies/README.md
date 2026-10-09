# Windows audio dependency probe

This offline executable enumerates recording endpoints, transfers one bounded sample, encodes and decodes a synthetic stereo frame with native Opus, prepares raw f32 PCM through Songbird, creates a DAVE key package, and initializes Songbird's driver/Serenity integration. It opens no recording stream and connects to no Discord account.

From the repository root:

```sh
bash scripts/probe-audio.sh
bash scripts/probe-audio.sh --run
bash scripts/probe-audio.sh --lint
```

The build runs in WSL; `--run` executes the Windows binary using host interop. Install the WSL tools in `docs/DEVELOPMENT.md`, plus Python 3.12+ and `patch`. The script prepares the pinned [Opus compatibility patch](../../patches/README.md) before invoking Cargo with the committed lockfile.

For intentional lockfile updates, run `python3 scripts/prepare-opus.py` first, then Cargo against this manifest. `--lint` checks formatting and runs Windows-target Clippy with warnings denied. Native Linux CPAL checks would need an additional Linux audio backend.

The executable defaults to counts and generic check results. Its optional `--list-devices` flag emits endpoint descriptions and format information; redirect that output to ignored `.trash/`. Never commit that machine-specific output.

Passing this probe establishes linkage and offline initialization, not live Discord encryption transitions, capture quality, clock drift, reconnect behavior, or latency. Those require the subsequent streaming task.
