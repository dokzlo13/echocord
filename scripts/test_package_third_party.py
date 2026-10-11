"""Security and reproducibility checks for release license/source packaging."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile

SPEC = importlib.util.spec_from_file_location("third_party", Path(__file__).with_name("package-third-party.py"))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def tar_bytes(entries):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for name, content, kind in entries:
            entry = tarfile.TarInfo(name)
            entry.type = kind
            if kind == tarfile.REGTYPE:
                entry.size = len(content)
                archive.addfile(entry, io.BytesIO(content))
            else:
                entry.linkname = "../../outside"
                archive.addfile(entry)
    return output.getvalue()


class ArchiveBoundaryTests(unittest.TestCase):
    def test_rejects_traversal_on_both_platforms(self):
        for name in ("pkg/../outside", "/outside", "C:/outside", "pkg\\..\\outside", "pkg/./LICENSE", "pkg//LICENSE"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                MODULE.tar_files(tar_bytes([(name, b"unsafe", tarfile.REGTYPE)]), "pkg")

    def test_rejects_links_and_special_files(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE, tarfile.FIFOTYPE):
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                MODULE.tar_files(tar_bytes([("pkg/link", b"", kind)]), "pkg")

    def test_rejects_duplicate_files_and_mixed_roots(self):
        for entries in (
            [("pkg/LICENSE", b"first", tarfile.REGTYPE), ("pkg/LICENSE", b"second", tarfile.REGTYPE)],
            [("pkg/LICENSE", b"first", tarfile.REGTYPE), ("other/LICENSE", b"second", tarfile.REGTYPE)],
        ):
            with self.assertRaises(ValueError):
                MODULE.tar_files(tar_bytes(entries), None)

    def test_accepts_npm_alternate_root_and_keeps_bytes_unchanged(self):
        content = b"Copyright upstream\r\nMIT terms\r\n"
        actual = MODULE.tar_files(tar_bytes([("react/LICENSE", content, tarfile.REGTYPE)]), None)
        self.assertEqual(actual, {"LICENSE": content})

    def test_sdk_rejects_unsafe_unused_members(self):
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as archive:
            archive.writestr("LICENSE.txt", b"terms")
            archive.writestr("../outside", b"unsafe")
        with self.assertRaises(ValueError):
            MODULE.sdk_files(output.getvalue(), {"LICENSE.txt": hashlib.sha256(b"terms").hexdigest()})

    def test_sdk_rejects_symlinks_and_wrong_license_checksum(self):
        for symlink in (True, False):
            output = io.BytesIO()
            with zipfile.ZipFile(output, "w") as archive:
                entry = zipfile.ZipInfo("LICENSE.txt")
                entry.external_attr = (0o120777 if symlink else 0o100644) << 16
                archive.writestr(entry, b"different")
            with self.assertRaises(ValueError):
                MODULE.sdk_files(output.getvalue(), {"LICENSE.txt": hashlib.sha256(b"terms").hexdigest()})


class IntegrityAndOutputTests(unittest.TestCase):
    def test_corrupt_cargo_cache_fails_without_network(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            candidate = root / "crate.archive"
            candidate.write_bytes(b"corrupted")
            fetcher = MODULE.Fetcher(root / "cache", False)
            with mock.patch.object(MODULE.urllib.request, "urlopen") as network:
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    fetcher.get("https://example.test/package", hashlib.sha256(b"original").hexdigest(), candidates=(candidate,))
                network.assert_not_called()

    def test_offline_uses_only_verified_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            candidate = root / "crate.archive"
            candidate.write_bytes(b"original")
            fetcher = MODULE.Fetcher(root / "cache", True)
            with mock.patch.object(MODULE.urllib.request, "urlopen") as network:
                self.assertEqual(fetcher.get("https://example.test/package", hashlib.sha256(b"original").hexdigest(), candidates=(candidate,)), b"original")
                with self.assertRaisesRegex(ValueError, "not cached"):
                    fetcher.get("https://example.test/absent", "0" * 64)
                network.assert_not_called()

    def test_no_credential_urls_or_plain_http(self):
        with tempfile.TemporaryDirectory() as directory:
            fetcher = MODULE.Fetcher(Path(directory), False)
            for url in ("http://example.test/license", "https://token@example.test/license"):
                with self.assertRaises(ValueError):
                    fetcher.get(url, "0" * 64)

    def test_zip_is_deterministic_and_contains_only_public_names(self):
        with tempfile.TemporaryDirectory() as directory:
            one, two = Path(directory) / "one.zip", Path(directory) / "two.zip"
            MODULE.deterministic_zip(one, {"crates/a.crate": b"a", "MANIFEST.json": b"{}"})
            MODULE.deterministic_zip(two, {"MANIFEST.json": b"{}", "crates/a.crate": b"a"})
            self.assertEqual(one.read_bytes(), two.read_bytes())
            with zipfile.ZipFile(one) as archive:
                self.assertEqual(archive.namelist(), ["MANIFEST.json", "crates/a.crate"])
                self.assertTrue(all(x.date_time == (1980, 1, 1, 0, 0, 0) for x in archive.infolist()))

    def test_missing_license_fails_instead_of_inventing_attribution(self):
        with tempfile.TemporaryDirectory() as directory:
            notices = MODULE.Notices(Path(directory))
            with self.assertRaisesRegex(ValueError, "reviewed supplemental"):
                MODULE.supplements_for("new-dependency@1.0.0", {"rust": {}}, None, notices)
            entry = notices.attribution({"src/lib.rs": b"// Copyright upstream contributor\npub fn test() {}"}, ["Published author"], "MIT")
            content = (Path(directory) / entry["path"]).read_text()
            self.assertIn("src/lib.rs:1: // Copyright upstream contributor", content)
            self.assertNotIn("Copyright Published author", content)


if __name__ == "__main__":
    unittest.main()
