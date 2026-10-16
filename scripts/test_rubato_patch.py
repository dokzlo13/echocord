"""Verify pinned Rubato sizing against extracted real index/read code and cache integrity.

Set ECHOCORD_RUBATO_DENSE=1 for the optional 2-million-case phase sweep.
"""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "prepare_rubato", Path(__file__).with_name("prepare-rubato.py")
)
prepare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prepare)
ROOT = Path(__file__).resolve().parent.parent


def braced_item(source: str, marker: str, start: int = 0) -> str:
    begin = source.index(marker, start)
    opening = source.index("{", begin)
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[begin:end]


class SizingContractTests(unittest.TestCase):
    def test_actual_estimator_and_processing_read_windows(self):
        with contextlib.redirect_stdout(io.StringIO()):
            prepare.main()  # Full source inventory must pass before compiling it.
        package = ROOT / ".cache" / prepare.PACKAGE
        source = (package / "src/asynchro_sinc.rs").read_text(encoding="utf-8")
        archive = ROOT / ".cache" / f"{prepare.PACKAGE}.crate"
        self.assertEqual(prepare.digest(archive), prepare.ARCHIVE_SHA256)
        with tarfile.open(archive) as crate:
            original = crate.extractfile(
                f"{prepare.PACKAGE}/src/asynchro_sinc.rs"
            ).read().decode()
        implementation = source.index("impl<T> SincFixedOut<T>")
        original_implementation = original.index("impl<T> SincFixedOut<T>")
        # The existing recording converter is FixedIn and must remain unchanged.
        self.assertEqual(source[:implementation], original[:original_implementation])
        current_estimate = braced_item(source, "fn update_needed_len(", implementation)
        original_estimate = braced_item(
            original, "fn update_needed_len(", original_implementation
        )
        loop_start = source.index("        let mut idx = self.last_index;", implementation)
        loop_end = source.index("        // Store last index", loop_start)
        nearest = (package / "src/interpolation.rs").read_text(encoding="utf-8")
        nearest = nearest.split("#[cfg(test)]")[0]
        types = braced_item(source, "pub enum SincInterpolationType")
        harness = (ROOT / "scripts/fixtures/rubato-sizing-harness.rs").read_text(
            encoding="utf-8"
        )
        harness = harness.replace(
            "// __INTERPOLATION_TYPES__", "#[derive(Clone, Copy)]\n" + types
        )
        harness = harness.replace("// __NEAREST_TIMES__", nearest)
        harness = harness.replace(
            "// __ORIGINAL_ESTIMATE__",
            original_estimate.replace("fn update_needed_len", "fn old_update_needed_len"),
        )
        harness = harness.replace(
            "// __ACTUAL_INDEX_AND_LOOKUP_LOOP__", source[loop_start:loop_end]
        )
        with tempfile.TemporaryDirectory(prefix="echocord-rubato-test-") as temporary:
            directory = Path(temporary)
            implementations = [("patched", current_estimate), ("unpatched", original_estimate)]
            dense = os.environ.get("ECHOCORD_RUBATO_DENSE") == "1"
            for name, estimate in implementations:
                with self.subTest(implementation=name):
                    rust = directory / f"{name}.rs"
                    executable = directory / f"{name}.exe"
                    rust.write_text(
                        harness.replace("// __PATCHED_ESTIMATE__", estimate), encoding="utf-8"
                    )
                    compiled = subprocess.run(
                        [
                            "rustc", "--edition=2024", "--test", "-O", "-D", "warnings",
                            str(rust), "-o", str(executable),
                        ],
                        cwd=ROOT, capture_output=True, text=True, timeout=60,
                    )
                    self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
                    args = [str(executable)]
                    if name == "unpatched":
                        args += [
                            "known_old_ramp_estimate_reads_beyond_initialized_input", "--exact"
                        ]
                    elif dense:
                        args += ["--include-ignored", "--nocapture"]
                    result = subprocess.run(
                        args, cwd=ROOT, capture_output=True, text=True, timeout=120
                    )
                    if name == "patched":
                        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                        self.assertIn("5 passed" if dense else "4 passed", result.stdout)
                        if dense:
                            print(result.stdout)
                    else:
                        self.assertNotEqual(
                            result.returncode, 0, "Unpatched negative control unexpectedly passed"
                        )
                        self.assertIn("FAILED", result.stdout)


class CacheIntegrityTests(unittest.TestCase):
    def test_archive_checksum_is_checked_before_extraction(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cache = root / ".cache"
            cache.mkdir()
            (cache / f"{prepare.PACKAGE}.crate").write_bytes(b"invalid archive")
            with mock.patch.object(prepare, "ROOT", root):
                with self.assertRaisesRegex(SystemExit, "checksum mismatch"):
                    prepare.main()
            self.assertFalse((cache / prepare.PACKAGE).exists())

    def test_cache_reuse_rejects_changed_added_deleted_and_rebound_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / ".cache" / prepare.PACKAGE
            package.mkdir(parents=True)
            source = package / "fixture.rs"
            source.write_text("original\n")
            stamp = {
                "archive": prepare.ARCHIVE_SHA256,
                "patch": prepare.digest(prepare.PATCH),
                "files": prepare.source_digests(package),
            }
            stamp_path = package / prepare.STAMP
            stamp_path.write_text(json.dumps(stamp), encoding="utf-8")
            with mock.patch.object(prepare, "ROOT", root):
                with contextlib.redirect_stdout(io.StringIO()):
                    prepare.main()
                for replacement in ["changed\n", None]:
                    with self.subTest(source=replacement):
                        if replacement is None:
                            source.unlink()
                        else:
                            source.write_text(replacement)
                        with self.assertRaisesRegex(SystemExit, "cache differs"):
                            prepare.main()
                        source.write_text("original\n")
                unexpected = package / "unexpected.rs"
                unexpected.write_text("added\n")
                with self.assertRaisesRegex(SystemExit, "cache differs"):
                    prepare.main()
                unexpected.unlink()
                for key in ["archive", "patch"]:
                    with self.subTest(stamp=key):
                        changed = dict(stamp)
                        changed[key] = "0" * 64
                        stamp_path.write_text(json.dumps(changed), encoding="utf-8")
                        with self.assertRaisesRegex(SystemExit, "cache differs"):
                            prepare.main()


if __name__ == "__main__":
    unittest.main()
