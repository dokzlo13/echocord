# Changelog

Notable user-facing changes are recorded here. Versions follow [Semantic Versioning](https://semver.org/); while the project is below 1.0, minor releases may change behavior or compatibility.

## Unreleased

## 0.1.0 — 2026-10-11

First public release.

### Added

- Capture selected Windows application process trees and recording inputs into one native shared mix.
- Mix up to eight sources into four independently controlled destinations: Discord rooms in different servers and optional Windows playback devices.
- Per-card capture/output controls, mute, precise dB gain, measured Simple/History meters, and targeted recovery/errors.
- Encrypted bot-token storage, guided bot setup, explicit room selection, saved setup with idle outputs, and tray controls.
- Compact resizable interface, independent card-list scrolling, matching progressive card/jump numbering, and reusable source colours.
- Native Windows and Linux/WSL cross-build bootstrap, portable checks, CI, versioned Windows ZIP releases, checksums, and third-party notices/source bundles.

### Limitations

- Windows 11 x64 runtime; Linux and macOS audio runtimes are not shipped.
- One bot identity, one voice room per server, and ordinary voice rooms only; no Discord audio input or Stage-channel support.
- Unsigned early builds requiring Microsoft WebView2 and the Visual C++ v14 x64 runtime.
- Physical audio delivery, device/network/sleep recovery, broad accessibility/scaling, latency, and long-session behavior need validation on each setup; local meters do not prove listener receipt.
