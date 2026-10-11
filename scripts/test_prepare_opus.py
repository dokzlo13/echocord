"""Regression checks for generated dependency cache drift; no network or compiler."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "prepare_opus", Path(__file__).with_name("prepare-opus.py")
)
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


class CacheDriftTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        prepare.ROOT = self.root
        prepare.PATCH = self.root / "change.patch"
        prepare.PATCH.write_text("fixture patch\n")
        self.source = self.root / ".cache" / prepare.PACKAGE
        self.source.mkdir(parents=True)
        (self.source / "build.rs").write_text("fixture build\n")
        (self.source / "encoder.c").write_text("fixture unpatched source\n")
        stamp = {
            "archive": prepare.ARCHIVE_SHA256,
            "patch": prepare.digest(prepare.PATCH),
            "files": prepare.source_digests(self.source),
        }
        (self.source / prepare.STAMP).write_text(json.dumps(stamp))

    def verify(self):
        with contextlib.redirect_stdout(io.StringIO()):
            prepare.main()

    def test_unchanged_cache_is_reusable(self):
        self.verify()

    def test_changed_unpatched_source_is_rejected(self):
        (self.source / "encoder.c").write_text("changed source\n")
        with self.assertRaises(SystemExit):
            self.verify()

    def test_added_source_is_rejected(self):
        (self.source / "extra.c").write_text("untracked source\n")
        with self.assertRaises(SystemExit):
            self.verify()

    def test_removed_source_is_rejected(self):
        (self.source / "encoder.c").unlink()
        with self.assertRaises(SystemExit):
            self.verify()

    def test_symlink_substitution_is_rejected(self):
        external = self.root / "external.c"
        external.write_text("fixture unpatched source\n")
        (self.source / "encoder.c").unlink()
        try:
            (self.source / "encoder.c").symlink_to(external)
        except OSError:
            self.skipTest("Creating symlinks is unavailable for this account")
        with self.assertRaises(SystemExit):
            self.verify()


class PatchTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.source.mkdir()
        (self.source / "file.txt").write_bytes(b"first\nold\nlast\n")
        self.patch = self.root / "change.patch"
        self.patch.write_text(
            "--- a/file.txt\n+++ b/file.txt\n@@ -1,3 +1,3 @@\n first\n-old\n+new\n last\n"
        )

    def test_exact_context_applies_without_external_tools(self):
        prepare.apply_patch(self.source, self.patch)
        self.assertEqual((self.source / "file.txt").read_bytes(), b"first\nnew\nlast\n")

    def test_context_mismatch_is_rejected(self):
        (self.source / "file.txt").write_text("first\nchanged\nlast\n")
        with self.assertRaises(ValueError):
            prepare.apply_patch(self.source, self.patch)

    def test_offset_drift_is_rejected(self):
        (self.source / "file.txt").write_text("extra\nfirst\nold\nlast\n")
        with self.assertRaises(ValueError):
            prepare.apply_patch(self.source, self.patch)

    def test_path_traversal_is_rejected(self):
        self.patch.write_text(self.patch.read_text().replace("file.txt", "../file.txt"))
        with self.assertRaises(ValueError):
            prepare.apply_patch(self.source, self.patch)


if __name__ == "__main__":
    unittest.main()
