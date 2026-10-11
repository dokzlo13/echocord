# Releases

Echocord uses stable Semantic Versioning tags (`vMAJOR.MINOR.PATCH`). The initial public version is0.1.0. Below 1.0, minor releases may change behavior or compatibility; patch releases should remain focused fixes. Every published version has a changelog section and an immutable source tag by project policy.

## What CI produces

`.github/workflows/ci.yml` runs on main, pull requests and manual dispatch. Ubuntu checks portable Rust and frontend behavior. Windows sets up the documented native toolchain, runs native core/application checks, builds the Windows executable, prepares full third-party notices/corresponding sources and packages the tested binary. Its downloadable artifact is the complete licensed `release-assets` set, not an unaccompanied executable.

`.github/workflows/release.yml` runs for `v*` tags. It verifies the tag against all version declarations, calls the same CI workflow, and publishes the exact checked assets after every required job succeeds. Only the final publishing job has repository-content write permission. Tests use no Discord token and never join voice.

Release files include:

- `echocord-<version>-windows-x64.zip`: EXE, runtime/setup instructions, project license and complete third-party notices.
- `echocord-<version>-third-party-sources.zip`: exact source packages required for the MPL-covered portions, with manifest/checksums and license.
- `SHA256SUMS` and the Windows ZIP checksum.
- Version-specific release notes explaining the change and download requirements.

The app is currently unsigned. Require Windows 11 x64, Microsoft WebView2 Evergreen Runtime and the latest Visual C++ v14 x64 runtime; do not describe the ZIP as having no runtime dependencies.

## Prepare a version

1. Update the workspace version in `Cargo.toml`, the two workspace package entries in `Cargo.lock`, `src-tauri/tauri.conf.json`, and the root/UI entries in `ui/package.json` and `ui/package-lock.json`. Member Cargo manifests inherit the workspace version. The UI displays build metadata from its package version.
2. Move user-facing changes from Unreleased into the dated version section in `CHANGELOG.md`. Include meaningful limitations or migration notes. Packaging must find that version's section.
3. Run the applicable check/build/packaging commands:

   ```sh
   python scripts/dev.py version --tag v0.1.0
   python scripts/dev.py check
   python scripts/dev.py build
   python scripts/package-third-party.py
   python scripts/dev.py package --tag v0.1.0
   ```

   Substitute the intended version and use `python3` on Linux/WSL. An existing app session must finish before replacing its executable; use a separate build target while one is running.
4. Review the source diff, generated contract, manifest/lock changes, release notes, source archive, notice inventory and packaged contents. Verify checksums and scan the EXE/archive for credentials and personal machine paths. Generated third-party output must match the current locks/version; do not hand-edit generated notices.
5. Obtain independent review. Record which tests ran, especially whether native audio/UI and real listening were exercised. Automated CI passing is not a listening or long-session guarantee.

## Publish

Publishing requires explicit maintainer authorization. After the reviewed commit is on main and CI is green:

```sh
git tag -a v0.1.0 -m "Echocord 0.1.0"
git push origin v0.1.0
```

The release workflow performs its own gates and publication. Check the completed run, tag/commit, release notes, all expected assets and their checksums. Download and inspect the published archive; perform a credential-free Windows startup check where available. Keep the corresponding-source archive beside the binary for the lifetime of the release.

If validation fails before publication, fix the cause and rerun the failed workflow only when its source remains correct. Do not bypass checks or upload an unrelated local binary. If source must change, choose the next version or an unpublished replacement tag according to review; never move/delete a version tag or replace assets after that release has been published. Publish a new patch version for corrections.

## Dependency and automation maintenance

Keep GitHub Actions pinned to reviewed commit SHAs and permissions scoped to each job. Do not run untrusted PR code with publishing credentials. Lock Rust/npm dependencies and regenerate notices/source packages after changes; imported fonts, icons, native libraries and embedded licenses retain their original terms.

Build source paths are remapped and PDB references are generic. C/C++ dependencies can embed assertion filenames too, so Rust remapping alone is insufficient. Changing native compiler flags may require cleaning that dependency's previous output before a local rebuild; clean CI checkouts avoid stale flags.

Source, scripts and this guide must remain sufficient to reproduce the process from a fresh checkout. Releases must not depend on local notes, personal accounts beyond ordinary GitHub maintainer permissions, or a particular coding-agent harness.
