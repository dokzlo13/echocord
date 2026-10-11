#!/usr/bin/env python3
"""Materialize a verified, narrowly patched dependency outside the tracked tree."""

import hashlib
import json
from pathlib import Path, PurePosixPath
import re
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


def apply_patch(directory: Path, patch: Path) -> None:
    """Apply this repository's text-only unified diff exactly, without Unix tools.

    No fuzz, offset searching, file creation, renames, or traversal is accepted.
    The downloaded archive is independently SHA-256 verified before this runs.
    """
    lines = patch.read_text(encoding="utf-8").splitlines(keepends=True)
    index = 0
    while index < len(lines):
        if not lines[index].startswith("--- a/"):
            raise ValueError("Expected old-file patch header")
        old_name = lines[index][6:].rstrip("\n")
        index += 1
        if index >= len(lines) or lines[index] != f"+++ b/{old_name}\n":
            raise ValueError("Patch renames and file creation are unsupported")
        relative = PurePosixPath(old_name)
        if (
            relative.is_absolute()
            or ".." in relative.parts
            or "\\" in old_name
            or ":" in old_name
        ):
            raise ValueError("Unsafe patch path")
        destination = directory.joinpath(*relative.parts)
        if destination.is_symlink() or not destination.resolve().is_relative_to(
            directory.resolve()
        ):
            raise ValueError("Patch path leaves source directory")
        original = destination.read_text(encoding="utf-8").splitlines(keepends=True)
        output = []
        consumed = 0
        index += 1
        hunks = 0
        while index < len(lines) and lines[index].startswith("@@ "):
            match = re.fullmatch(
                r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@[^\n]*\n", lines[index]
            )
            if not match:
                raise ValueError("Invalid patch hunk")
            old_start, old_count, new_start, new_count = (
                int(value) if value is not None else 1 for value in match.groups()
            )
            offset = old_start - 1
            if offset < consumed or offset > len(original):
                raise ValueError("Patch hunk offset is invalid")
            output.extend(original[consumed:offset])
            if len(output) != new_start - 1:
                raise ValueError("Patch new-file offset differs")
            index += 1
            before, after = [], []
            while (
                index < len(lines)
                and lines[index][:1] in (" ", "+", "-")
                and not lines[index].startswith("--- a/")
            ):
                line = lines[index]
                if line[0] in " -":
                    before.append(line[1:])
                if line[0] in " +":
                    after.append(line[1:])
                index += 1
            if (
                len(before) != old_count
                or len(after) != new_count
                or original[offset : offset + old_count] != before
            ):
                raise ValueError(
                    "Patch context or line count differs from pinned source"
                )
            output.extend(after)
            consumed = offset + old_count
            hunks += 1
        if not hunks:
            raise ValueError("Patch file has no hunks")
        output.extend(original[consumed:])
        destination.write_bytes("".join(output).encode("utf-8"))


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
                "Generated Opus cache differs from the pinned patch; inspect it before rebuilding"
            )
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
    print("Pinned Opus patch: prepared")


if __name__ == "__main__":
    main()
