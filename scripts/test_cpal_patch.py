"""Verify pinned CPAL cache and real packet code using a device-free Rust mock."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock


spec = importlib.util.spec_from_file_location(
    "prepare_cpal", Path(__file__).with_name("prepare-cpal.py")
)
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)
ROOT = Path(__file__).resolve().parent.parent


class PacketContractTests(unittest.TestCase):
    def test_actual_patched_packet_function(self):
        package = ROOT / ".cache" / prepare.PACKAGE
        self.assertTrue(
            package.is_dir(), "Run python scripts/prepare-cpal.py before this check"
        )
        with contextlib.redirect_stdout(io.StringIO()):
            prepare.main()  # Verify the full prepared source inventory before extracting code.
        source = (package / "src/host/wasapi/stream.rs").read_text(encoding="utf-8")
        start = source.index("fn process_input(")
        end = source.index("// The loop for writing output data.", start)
        packet_function = source[start:end]
        equilibrium = (package / "src/host/equilibrium.rs").read_text(encoding="utf-8")
        harness = (ROOT / "scripts/fixtures/cpal-silent-harness.rs").read_text(
            encoding="utf-8"
        )
        harness = harness.replace(
            "// __UPSTREAM_EQUILIBRIUM__",
            "mod equilibrium {\n" + equilibrium + "\n}\nuse equilibrium::fill_equilibrium;",
        ).replace("// __PATCHED_PROCESS_INPUT__", packet_function)
        with tempfile.TemporaryDirectory(prefix="echocord-cpal-test-") as temporary:
            directory = Path(temporary)
            rust = directory / "packet-contract.rs"
            executable = directory / "packet-contract.exe"
            rust.write_text(harness, encoding="utf-8")
            compiled = subprocess.run(
                [
                    "rustc", "--edition=2024", "--test", "-O", "-D", "warnings",
                    str(rust), "-o", str(executable),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                timeout=60,
            )
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            result = subprocess.run(
                [str(executable)],
                cwd=ROOT,
                capture_output=True,
                text=True,
                timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("4 passed", result.stdout)


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

    def test_cache_reuse_rejects_changed_or_added_files(self):
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
            (package / prepare.STAMP).write_text(json.dumps(stamp), encoding="utf-8")
            with mock.patch.object(prepare, "ROOT", root):
                with contextlib.redirect_stdout(io.StringIO()):
                    prepare.main()
                source.write_text("changed\n")
                with self.assertRaisesRegex(SystemExit, "cache differs"):
                    prepare.main()
                source.write_text("original\n")
                (package / "unexpected.rs").write_text("added\n")
                with self.assertRaisesRegex(SystemExit, "cache differs"):
                    prepare.main()


if __name__ == "__main__":
    unittest.main()
