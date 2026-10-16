# Third-party notices

Echocord's original code is licensed under [MIT](LICENSE). Its dependencies retain their own licenses; the MIT license does not replace their terms or restrict their recipients' rights.

Every Windows release executable embeds the complete generated dependency inventory, copyright notices and license texts, available offline in **Settings → Licenses**. Original notice files are retained verbatim; HTML notices also have readable text views. The generated source files are `licenses/generated/INDEX.md` and `licenses/generated/texts/`. The inventory records exact package versions, checksums and source locations. It includes the Windows Rust dependency graph, production frontend packages, the Rust standard library, bundled Opus, the WebView2 SDK loader, and embedded third-party notices. Build and cross-platform code can appear in upstream notices even when it is not used by the Windows executable.

The generated index and JSON inventory retain the original generator paths as provenance. The app's catalog resolves their embedded locations. HTML originals use an additional `.txt` suffix inside the app so webview packaging cannot alter their bytes; their readable text views are separate derived documents.

## Corresponding source

The MPL-2.0 portions remain under the Mozilla Public License 2.0. Their exact, unchanged published source packages are provided as **echocord-<version>-third-party-sources.zip**, beside the Windows download on the [same GitHub release](https://github.com/dokzlo13/echocord/releases). That archive contains a manifest, checksum-verified `.crate` source archives and the MPL license. `.crate` files are ordinary gzip-compressed tar archives. Each package's original source notices remain intact. The generated inventory also links to each exact source archive on crates.io. No MPL-covered source is modified by Echocord.

The small MIT/BSD Opus build patch is published in `patches/` in the matching Echocord source tag. Opus retains its BSD license and upstream patent-license references. The Rust WebView2 bindings are MIT; the statically linked Microsoft WebView2 SDK loader has its own BSD license and accompanying notices. The separately installed Microsoft WebView2 Runtime and Visual C++ Redistributable are not included in Echocord's executable and retain Microsoft's terms.

Sofia Sans and Sofia Sans Condensed retain the SIL Open Font License 1.1. Lucide's complete notice also includes the MIT attribution for icons derived from Feather. Imported development skills retain their adjacent upstream licenses and notices; additional bundled-tool attribution is in `.agents/NOTICES/`. These development skills are not embedded in the application.

## Reproducing the release materials

After preparing the locked dependency patch, run:

```sh
python scripts/prepare-opus.py
python scripts/package-third-party.py
```

Use the pinned Rust toolchain and Python 3.12 or newer. The generator uses Cargo's selected Windows dependency graph and the npm production lock entries. It checks cached or downloaded source archives against lockfile checksums, never extracts them, and rejects unsafe archive paths and links. Supplemental notices are pinned in `licenses/supplements.json`. Outputs are `licenses/generated/` and `artifacts/release/echocord-<version>-third-party-sources.zip`; they contain no build-machine paths. Repeat with `--offline` once the archives are cached. Changes to dependencies or supplemental notices require regeneration and review before release.

The generator deliberately stops when a dependency lacks both its own license file and a reviewed supplement. For a few packages whose published source only declares a standard license, the inventory includes the original package metadata and source attribution alongside the standard terms. It does not invent a copyright holder or replace an upstream notice.
