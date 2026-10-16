"""Fail closed when a binary release would carry stale or incomplete notices."""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("dev", Path(__file__).with_name("dev.py"))
DEV = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DEV)


class ReleaseInventoryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        patch = mock.patch.object(DEV, "ROOT", self.root)
        patch.start()
        self.addCleanup(patch.stop)
        self.generated = self.root / "licenses/generated"
        (self.generated / "texts").mkdir(parents=True)
        (self.generated / "INDEX.md").write_text("# Release dependency notices\n")
        (self.root / "ui").mkdir()
        (self.root / "Cargo.lock").write_bytes(b"locked Rust graph")
        (self.root / "ui/package-lock.json").write_bytes(b"locked frontend graph")
        (self.root / "rust-toolchain.toml").write_text(
            '[toolchain]\nchannel = "1.99.0"\n'
        )
        self.source = self.root / "echocord-0.1.0-third-party-sources.zip"
        self.source.write_bytes(b"verified source ZIP")
        self.notice = self.generated / "texts/notice.txt"
        self.notice.write_bytes(b"Original license and attribution")
        self.inventory = {
            "schema": 1,
            "echocord_version": "0.1.0",
            "target": DEV.TARGET,
            "rust_version": "1.99.0",
            "cargo_lock_sha256": self.digest(self.root / "Cargo.lock"),
            "npm_lock_sha256": self.digest(self.root / "ui/package-lock.json"),
            "source_archive": {
                "name": self.source.name,
                "sha256": self.digest(self.source),
            },
            "packages": [
                {
                    "notices": [
                        {"path": "texts/notice.txt", "sha256": self.digest(self.notice)}
                    ]
                }
            ],
        }
        self.save_inventory()

    @staticmethod
    def digest(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def save_inventory(self):
        (self.generated / "inventory.json").write_text(json.dumps(self.inventory))

    def verify(self):
        return DEV.validated_notices("0.1.0", self.source)

    def test_only_indexed_complete_material_is_selected(self):
        stray = self.generated / "unrelated-local-note.txt"
        stray.write_text("Must not be distributed")
        selected = self.verify()
        self.assertEqual(
            set(selected),
            {
                self.notice,
                self.generated / "INDEX.md",
                self.generated / "inventory.json",
            },
        )
        self.assertNotIn(stray, selected)

    def test_missing_generated_index_is_rejected(self):
        (self.generated / "INDEX.md").unlink()
        (self.root / "licenses/supplements.json").write_text("{}")
        with self.assertRaisesRegex(SystemExit, "Missing generated"):
            self.verify()

    def test_different_version_is_rejected(self):
        self.inventory["echocord_version"] = "0.0.9"
        self.save_inventory()
        with self.assertRaisesRegex(SystemExit, "version"):
            self.verify()

    def test_changed_lockfiles_are_rejected(self):
        for name in ("Cargo.lock", "ui/package-lock.json"):
            with self.subTest(name=name):
                path = self.root / name
                original = path.read_bytes()
                path.write_bytes(original + b"changed dependency")
                with self.assertRaisesRegex(SystemExit, "lockfile changed"):
                    self.verify()
                path.write_bytes(original)

    def test_changed_or_missing_source_archive_is_rejected(self):
        self.source.write_bytes(b"old release sources")
        with self.assertRaisesRegex(SystemExit, "source archive"):
            self.verify()
        self.source.unlink()
        with self.assertRaisesRegex(SystemExit, "source archive"):
            self.verify()

    def test_changed_or_missing_notice_is_rejected(self):
        self.notice.write_bytes(b"Incomplete terms")
        with self.assertRaisesRegex(SystemExit, "indexed notice"):
            self.verify()
        self.notice.unlink()
        with self.assertRaisesRegex(SystemExit, "indexed notice"):
            self.verify()

    def test_notice_traversal_is_rejected(self):
        self.inventory["packages"][0]["notices"][0]["path"] = "../outside.txt"
        self.save_inventory()
        with self.assertRaisesRegex(SystemExit, "unsafe notice path"):
            self.verify()

    def test_changed_dependency_patch_is_rejected(self):
        patches = self.root / "patches"
        patches.mkdir()
        patch = patches / "fixture.patch"
        patch.write_bytes(b"reviewed dependency patch")
        self.inventory["packages"][0]["patch"] = {
            "path": "patches/fixture.patch", "sha256": self.digest(patch)
        }
        self.save_inventory()
        self.verify()
        patch.write_bytes(b"changed dependency patch")
        with self.assertRaisesRegex(SystemExit, "dependency patch changed"):
            self.verify()

    def test_dependency_patch_traversal_is_rejected(self):
        self.inventory["packages"][0]["patch"] = {
            "path": "patches/../../outside.patch", "sha256": "unused"
        }
        self.save_inventory()
        with self.assertRaisesRegex(SystemExit, "unsafe dependency patch"):
            self.verify()


if __name__ == "__main__":
    unittest.main()
