"""Offline release notices remain complete and tied to the actual executable."""

import hashlib
import json
from pathlib import Path
import unittest
from unittest import mock

from test_release_inventory import DEV, ReleaseInventoryTests


class EmbeddedLicenseTests(ReleaseInventoryTests):
    def setUp(self):
        super().setUp()
        (self.root / "LICENSE").write_text("Echocord MIT copyright and terms\n")
        (self.root / "THIRD_PARTY_NOTICES.md").write_text("MPL sources remain available.\n")
        package = self.inventory["packages"][0]
        package.update(name="fixture-package", version="1.2.3")
        package["notices"][0]["original_name"] = "LICENSE"
        self.save_inventory()
        self.target = self.root / "target"
        self.release = self.target / DEV.TARGET / "release"
        self.release.mkdir(parents=True)
        patch = mock.patch.object(DEV, "TARGET_DIR", self.target)
        patch.start()
        self.addCleanup(patch.stop)

    def assets(self):
        return DEV.license_assets("0.1.0", self.verify())

    def test_catalog_resolves_offline_and_retains_exact_originals(self):
        assets = self.assets()
        catalog = json.loads(assets["catalog.json"])
        self.assertEqual(catalog["version"], "0.1.0")
        self.assertEqual(catalog["sourceUrl"], "https://github.com/dokzlo13/echocord/releases/download/v0.1.0/echocord-0.1.0-third-party-sources.zip")
        for document in catalog["documents"]:
            self.assertTrue(document["path"].startswith("/licenses/"))
            self.assertIn(document["path"][len("/licenses/"):], assets)
        self.assertEqual(assets["generated/texts/notice.txt"], self.notice.read_bytes())
        self.assertEqual(self.assets(), assets)

    def test_unindexed_material_is_not_embedded(self):
        (self.generated / "private-note.txt").write_text("must stay local")
        self.assertNotIn(b"must stay local", self.assets().values())

    def test_html_notice_has_readable_text_and_byte_exact_plain_text_original(self):
        original = (
            "<!doctype html>\r\n<style>hidden CSS</style><h1>Copyright © &amp; terms</h1>"
            "<p>Permission is granted.</p>\r\n"
        ).encode("utf-8")
        for extension in ("html", "htm", "HTML"):
            with self.subTest(extension=extension):
                relative = f"texts/notice.{extension}"
                path = self.generated / relative
                path.write_bytes(original)
                digest = self.digest(path)
                self.inventory["packages"][0]["notices"] = [{
                    "path": relative, "sha256": digest,
                    "original_name": f"COPYRIGHT.{extension}",
                }]
                self.save_inventory()
                assets = self.assets()
                embedded = f"generated/{relative}.txt"
                self.assertEqual(assets[embedded], original)
                self.assertEqual(hashlib.sha256(assets[embedded]).hexdigest(), digest)
                self.assertNotIn(f"generated/{relative}", assets)
                self.assertFalse(any(Path(name).suffix.lower() in (".html", ".htm") for name in assets))
                catalog = json.loads(assets["catalog.json"])
                originals = [d for d in catalog["documents"] if d["title"].endswith("(original HTML)")]
                self.assertEqual([d["path"] for d in originals], [f"/licenses/{embedded}"])
                readable = assets[f"readable/{digest}.txt"].decode()
                self.assertIn("Copyright © & terms", readable)
                self.assertIn("Permission is granted.", readable)
                self.assertNotIn("hidden CSS", readable)
                self.assertNotIn("<p>", readable)

    def save_receipt(self, binary=b"fixture EXE"):
        receipt = DEV.receipt_data("0.1.0", binary, self.assets())
        DEV.build_receipt_path().write_text(json.dumps(receipt))
        return binary

    def test_receipt_accepts_the_same_executable_and_notices(self):
        binary = self.save_receipt()
        DEV.validated_build_receipt("0.1.0", binary, self.assets())

    def test_receipt_rejects_missing_receipt_changed_exe_and_notice(self):
        with self.assertRaisesRegex(SystemExit, "Missing or invalid"):
            DEV.validated_build_receipt("0.1.0", b"EXE", self.assets())
        binary = self.save_receipt()
        with self.assertRaisesRegex(SystemExit, "stale"):
            DEV.validated_build_receipt("0.1.0", binary + b"changed", self.assets())
        assets = self.assets()
        assets["LICENSE.txt"] += b"changed notice"
        with self.assertRaisesRegex(SystemExit, "stale"):
            DEV.validated_build_receipt("0.1.0", binary, assets)

    def test_receipt_rejects_changed_lock_even_with_same_notice_bytes(self):
        binary = self.save_receipt()
        assets = self.assets()
        (self.root / "Cargo.lock").write_text("new Rust dependencies")
        with self.assertRaisesRegex(SystemExit, "stale"):
            DEV.validated_build_receipt("0.1.0", binary, assets)

    def test_digest_covers_names_boundaries_and_contents(self):
        self.assertNotEqual(DEV.asset_digest({"a": b"bc"}), DEV.asset_digest({"ab": b"c"}))
        self.assertNotEqual(DEV.asset_digest({"a": b"bc"}), DEV.asset_digest({"a": b"bd"}))
        self.assertEqual(DEV.asset_digest({"a": b"b", "c": b"d"}), DEV.asset_digest({"c": b"d", "a": b"b"}))

    def test_package_only_checksums_public_downloads_and_keeps_notes_internal(self):
        binary = self.save_receipt()
        (self.release / "echocord.exe").write_bytes(binary)
        output = self.root / "artifacts/release"
        output.mkdir(parents=True)
        source = output / self.source.name
        source.write_bytes(self.source.read_bytes())
        (self.root / "CHANGELOG.md").write_text("## 0.1.0\n\nFixture change.\n")
        with mock.patch.object(DEV, "version", return_value="0.1.0"):
            DEV.package()
        self.assertEqual((output / "echocord.exe").read_bytes(), binary)
        sums = (output / "SHA256SUMS").read_text().splitlines()
        self.assertEqual(len(sums), 2)
        for line in sums:
            digest, name = line.split("  ")
            self.assertIn(name, ("echocord.exe", source.name))
            self.assertEqual(digest, hashlib.sha256((output / name).read_bytes()).hexdigest())
        self.assertIn("Fixture change", (output / "RELEASE-NOTES.md").read_text())
        self.assertEqual({p.name for p in output.iterdir()}, {"echocord.exe", source.name, "SHA256SUMS", "RELEASE-NOTES.md"})


if __name__ == "__main__":
    unittest.main()
