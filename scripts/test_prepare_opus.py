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
        (self.source / "encoder.c").symlink_to(external)
        with self.assertRaises(SystemExit):
            self.verify()


if __name__ == "__main__":
    unittest.main()
