# Initial technology survey

Research date: 2026-10-09. The audio dependency probe now cross-builds and runs on Windows; Tauri validation and real audio measurements remain separate. No streaming path has been implemented.

## Recommended first probe

| Component | Candidate | Rationale and qualification |
| --- | --- | --- |
| Device capture | CPAL 0.18.2 | The agreed MVP reads a normal recording device, unlike the earlier playback-loopback discussion. Start with the portable device API and negotiate actual format. |
| Windows-specific fallback | wasapi 0.25.0 | Explicit buffering and notifications; investigate if CPAL cannot meet measured recovery/latency needs or for later loopback features. |
| Discord voice | Songbird 0.6.0 | DAVE support; verify native Opus build and live join/rejoin behavior. |
| Gateway | Serenity 0.12 | Integrated Songbird setup; Twilight also works but needs event forwarding and version compatibility checking. |
| Buffer | rtrb 0.4.0 | Preallocated bounded SPSC transfer; never let the capture callback block on UI/network work. |
| Resampling | Rubato, exact version after API probe | Use only where needed for format/clock conversion. Current 5.x differs from Songbird's internal 0.16; do not mix example APIs. |
| Desktop | Tauri 2, React, TypeScript, Mantine, CSS Modules | Preferred project stack. Verify Windows cross-linking before building the UI around a presumed toolchain. |

Sources: [CPAL](https://docs.rs/cpal/0.18.2/cpal/), [wasapi](https://docs.rs/wasapi/0.25.0/wasapi/), [Songbird manifest](https://github.com/serenity-rs/songbird/blob/v0.6.0/Cargo.toml), [rtrb](https://docs.rs/rtrb/0.4.0/rtrb/), [Rubato](https://docs.rs/rubato/5.0.1/rubato/), [Mantine Vite](https://mantine.dev/guides/vite/).

## PCM integration and dependency risks

Candidate flow: capture → bounded queue → conversion/resampling → interleaved little-endian f32 at 48 kHz stereo → Songbird RawAdapter. Explicitly check Symphonia's PCM feature: avoiding a custom player does not mean Songbird has no decoding-framework dependency. Do not add FFmpeg or URL download helpers.

Temporary starvation must not appear as permanent EOF. Inspect Songbird's actual read-ahead and polling behavior; unbounded synthetic silence can also distort pacing if the consumer reads ahead. Measure sample age at relevant handoffs.

rtrb Consumer is Send but not Sync while the media-source abstraction requires Sync. Use a sound adapter; do not add an unsafe Sync implementation merely to satisfy bounds. A private Mutex with exclusive access via get_mut is one candidate to verify against the exact trait and consumption pattern.

Songbird 0.6 uses opus2 0.4 and libopus_sys ^0.3.2. The 0.3.3 bundled source failed under clang-cl on missing SSE4.1 flags, then an MSVC-only replacement block. A [narrow compatibility patch](../patches/README.md) fixes these and makes libm linkage target-aware. The patched probe builds native static Opus and passes a synthetic stereo encode/decode on Windows. Adding libopus_sys 0.4 separately would not replace the transitive ^0.3 dependency.

DAVE uses davey/OpenMLS in this stack. The probe creates a DAVE key package offline with davey 0.1.4, initializes Songbird's driver/Serenity manager, and prepares raw PCM. No C++ libdave was needed. The lockfile resolves Serenity 0.12.5 and Symphonia 0.5.5. No live encryption transition has been tested. DLL import inspection shows Windows system/UCRT dependencies and VCRUNTIME140.dll, with no separate Opus DLL; a clean-machine runtime packaging check remains necessary.

Sources: [RawAdapter source](https://github.com/serenity-rs/songbird/blob/v0.6.0/src/input/adapters/raw_adapter.rs), [Songbird configuration](https://github.com/serenity-rs/songbird/blob/v0.6.0/README.md), [Consumer traits](https://docs.rs/rtrb/0.4.0/rtrb/struct.Consumer.html), [Opus dependency](https://docs.rs/crate/opus2/0.4.0), [native build script](https://docs.rs/crate/libopus_sys/0.3.3/source/build.rs), [davey](https://github.com/Snazzah/davey/blob/master/davey/Cargo.toml), [Songbird release](https://github.com/serenity-rs/songbird/releases/tag/v0.6.0).

## Architecture boundaries

Separate capture adapters, bounded sample processing, destination adapters, and control state. Keep samples entirely in Rust. Initially implement one source and one destination; preserve room for a shared mix and independent destinations without implementing a routing graph/plugin system.

Clock mismatch can occur between capture hardware and transmission pacing. Resampling and queue regulation must be based on evidence, not an ever-growing buffer. Additional sources/destinations increase synchronization work.

Desktop loopback captures an endpoint's whole mix; application capture selects/excludes a process tree across endpoints. A new Echocord output endpoint requires virtual-device engineering. These are separate future source capabilities, not a promise that any playback device can simply exclude Discord.

Sources: [endpoint capture](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording), [process capture](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/), [virtual driver sample](https://learn.microsoft.com/en-us/samples/microsoft/windows-driver-samples/sysvad-virtual-audio-device-driver-sample/).
