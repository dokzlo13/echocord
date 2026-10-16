"""Release-note gates prevent publishing another version's or unfinished changes."""

import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("dev", Path(__file__).with_name("dev.py"))
DEV = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DEV)


class ReleaseNotesTests(unittest.TestCase):
    def test_selects_exact_version_and_retains_subsections(self):
        changelog = """# Changelog

## Unreleased
Do not publish this yet.

## 0.1.0 — 2026-10-11
First public release.

### Added
- A user-facing feature.

## 0.0.9 — 2026-10-01
Old changes.
"""
        notes = DEV.release_notes(changelog, "0.1.0")
        self.assertIn("First public release.", notes)
        self.assertIn("### Added\n- A user-facing feature.", notes)
        self.assertNotIn("Do not publish this yet", notes)
        self.assertNotIn("Old changes", notes)
        self.assertIn("releases/download/v0.1.0/echocord.exe", notes)
        self.assertIn("echocord-0.1.0-third-party-sources.zip", notes)
        self.assertIn("Visual C++ v14 x64", notes)
        self.assertIn("https://github.com/dokzlo13/echocord#readme", notes)

    def test_accepts_link_style_version_heading(self):
        notes = DEV.release_notes("## [0.2.0] - 2026-11-01\n\nChanges.\n", "0.2.0")
        self.assertIn("Changes.", notes)
        self.assertIn("releases/download/v0.2.0/echocord.exe", notes)

    def test_missing_exact_version_is_rejected(self):
        with self.assertRaisesRegex(SystemExit, "exactly one"):
            DEV.release_notes("## 0.1.00\nOther version.\n", "0.1.0")

    def test_duplicate_version_is_rejected(self):
        with self.assertRaisesRegex(SystemExit, "exactly one"):
            DEV.release_notes("## 0.1.0\nFirst.\n## [0.1.0]\nSecond.\n", "0.1.0")

    def test_empty_version_is_rejected(self):
        with self.assertRaisesRegex(SystemExit, "empty version"):
            DEV.release_notes("## 0.1.0\n\n## 0.0.9\nPrevious.\n", "0.1.0")


if __name__ == "__main__":
    unittest.main()
