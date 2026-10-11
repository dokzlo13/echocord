# Windows cross-compilation patch

`libopus_sys-0.3.3-cross.patch` applies to the published 0.3.3 crate used by opus2 0.4.0 and Songbird 0.6.0. `scripts/prepare-opus.py` verifies the archive SHA-256 against its crates.io checksum, extracts it under ignored `.cache/`, applies the patch with an exact Python patcher, and checks the complete generated source-file inventory and digests on reuse. It rejects unexpected symlinks. It needs Python 3.12+; no Unix `patch` executable is required. Cargo uses the generated source through an explicit path override.

The patch has three narrow changes:

- Choose Unix libm linking from Cargo's target family rather than the build host. The build explicitly enables bundled/static Opus, so host package discovery and default linking-mode selection are bypassed.
- Apply SSE4.1 source flags with clang-cl as well as GNU-style compilers. Keep runtime CPU dispatch; do not globally require AVX2 or SSE4.1 for the executable.
- Exclude MSVC-only builtin replacements when compiling with Clang. Clang provides those builtins; the replacement block also includes a Windows header using case-sensitive spelling incompatible with the Linux SDK cache.

The initial unpatched build reproduced missing SSE4.1 target-feature errors. After fixing those flags, the MSVC-only block failed on its header include. Both failures are build-environment compatibility issues; they do not require a Windows compiler installation.

The upstream source stays outside Git. License notices for the small patch context are retained here and are also present in the extracted source. Release packaging collects these and the full dependency notices with `scripts/package-third-party.py`; covered dependency source archives accompany the binary release.

Remove the override after a compatible upstream release builds and passes the probe without it. Do not edit Cargo's global registry cache or silently substitute a different Opus ABI.

Sources: [libopus_sys 0.3.3](https://crates.io/crates/libopus_sys/0.3.3), [opus2 0.4.0](https://crates.io/crates/opus2/0.4.0), [Cargo build-script target variables](https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-build-scripts).
