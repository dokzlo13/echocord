# Capture and routing feasibility

Initial research checked 2026-10-09; implementation architecture reviewed in the current epoch. Recording-input to Discord is user-confirmed. The new application adapter remains under implementation and native validation; this report does not yet establish its behavior. [BRIEF](BRIEF.md) defines authorized delivery scope; [QUALITY](QUALITY.md) governs validation.

Recommend a Windows process-capture spike while retaining CPAL recording inputs. Driver-free capture is feasible; independent suppression of the captured application's original playback is unproven. PCM, mixing and recovery stay inside the owned Rust engine.

| Capability | Decision and practical limit | Relative scope |
| --- | --- | --- |
| Recording input | Retain CPAL and the isolated virtual-cable route as the working baseline. | Existing |
| Application/process capture | WASAPI captures one included process tree across playback endpoints, not browser tabs. [Microsoft sample][process-sample] | M |
| Desktop/endpoint capture | CPAL 0.18.2 enables loopback when building an input stream on a render device. Its whole mix includes Discord and notifications. [CPAL source][cpal] | S–M |
| Independent local mute | A future local destination can have separate gain/mute. Suppressing original source playback needs an experiment. | S probe; implementation depends on routing |
| Multiple sources, one mix | One Rust mixing timeline with bounded queues, drift handling, clipping control and protection against overlapping captures. | L |
| Multiple destinations | Fan out common PCM into independent bounded queues and output controls; enforce the guild-scoped bot constraint below. | M–L |
| Own selectable endpoint | Separate driver project: install/admin access, signing, packaging and recovery. SysVAD simulates capture/loopback, not a finished virtual cable. [SysVAD][sysvad] | XL |

S = bounded experiment; M = integrated feature; L = substantial pipeline work; XL = separate project. These are relative engineering judgments, not delivery commitments.

## API and library findings

- **Windows boundary:** the sample documents build 20348+, covering Windows 11. Probe capabilities under normal user privileges; report access-denied/unsupported audio without widening capture. Shared-mode loopback does not guarantee exclusive, ASIO or protected-stream capture. [Process sample][process-sample], [WASAPI loopback][loopback]
- **Include/exclude:** `wasapi` 0.25.0 maps `include_tree=true` to target plus descendants; `false` excludes that tree and captures other system audio. Its target-only docstring is incorrect. Exclusion is neither endpoint filtering nor an arbitrary exclusion list. [Pinned implementation][wasapi-code]
- **Queries:** process clients lack normal `get_mixformat`, `is_supported` and device-period queries; buffer sizes can be unusable. Initialize shared capture at explicit 48 kHz stereo float and bound allocations independently. Verify timestamp validity; timestamps alone do not establish total pipeline latency. [Versioned API][wasapi-client]
- **Libraries/pattern:** `wasapi` 0.25.0 is MIT-licensed, released October 1. Implementation review found its capture reader copies the packet pointer before checking the silent flag, and its activation helper waits without a deadline. The current design therefore uses direct Microsoft `windows` bindings for the small process adapter, keeping COM objects on a capture thread, checking silence before reading data, and bounding activation/shutdown. OBS 32.2.2 demonstrates window-to-PID selection, included-tree capture, explicit float format and event-driven processing. [Pinned implementation][wasapi-code], [release][wasapi-release], [manifest][wasapi-manifest], [OBS source][obs]
- **Selection/silence:** browser roots can cover several tabs/windows; a window picker does not establish tab isolation. Verify roots, invalidate exited PIDs and retain recording input for incompatible applications. Honor silent/discontinuity flags and bounded cancellation. The recorder example's 4096-frame chunks and three-second event timeout are unsuitable live defaults. [Example][wasapi-example], [OBS guidance][obs-guide], [packet flags][packet-flags]

## Local playback suppression

Process-loopback activation has no original-playback suppression option. Muting Echocord's future monitor affects only its own playback; the source application can remain audible. [Activation parameters][activation]

An upstream experiment reports session-volume changes reaching captured samples. This is external evidence; independent application/session mute remains unproven. Endpoint mute also silences Discord on that endpoint. Quiet routing remains the existing cable without monitoring, or a separate existing playback endpoint when available. [Reported experiment][volume-report], [endpoint mute][endpoint-mute]

## Discord routing constraint

Protocol/library conclusion: one bot identity supports multiple guilds concurrently, one channel in each. A same-guild channel change replaces the voice session; Songbird switches its guild-keyed call. Extra processes/sessions using that identity do not add slots. Simultaneous same-guild destinations require distinct identities and connection/encryption lifecycles. No live multi-channel test was run. [Pinned Discord protocol][discord], [Songbird 0.6.0][songbird]

## Proposed next spike

Scope: one selected process tree to 48 kHz stereo float through the existing bounded reader and owned engine. Cross-build in WSL, run on Windows; the local experiment needs no bot credentials or additional Windows development toolchain.

1. Verify isolation with distinguishable selected/unselected signals, browser roots and output-device changes.
2. Test app/tab mute, session volume/mute and endpoint mute separately against captured signal, local audibility and other playback.
3. Exercise silence/resume, source exit/restart, sleep/resume and stop; never automatically capture unrelated audio.
4. Record packet flags, queue age/drops, marker delay and cleanup. Require isolation, correct PCM, silence/resume and cancellation; report local-mute results separately. Listener latency and five-hour acceptance remain later work.

Use the evidence to design source selection. Later fan-out must not share a destructively consumed reader or let slow outputs stall capture. Routing-matrix and driver implementation are outside this spike.

[process-sample]: https://github.com/microsoft/Windows-classic-samples/blob/434f6002bdf9cf9829406c3ff2b33387982d6168/Samples/ApplicationLoopback/README.md
[cpal]: https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/wasapi/device.rs#L830
[loopback]: https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording
[wasapi-code]: https://github.com/HEnquist/wasapi-rs/blob/v0.25.0/src/api.rs#L734
[wasapi-client]: https://docs.rs/wasapi/0.25.0/wasapi/struct.AudioClient.html
[wasapi-release]: https://github.com/HEnquist/wasapi-rs/releases/tag/v0.25.0
[wasapi-manifest]: https://github.com/HEnquist/wasapi-rs/blob/v0.25.0/Cargo.toml
[wasapi-example]: https://github.com/HEnquist/wasapi-rs/blob/v0.25.0/examples/record_application.rs
[obs]: https://github.com/obsproject/obs-studio/blob/32.2.2/plugins/win-wasapi/win-wasapi.cpp#L660
[obs-guide]: https://obsproject.com/kb/application-audio-capture-guide
[packet-flags]: https://learn.microsoft.com/en-us/windows/win32/api/audioclient/ne-audioclient-_audclnt_bufferflags
[activation]: https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params
[volume-report]: https://github.com/microsoft/Windows-classic-samples/issues/319#issuecomment-3951821764
[endpoint-mute]: https://learn.microsoft.com/en-us/windows/win32/api/endpointvolume/nf-endpointvolume-iaudioendpointvolume-setmute
[discord]: https://github.com/discord/discord-api-docs/blob/37148620d2324292b76047541d5f8331f81ea77a/developers/topics/voice-connections.mdx#L38
[songbird]: https://github.com/serenity-rs/songbird/blob/v0.6.0/src/manager.rs#L206
[sysvad]: https://learn.microsoft.com/en-us/samples/microsoft/windows-driver-samples/sysvad-virtual-audio-device-driver-sample/
