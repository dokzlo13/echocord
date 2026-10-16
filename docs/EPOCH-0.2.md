# 0.2 engineering decisions

This epoch triages the independent October 11, 2026 review of `f1a2414`
against the released `c52030f` baseline. A source-proven failure path is not
evidence of its frequency on real devices. The minor version reflects new
recording-format support and the changed download format.

## Review disposition

| Finding | Decision |
| --- | --- |
| EC-01: hung capture cleanup stops the mix | Retain the bounded fail-closed policy. A native destructor cannot safely be abandoned. Per-source processes or mixer restart need a separate ownership design and Windows reproducer. |
| EC-02: listener/long-session evidence | Expand the repeatable acceptance protocol. Credential-free tests remain separate from listening, hardware and soak validation; do not invent results. |
| EC-03: migration exceeds route limits | Fix with ordered version migrations and complete final validation. Also fix overflowing restored card ID counters. |
| EC-04: live change succeeds but saving fails | Preserve the applied change; publish a persistent save warning and explicit retry. Keep command rejection and persistence failure separate. |
| EC-05: hard clipping | Retain bounded clipping and overload indication. Existing source faders provide headroom. Defer a global attenuator/limiter pending a demonstrated use case and listening comparison. |
| EC-06: endpoint formats | Add recording conversion for 44.1/96 kHz mono/stereo. Preserve exact 48 kHz path. Playback already requests Windows conversion through CPAL; do not add a second output resampler. Surround remains unsupported. |
| EC-07: scheduling/independent clocks | Add deterministic pause/jitter/skew coverage. Keep fixed bounded queues; larger buffers cannot correct permanent clock mismatch. Physical soak measurements remain outstanding. |
| EC-08: oversized process-loopback packet | Retain bounded rejection and packet release. Occurrence is unmeasured; avoid altering this separate native path alongside recording conversion. |
| EC-09: REST permission polling | Preserve fresh permission checks and existing transient-error classification. Profile request counts before changing caching or safety cadence. |
| EC-10: retired voice-token bound | Retain fail-closed anti-replay protection. Session-epoch retirement needs a separate proof. |
| EC-11: background process discovery | Keep explicit visible-process selection. Expanded discovery is a distinct feature. |
| EC-12: capture scope warning | Keep existing process-tree/browser disclosure; no new modal or tab-isolation claim. |
| EC-13: no published download | Stale: v0.1.0 was published with a successful release workflow. Simplify the next release's assets. |
| EC-14: large modules | Extract migration/validation behavior only; defer broad source moves. |

## Recording conversion

The next advisory pass reproduced an upstream CPAL silence-handling defect
before publication: Windows packets carrying `AUDCLNT_BUFFERFLAGS_SILENT`
still forwarded their payload. This was promoted into the 0.2 release gate.
A narrow checksum-verified dependency patch substitutes correctly represented
silence without reading the flagged payload. Synthetic packet-provider tests
exercise the actual patched function; no physical-device trigger frequency is
claimed. See [the acknowledged upstream issue](https://github.com/RustAudio/cpal/issues/1376)
and [Microsoft's packet-flag contract](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/ne-audioclient-_audclnt_bufferflags).

The engine and output bus stay at 48 kHz stereo. Negotiate only explicitly
selected recording endpoints and known PCM sample formats. Prefer 48 kHz,
then 44.1 kHz, then 96 kHz; accept mono/stereo only. Unsupported endpoints
remain visibly unavailable, with no default-device or system-audio fallback.

The 48 kHz path bypasses conversion. Other supported rates use the already
locked Rubato sinc converter, constructed before the callback with fixed
128-frame input blocks and preallocated stereo buffers. Mono is duplicated.
Callback partitions preserve partial frames and filter state; non-finite
samples are sanitized before filtering. Epoch changes reset history and fence
old output. Ring capacity and stale-data rejection remain unchanged. Filter
delay and block staging are local costs, not end-to-end latency measurements.

Portable tests use synthetic signals to check frequency/amplitude, channel
order, callback partitioning, anti-alias rejection, reset/mute behavior,
bounded overflow and allocation-free processing. Windows descriptor tests
exercise negotiation without opening real devices. A conversion-only timing
fixture is available for comparison; it does not measure application CPU,
Windows scheduling, power consumption or listener latency.

## Why there is no performance slider yet

CPAL 0.18.2's WASAPI `BufferSize::Fixed` changes buffer capacity, while callbacks
still follow the device period. Changing that number alone does not establish
lower callback overhead. Microsoft exposes period negotiation through
`IAudioClient3`, with device-dependent latency/power tradeoffs. Larger queues
also permit older audio and do not solve independent-clock drift.

Before shipping named modes, measure current behavior on the same synthetic
workload under idle/load, then compare CPU, wakeups, underruns, stale drops and
local delay for each candidate. A mode must provide a demonstrated useful
tradeoff and have a clear restart/live-change contract. No control is shown
until its behavior is wired and tested. Non-Windows runtime support remains a
separate project.

Sources: [pinned CPAL WASAPI implementation](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/wasapi/device.rs),
[Microsoft low-latency audio](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/low-latency-audio),
[Rubato 0.16.2](https://docs.rs/rubato/0.16.2/rubato/).

## Preferences as an evolving schema

The current file schema remains version 4; the new runtime warning is not a
saved setting. Read a bounded JSON envelope and check its version before
deserializing the known schema. Apply explicit `0 → 1 → 2 → 3 → 4` steps,
validate the complete migrated state, then normalize runtime state. Historical
constraints remain checked during their transition. Loading never rewrites a
file; a subsequent successful save writes the current schema atomically.

Future versions, malformed version declarations and oversized existing files
are preserved. Additive settings have defaults; invalid values remain errors.
New semantic changes must increment the schema constant, add one transition,
and test all historical fixtures, idempotence, invalid limits/identities,
forward-version preservation and failed writes. Restored applications await
explicit capture and all outputs restore idle.

The serialized byte count is also checked before writing: valid individual
field lengths can exceed the file budget after JSON escaping. Such a live
setup remains visibly unsaved while the previous valid file is preserved.

## Download design

Publish `echocord.exe`, the exact corresponding third-party source archive and
`SHA256SUMS`. Render version notes in the GitHub release body. Embed the MIT
license, complete dependency notices/attribution and source-location notice
inside the executable, with an offline Settings viewer. A build receipt binds
the executable, license inventory and locks; stale packaging fails.

This follows the practical separation seen in [Rufus executable releases](https://github.com/pbatard/rufus/releases/tag/v4.15)
and [OBS application/source downloads](https://github.com/obsproject/obs-studio/releases/tag/32.2.2).
Required notices are relocated, not removed. Covered sources remain available
beside the executable for the lifetime of the release; see [MPL 2.0 §3.2](https://www.mozilla.org/en-US/MPL/2.0/)
and [the release policy](RELEASING.md).
