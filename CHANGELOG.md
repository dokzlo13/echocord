# Changelog

Notable user-facing changes are recorded here. Versions follow [Semantic Versioning](https://semver.org/); while the project is below 1.0, minor releases may change behavior or compatibility.

## Unreleased

## 0.2.0 — 2026-10-16

### Added

- Recording inputs can use 44.1, 48 or 96 kHz mono/stereo PCM. Conversion to the 48 kHz mix is automatic; existing 48 kHz inputs keep their direct path.
- Persistent notice and save retry when live changes apply but preferences cannot be saved.
- Offline searchable licenses in Settings, with full dependency notices and corresponding-source information embedded in the app.
- Explicit settings migration steps, historical-schema fixtures, synthetic audio conversion/clock tests and a repeatable Windows acceptance protocol.

### Fixed

- Validate final migrated source/output limits, gains and identities before restoring a setup.
- Prevent restored card identifiers near the numeric limit from overflowing or colliding with configured/retiring cards.
- Preserve files from newer settings schemas before attempting current-schema loading; default missing additive settings without resetting existing choices.
- Preserve stereo alignment and discard stale filter history across mute/discontinuity changes.

### Distribution

- Download and run `echocord.exe` directly. Releases now attach only the executable, required third-party sources and `SHA256SUMS`; release notes stay on the release page.
- Packaging verifies that the executable was built with the current complete notice bundle and dependency locks.

### Limitations

- Windows 11 x64, unsigned; Microsoft WebView2 and Visual C++ v14 x64 runtimes are still required.
- Recording conversion is covered by synthetic signal, timing, allocation and Windows format-descriptor tests. Physical device compatibility, Discord listener delivery and multi-hour stability are separate manual checks, not guarantees from CI.
- Surround input, adaptive clock-drift correction, performance presets and a mix limiter remain deferred. A capture cleanup timeout still stops the shared audio worker safely.

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
