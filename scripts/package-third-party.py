#!/usr/bin/env python3
"""Build release notices and exact MPL sources from checksum-locked packages.

Requires Python 3.11+, the pinned Rust toolchain, and the prepared Opus patch.
Archives are inspected in memory, never extracted. No credentials are accessed.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"
MAX_DOWNLOAD = 200 * 1024 * 1024
MAX_UNPACKED = 1024 * 1024 * 1024
NOTICE_NAME = re.compile(r"^(?:licen[cs]e|copying|copyright|notice|patents?)(?:[._-].*)?$", re.I)
ATTRIBUTION = re.compile(r"^\s*(?:(?://|/\*|\*|#|<!--)\s*)?(?:copyright(?:\s|\()|SPDX-(?:FileCopyrightText|License-Identifier):)", re.I)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + "\n").encode()


def run(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8").strip()


def safe_name(name: str) -> str:
    """Reject both POSIX and Windows traversal even when no extraction occurs."""
    if not name or "\\" in name or "\x00" in name or ":" in name:
        raise ValueError("Unsafe archive member name")
    parts = name.rstrip("/").split("/")
    if any(part in ("", ".", "..") for part in parts) or PurePosixPath(name).is_absolute():
        raise ValueError("Unsafe archive member path")
    return "/".join(parts)


def tar_files(data: bytes, expected_root: str | None) -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    total = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for index, member in enumerate(archive):
            if index > 100_000:
                raise ValueError("Too many source archive members")
            name = safe_name(member.name)
            if expected_root is None:
                expected_root = name.split("/", 1)[0]
            if name.split("/", 1)[0] != expected_root:
                raise ValueError("Unexpected source archive root")
            if not (member.isdir() or member.isfile()):
                raise ValueError("Source archive links and special files are not accepted")
            if member.isdir():
                continue
            total += member.size
            if total > MAX_UNPACKED or member.size > MAX_DOWNLOAD:
                raise ValueError("Source archive exceeds size limit")
            relative = name.removeprefix(expected_root + "/")
            if relative == name or relative in files:
                raise ValueError("Invalid or duplicate source archive member")
            handle = archive.extractfile(member)
            if handle is None:
                raise ValueError("Unreadable source archive member")
            files[relative] = handle.read()
    return files


def sdk_files(data: bytes, expected: dict[str, str]) -> dict[str, bytes]:
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        names: set[str] = set()
        total = 0
        for member in archive.infolist():
            name = safe_name(member.filename)
            if name in names:
                raise ValueError("Duplicate SDK archive member")
            names.add(name)
            if (member.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError("SDK archive symlink is not accepted")
            total += member.file_size
            if total > MAX_UNPACKED or member.file_size > MAX_DOWNLOAD:
                raise ValueError("SDK archive exceeds size limit")
        result = {name: archive.read(name) for name in expected}
    for name, content in result.items():
        if sha256(content) != expected[name]:
            raise ValueError("SDK notice checksum mismatch")
    return result


class Fetcher:
    def __init__(self, cache: Path, offline: bool):
        self.cache, self.offline = cache, offline
        cache.mkdir(parents=True, exist_ok=True)

    def get(self, url: str, expected: str, algorithm: str = "sha256", candidates: tuple[Path, ...] = ()) -> bytes:
        parsed = urllib.parse.urlparse(url)
        if parsed.scheme != "https" or parsed.username or parsed.password:
            raise ValueError("Source downloads must use credential-free HTTPS URLs")
        key = hashlib.sha256((algorithm + ":" + expected).encode()).hexdigest()
        cached = self.cache / key
        for candidate in (cached, *candidates):
            if candidate.is_file():
                data = candidate.read_bytes()
                if hashlib.new(algorithm, data).hexdigest() != expected:
                    raise ValueError("Cached dependency checksum mismatch; remove the corrupted cache entry")
                return data
        if self.offline:
            raise ValueError("Required dependency archive is not cached; run once without --offline")
        request = urllib.request.Request(url, headers={"User-Agent": "Echocord-license-materials/1"})
        with urllib.request.urlopen(request, timeout=120) as response:
            data = response.read(MAX_DOWNLOAD + 1)
        if len(data) > MAX_DOWNLOAD:
            raise ValueError("Dependency download exceeds size limit")
        if hashlib.new(algorithm, data).hexdigest() != expected:
            raise ValueError("Downloaded dependency checksum mismatch")
        with tempfile.NamedTemporaryFile(dir=self.cache, delete=False) as handle:
            handle.write(data)
            temporary = Path(handle.name)
        temporary.replace(cached)
        return data


class Notices:
    def __init__(self, destination: Path):
        self.destination = destination
        (destination / "texts").mkdir(parents=True)

    def add(self, content: bytes, name: str) -> dict[str, str]:
        digest = sha256(content)
        extension = ".html" if name.lower().endswith(".html") else ".txt"
        path = "texts/" + digest + extension
        (self.destination / path).write_bytes(content)
        return {"original_name": name, "path": path, "sha256": digest}

    def package(self, files: dict[str, bytes]) -> list[dict[str, str]]:
        result = []
        for name, data in sorted(files.items()):
            path = PurePosixPath(name)
            if NOTICE_NAME.fullmatch(path.name) and path.suffix.lower() not in (".rs", ".js", ".ts", ".c", ".h", ".py", ".json"):
                result.append(self.add(data, name))
        return result

    def attribution(self, files: dict[str, bytes], authors: object, license_expression: str) -> dict[str, str]:
        lines = []
        for name, data in sorted(files.items()):
            if len(data) > 4 * 1024 * 1024 or b"\0" in data:
                continue
            try:
                text = data.decode("utf-8")
            except UnicodeDecodeError:
                continue
            for index, line in enumerate(text.splitlines(), 1):
                if ATTRIBUTION.search(line):
                    lines.append(f"{name}:{index}: {line}")
        content = "Published package metadata (authors are not an invented copyright notice):\n"
        content += json.dumps({"authors": authors, "license": license_expression}, indent=2, ensure_ascii=False)
        content += "\n\nVerbatim copyright/SPDX lines from the published source package:\n"
        content += "\n".join(lines) + ("\n" if lines else "No such lines supplied in this package.\n")
        return self.add(content.encode(), "published-attribution.txt")


def supplements_for(key: str, config: dict, fetcher: Fetcher, notices: Notices) -> tuple[list[dict], str | None]:
    supplement = config["rust"].get(key)
    if not supplement:
        raise ValueError(f"No license file or reviewed supplemental notice for {key}")
    result = []
    for file in supplement["files"]:
        item = notices.add(fetcher.get(file["url"], file["sha256"]), file["name"])
        item["source_url"] = file["url"]
        result.append(item)
    return result, supplement["reason"]


def rust_packages(fetcher: Fetcher, notices: Notices, config: dict) -> tuple[list[dict], dict[str, bytes]]:
    offline = ("--offline",) if fetcher.offline else ()
    metadata = json.loads(run("cargo", "metadata", "--locked", *offline, "--filter-platform", TARGET, "--format-version", "1"))
    tree = run("cargo", "tree", "--locked", *offline, "--target", TARGET, "--package", "echocord", "--edges", "normal", "--prefix", "none", "--format", "{p}")
    included = set(re.findall(r"^(\S+) v(\S+)", tree, re.M))
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    checksums = {(p["name"], p["version"]): p["checksum"] for p in lock["package"] if "checksum" in p}
    result, mpl_sources = [], {}
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        name, version = package["name"], package["version"]
        if (name, version) not in included or name in ("echocord", "echocord-core"):
            continue
        key = name + "@" + version
        filename = f"{name}-{version}.crate"
        url = f"https://static.crates.io/crates/{name}/{filename}"
        manifest = Path(package["manifest_path"])
        if package["source"] == "registry+https://github.com/rust-lang/crates.io-index":
            digest = checksums[(name, version)]
            # Cargo registry/src/<registry>/<package>/Cargo.toml -> registry/cache/<registry>/<archive>.
            candidate = manifest.parents[3] / "cache" / manifest.parents[1].name / filename
            candidates = (candidate,)
        elif name == "libopus_sys" and version == "0.3.3" and package["source"] is None:
            digest = "b81c32f233fb2507347a93f97b9919493be9db7ec2249ce2c3ed0c89ad8edf60"
            candidates = (ROOT / ".cache" / filename,)
        else:
            raise ValueError(f"Unreviewed non-registry dependency: {key}")
        data = fetcher.get(url, digest, candidates=candidates)
        files = tar_files(data, f"{name}-{version}")
        entries = notices.package(files)
        reason = None
        if not entries:
            entries, reason = supplements_for(key, config, fetcher, notices)
        license_expression = package.get("license")
        if not license_expression:
            raise ValueError(f"Missing declared license: {key}")
        attribution = notices.attribution(files, package.get("authors", []), license_expression)
        item = {"ecosystem": "cargo", "name": name, "version": version, "license": license_expression,
                "source_url": url, "source_sha256": digest, "notices": entries, "attribution": attribution}
        if reason:
            item["supplement_reason"] = reason
        if name == "libopus_sys":
            item["patch"] = {"path": "patches/libopus_sys-0.3.3-cross.patch", "sha256": sha256((ROOT / "patches/libopus_sys-0.3.3-cross.patch").read_bytes())}
        if "MPL-2.0" in license_expression:
            mpl_sources["crates/" + filename] = data
        result.append(item)
    return result, mpl_sources


def npm_packages(fetcher: Fetcher, notices: Notices) -> list[dict]:
    lock = json.loads((ROOT / "ui/package-lock.json").read_text())
    result = []
    for path, package in sorted(lock["packages"].items()):
        if not path or package.get("dev"):
            continue
        if "node_modules/" not in path or package.get("link"):
            raise ValueError("Unreviewed linked frontend dependency")
        name = path.rsplit("node_modules/", 1)[1]
        version, url, integrity = package["version"], package["resolved"], package["integrity"]
        algorithm, encoded = integrity.split("-", 1)
        if algorithm not in ("sha256", "sha384", "sha512"):
            raise ValueError("Frontend dependency must have a strong lockfile integrity hash")
        expected = base64.b64decode(encoded, validate=True).hex()
        data = fetcher.get(url, expected, algorithm)
        # DefinitelyTyped packages use their own directory name, not "package".
        # Enforce one safe root and verify the manifest identity below.
        files = tar_files(data, None)
        package_metadata = json.loads(files["package.json"])
        if package_metadata["name"] != name or package_metadata["version"] != version:
            raise ValueError("Frontend source identity does not match lockfile")
        entries = notices.package(files)
        if not entries:
            raise ValueError(f"No frontend license notice for {name}@{version}")
        result.append({"ecosystem": "npm", "name": name, "version": version, "license": package["license"],
                       "source_url": url, "source_sha256": sha256(data), "lock_integrity": integrity, "notices": entries,
                       "attribution": notices.attribution(files, package_metadata.get("author", package_metadata.get("contributors", [])), package["license"])})
    return result


def deterministic_zip(path: Path, files: dict[str, bytes]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, suffix=".zip", delete=False) as temporary:
        temp_path = Path(temporary.name)
    try:
        with zipfile.ZipFile(temp_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name, data in sorted(files.items()):
                safe_name(name)
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.create_system = 3
                info.external_attr = 0o100644 << 16
                archive.writestr(info, data)
        temp_path.replace(path)
    finally:
        temp_path.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="Use only checksum-verified cached archives/notices")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "artifacts/release", help="Destination for the corresponding-source ZIP")
    args = parser.parse_args()
    config = json.loads((ROOT / "licenses/supplements.json").read_text())
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?", version):
        raise ValueError("Unsupported release version syntax")
    fetcher = Fetcher(ROOT / ".cache/license-materials", args.offline)
    output = ROOT / "licenses/generated"
    if output.is_symlink():
        raise ValueError("Refusing to replace a symlinked notice directory")
    with tempfile.TemporaryDirectory(prefix="license-output-", dir=ROOT / ".cache") as temporary:
        staging = Path(temporary) / "generated"
        notices = Notices(staging)
        packages, source_files = rust_packages(fetcher, notices, config)
        packages.extend(npm_packages(fetcher, notices))
        sdk = config["webview2_sdk"]
        if sdk["wrapper"] not in {p["name"] + "@" + p["version"] for p in packages}:
            raise ValueError("WebView2 wrapper changed; review the SDK notice pin")
        sdk_data = fetcher.get(sdk["url"], sdk["sha256"])
        sdk_notices = sdk_files(sdk_data, sdk["files"])
        packages.append({"ecosystem": "sdk", "name": "Microsoft.Web.WebView2 loader", "version": sdk["version"],
                         "license": "BSD-3-Clause", "source_url": sdk["url"], "source_sha256": sdk["sha256"],
                         "notices": [notices.add(data, name) for name, data in sorted(sdk_notices.items())]})
        sysroot = Path(run("rustc", "--print", "sysroot"))
        rust_version = run("rustc", "--version").split()[1]
        pinned = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
        if rust_version != pinned:
            raise ValueError("Use the repository's pinned Rust toolchain for release notices")
        library_notice = sysroot / "share/doc/rust/COPYRIGHT-library.html"
        if not library_notice.is_file():
            raise ValueError("Rust toolchain is missing COPYRIGHT-library.html; reinstall the pinned toolchain")
        packages.append({"ecosystem": "toolchain", "name": "Rust standard library", "version": rust_version,
                         "license": "MIT OR Apache-2.0; embedded notices and exceptions retained",
                         "source_url": f"https://github.com/rust-lang/rust/tree/{rust_version}/library",
                         "notices": [notices.add(library_notice.read_bytes(), "COPYRIGHT-library.html")]})
        mpl = config["standard_mpl"]
        source_files["MPL-2.0.txt"] = fetcher.get(mpl["url"], mpl["sha256"])
        covered = [{key: p[key] for key in ("name", "version", "license", "source_url", "source_sha256")} for p in packages if "MPL-2.0" in p["license"]]
        source_files["MANIFEST.json"] = json_bytes({"echocord_version": version, "packages": covered})
        source_files["README.txt"] = ("Corresponding MPL-2.0 sources for Echocord " + version + ".\n\nThese are unchanged, checksum-verified published source packages. Each .crate file\nis a gzip-compressed tar archive; extract with tar -xzf <package>.crate.\nPackage names, versions, URLs and SHA-256 hashes are in MANIFEST.json.\nThe source remains under MPL-2.0; see MPL-2.0.txt and original per-file notices.\n").encode()
        filename = f"echocord-{version}-third-party-sources.zip"
        source_zip = args.output_dir / filename
        deterministic_zip(source_zip, source_files)
        inventory = {"schema": 1, "echocord_version": version, "target": TARGET, "rust_version": rust_version,
                     "cargo_lock_sha256": sha256((ROOT / "Cargo.lock").read_bytes()),
                     "npm_lock_sha256": sha256((ROOT / "ui/package-lock.json").read_bytes()),
                     "source_archive": {"name": filename, "sha256": sha256(source_zip.read_bytes())}, "packages": packages}
        (staging / "inventory.json").write_bytes(json_bytes(inventory))
        index = ["# Release dependency notices", "", f"Echocord {version} · Windows x64 · Rust {rust_version}", "",
                 "This inventory covers the selected Windows normal dependency graph (including proc-macro code) and production npm lock entries. Upstream packages can include notices for additional platforms/build code. Original licenses, embedded notices and attribution are retained verbatim; standard supplemental terms do not invent copyright attribution.", "",
                 f"MPL corresponding source: [{filename}](https://github.com/dokzlo13/echocord/releases/download/v{version}/{filename}). SHA-256: `{inventory['source_archive']['sha256']}`. The same release also provides the Echocord source tag and Opus build patch.", "",
                 "Machine-readable versions, checksums, archive URLs, supplements and notice-file provenance: [inventory.json](inventory.json).", ""]
        for package in packages:
            index.extend([f"## {package['name']} {package['version']}", "", f"License: `{package['license']}`. [Published source]({package['source_url']}).", ""])
            for entry in package["notices"]:
                index.append(f"- [{entry['original_name']}]({entry['path']})" + (f" ([upstream supplement]({entry['source_url']}))" if "source_url" in entry else ""))
            if "attribution" in package:
                index.append(f"- [Published authors and source attribution]({package['attribution']['path']})")
            index.append("")
            if package.get("supplement_reason"):
                index.extend([package["supplement_reason"], ""])
        (staging / "INDEX.md").write_text("\n".join(index), encoding="utf-8", newline="\n")
        if output.exists():
            shutil.rmtree(output)
        shutil.move(str(staging), output)
    print(f"Packaged notices for {len(packages)} dependencies; {len(covered)} MPL source packages ({source_zip.stat().st_size:,} bytes).")
    print("Notices: licenses/generated/INDEX.md")
    print("Corresponding sources: " + filename)


if __name__ == "__main__":
    main()
