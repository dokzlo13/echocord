#!/usr/bin/env python3
"""Materialize a verified, narrowly patched dependency outside the tracked tree."""

import importlib.util
import json
from pathlib import Path
import tarfile
import tempfile
import urllib.request


ROOT = Path(__file__).resolve().parent.parent
VERSION = "0.16.2"
PACKAGE = f"rubato-{VERSION}"
ARCHIVE_SHA256 = "5258099699851cfd0082aeb645feb9c084d9a5e1f1b8d5372086b989fc5e56a1"
PATCH = ROOT / "patches" / "rubato-0.16.2-sizing.patch"
STAMP = ".echocord-patch.json"


spec = importlib.util.spec_from_file_location(
    "prepare_opus_helpers", Path(__file__).with_name("prepare-opus.py")
)
helpers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helpers)
digest = helpers.digest
source_digests = helpers.source_digests
apply_patch = helpers.apply_patch


def main() -> None:
    cache = ROOT / ".cache"
    cache.mkdir(exist_ok=True)
    destination = cache / PACKAGE
    patch_digest = digest(PATCH)
    stamp_path = destination / STAMP
    if destination.exists():
        stamp = json.loads(stamp_path.read_text(encoding="utf-8"))
        if (
            stamp["archive"] != ARCHIVE_SHA256
            or stamp["patch"] != patch_digest
            or source_digests(destination) != stamp["files"]
        ):
            raise SystemExit(
                "Generated Rubato cache differs from the pinned patch; inspect it before rebuilding"
            )
        print("Pinned Rubato patch: cached and verified")
        return

    archive = cache / f"{PACKAGE}.crate"
    if not archive.exists():
        url = f"https://static.crates.io/crates/rubato/{PACKAGE}.crate"
        with urllib.request.urlopen(url, timeout=120) as response:
            archive.write_bytes(response.read())
    if digest(archive) != ARCHIVE_SHA256:
        raise SystemExit("Rubato archive checksum mismatch; refusing extraction")

    with tempfile.TemporaryDirectory(prefix="rubato-", dir=cache) as temporary:
        temporary_path = Path(temporary)
        with tarfile.open(archive) as source:
            source.extractall(temporary_path, filter="data")
        extracted = temporary_path / PACKAGE
        apply_patch(extracted, PATCH)
        stamp = {
            "archive": ARCHIVE_SHA256,
            "patch": patch_digest,
            "files": source_digests(extracted),
        }
        (extracted / STAMP).write_text(
            json.dumps(stamp, indent=2) + "\n", encoding="utf-8"
        )
        extracted.rename(destination)
    print("Pinned Rubato patch: prepared")


if __name__ == "__main__":
    main()
