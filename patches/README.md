# Pinned dependency patches

`libopus_sys-0.3.3-cross.patch` applies to the published 0.3.3 crate used by opus2 0.4.0 and Songbird 0.6.0. `scripts/prepare-opus.py` verifies the archive SHA-256 against its crates.io checksum, extracts it under ignored `.cache/`, applies the patch with an exact Python patcher, and checks the complete generated source-file inventory and digests on reuse. It rejects unexpected symlinks. It needs Python 3.12+; no Unix `patch` executable is required. Cargo uses the generated source through an explicit path override.

The patch has three narrow changes:

- Choose Unix libm linking from Cargo's target family rather than the build host. The build explicitly enables bundled/static Opus, so host package discovery and default linking-mode selection are bypassed.
- Apply SSE4.1 source flags with clang-cl as well as GNU-style compilers. Keep runtime CPU dispatch; do not globally require AVX2 or SSE4.1 for the executable.
- Exclude MSVC-only builtin replacements when compiling with Clang. Clang provides those builtins; the replacement block also includes a Windows header using case-sensitive spelling incompatible with the Linux SDK cache.

The initial unpatched build reproduced missing SSE4.1 target-feature errors. After fixing those flags, the MSVC-only block failed on its header include. Both failures are build-environment compatibility issues; they do not require a Windows compiler installation.

The upstream source stays outside Git. License notices for the small patch context are retained here and are also present in the extracted source. Release packaging collects these and the full dependency notices with `scripts/package-third-party.py`; covered dependency source archives accompany the binary release.

Remove the override after a compatible upstream release builds and passes the probe without it. Do not edit Cargo's global registry cache or silently substitute a different Opus ABI.

Sources: [libopus_sys 0.3.3](https://crates.io/crates/libopus_sys/0.3.3), [opus2 0.4.0](https://crates.io/crates/opus2/0.4.0), [Cargo build-script target variables](https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-build-scripts).

## CPAL silent capture packets

`cpal-0.18.2-silent.patch` fixes the pinned WASAPI input path's handling of
`AUDCLNT_BUFFERFLAGS_SILENT`. Windows requires the packet payload to be ignored;
upstream 0.18.2 forwards it unchanged. This is tracked in
[CPAL issue 1376](https://github.com/RustAudio/cpal/issues/1376), with an
[unmerged upstream fix](https://github.com/RustAudio/cpal/pull/1379).

`scripts/prepare-cpal.py` verifies the published crate checksum, uses the same
exact patcher and complete source-inventory verification as the Opus bootstrap,
and materializes the dependency under ignored `.cache/`. It never edits the
global Cargo registry. The original Apache-2.0 license is retained in
`cpal-LICENSE` and the generated crate. Local modifications follow the
repository's MIT license; the resulting dependency remains subject to Apache-2.0.
Release notices identify the modification and preserve the original license.

The patch allocates aligned scratch once before native callbacks, fills silent
packets with the sample format's equilibrium value, and never reads or writes
their driver payload. Nonempty packets are released on validation/timestamp
errors as well as successful processing. Packet sizes are checked against the
negotiated native buffer and scratch capacity. Ordinary PCM and I24 conversion
retain their existing behavior. Device selection, stream routing, playback and
stop/drain policy are unchanged.

`scripts/test_cpal_patch.py` compiles the actual patched packet function and the
upstream equilibrium helper against a synthetic packet provider. It checks
silent nonzero/null payloads, unsigned midpoint silence, ordinary PCM, I24,
alignment, capacity errors, packet release and successful packet allocation counts.
It opens no devices and does not establish driver or physical audio acceptance.

Remove the override after a released upstream version includes an equivalent
fix and passes these checks. Source:
[Microsoft's buffer-flag contract](https://learn.microsoft.com/windows/win32/api/audioclient/ne-audioclient-_audclnt_bufferflags).
