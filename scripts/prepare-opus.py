#!/usr/bin/env python3
"""Materialize a verified, narrowly patched dependency outside the tracked tree."""

import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import urllib.request


ROOT = Path(__file__).resolve().parent.parent
VERSION = "0.3.3"
PACKAGE = f"libopus_sys-{VERSION}"
ARCHIVE_SHA256 = "b81c32f233fb2507347a93f97b9919493be9db7ec2249ce2c3ed0c89ad8edf60"
PATCH = ROOT / "patches" / "libopus_sys-0.3.3-cross.patch"
STAMP = ".echocord-patch.json"


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_digests(directory: Path) -> dict[str, str]:
    result = {}
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise SystemExit("Unexpected symlink in generated Opus source")
        if path.is_file() and path.relative_to(directory).as_posix() != STAMP:
            result[path.relative_to(directory).as_posix()] = digest(path)
    return result


def main() -> None:
    cache = ROOT / ".cache"
    cache.mkdir(exist_ok=True)
    destination = cache / PACKAGE
    patch_digest = digest(PATCH)
    stamp_path = destination / STAMP
    if destination.exists():
        stamp = json.loads(stamp_path.read_text())
        if (
            stamp["archive"] != ARCHIVE_SHA256
            or stamp["patch"] != patch_digest
            or source_digests(destination) != stamp["files"]
        ):
            raise SystemExit("Generated Opus cache differs from the pinned patch; inspect it before rebuilding")
        print("Pinned Opus patch: cached and verified")
        return

    archive = cache / f"{PACKAGE}.crate"
    if not archive.exists():
        url = f"https://static.crates.io/crates/libopus_sys/{PACKAGE}.crate"
        with urllib.request.urlopen(url, timeout=120) as response:
            archive.write_bytes(response.read())
    if digest(archive) != ARCHIVE_SHA256:
        raise SystemExit("Opus archive checksum mismatch; refusing extraction")

    with tempfile.TemporaryDirectory(prefix="opus-", dir=cache) as temporary:
        temporary_path = Path(temporary)
        with tarfile.open(archive) as source:
            source.extractall(temporary_path, filter="data")
        extracted = temporary_path / PACKAGE
        subprocess.run(
            ["patch", "--batch", "--fuzz=0", "-p1", "-i", str(PATCH)],
            cwd=extracted,
            check=True,
        )
        stamp = {
            "archive": ARCHIVE_SHA256,
            "patch": patch_digest,
            "files": source_digests(extracted),
        }
        (extracted / STAMP).write_text(json.dumps(stamp, indent=2) + "\n")
        extracted.rename(destination)
    print("Pinned Opus patch: prepared")


if __name__ == "__main__":
    main()
