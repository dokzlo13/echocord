"""Fail closed when promoting CI assets across workflow runs."""

import copy
from contextlib import nullcontext
from datetime import datetime, timezone
import hashlib
import io
import json
import stat
import unittest
from unittest import mock
import zipfile

import release_assets as release

REPO = "owner/echocord"
SHA = "a" * 40
NOW = datetime(2026, 10, 16, 12, 0, tzinfo=timezone.utc)


class ProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.workflow = {"id": 10, "path": release.WORKFLOW, "state": "active"}
        self.run = {
            "id": 20, "workflow_id": 10, "path": release.WORKFLOW,
            "event": "push", "head_branch": "main", "head_sha": SHA,
            "repository": {"id": 30, "full_name": REPO},
            "head_repository": {"id": 30, "full_name": REPO},
            "status": "completed", "conclusion": "success", "run_attempt": 2,
        }
        self.jobs = [{
            "name": name, "run_id": 20, "run_attempt": 2, "head_sha": SHA,
            "status": "completed", "conclusion": "success",
            "started_at": "2026-10-16T10:00:00Z", "completed_at": "2026-10-16T10:20:00Z",
            "steps": [{"name": step, "conclusion": "success"} for step in steps],
        } for name, steps in release.JOBS.items()]
        self.artifact = {
            "id": 40, "name": "release-assets", "expired": False,
            "created_at": "2026-10-16T10:19:00Z", "expires_at": "2026-10-23T10:19:00Z",
            "size_in_bytes": 1000, "digest": "sha256:" + "b" * 64,
            "workflow_run": {"id": 20, "repository_id": 30, "head_repository_id": 30,
                             "head_branch": "main", "head_sha": SHA},
        }
        self.runs = [{"id": 20}]
        self.artifacts = [self.artifact]
        self.api = mock.Mock(side_effect=self.response)

    def response(self, endpoint):
        if endpoint.endswith("/actions/workflows/ci.yml"):
            return self.workflow
        if "/actions/workflows/10/runs?" in endpoint:
            return {"total_count": len(self.runs), "workflow_runs": self.runs}
        if endpoint.endswith("/actions/runs/20"):
            return self.run
        if "/attempts/2/jobs?" in endpoint:
            return {"total_count": len(self.jobs), "jobs": self.jobs}
        if "/actions/runs/20/artifacts?" in endpoint:
            return {"total_count": len(self.artifacts), "artifacts": self.artifacts}
        raise AssertionError(endpoint)

    def checked(self):
        return release.checked_artifact(self.api, REPO, SHA, now=NOW)

    def test_accepts_exact_main_run_and_latest_attempt(self):
        self.assertEqual(self.checked(), {
            "commit": SHA, "run_id": 20, "run_attempt": 2,
            "artifact_id": 40, "artifact_digest": self.artifact["digest"],
        })
        self.assertTrue(any("attempts/2/jobs" in call.args[0] for call in self.api.call_args_list))

    def test_rejects_wrong_sha_workflow_event_branch_repository_or_unsuccessful_run(self):
        for field, value in (
            ("head_sha", "c" * 40), ("workflow_id", 11), ("path", ".github/workflows/other.yml"),
            ("event", "pull_request"), ("event", "workflow_dispatch"), ("head_branch", "feature"),
            ("status", "in_progress"), ("conclusion", "failure"),
            ("head_repository", {"id": 31, "full_name": "fork/echocord"}),
            ("repository", {"id": 31, "full_name": "other/echocord"}),
        ):
            with self.subTest(field=field, value=value):
                original = self.run[field]
                self.run[field] = value
                with self.assertRaises(release.Rejected):
                    self.checked()
                self.run[field] = original

    def test_newest_run_is_not_replaced_by_an_older_success(self):
        self.runs.insert(0, {"id": 19})
        self.run["status"] = "queued"
        with self.assertRaisesRegex(release.Rejected, "not completed"):
            self.checked()

    def test_missing_run_fails(self):
        self.runs.clear()
        with self.assertRaisesRegex(release.Rejected, "No main-push"):
            self.checked()

    def test_required_jobs_cannot_be_missing_duplicated_or_from_previous_attempt(self):
        original = copy.deepcopy(self.jobs)
        for jobs in (original[:1], [original[0], original[0]]):
            self.jobs = jobs
            with self.assertRaisesRegex(release.Rejected, "Both required jobs"):
                self.checked()
        self.jobs = original
        self.jobs[0]["run_attempt"] = 1
        with self.assertRaisesRegex(release.Rejected, "this attempt"):
            self.checked()

    def test_failed_job_and_skipped_gate_fail(self):
        self.jobs[0]["conclusion"] = "failure"
        with self.assertRaisesRegex(release.Rejected, "job did not succeed"):
            self.checked()
        self.jobs[0]["conclusion"] = "success"
        self.jobs[0]["steps"][0]["conclusion"] = "skipped"
        with self.assertRaisesRegex(release.Rejected, "gate did not succeed"):
            self.checked()

    def test_artifact_must_be_unique_current_unexpired_and_digest_bound(self):
        for artifacts in ([], [self.artifact, self.artifact]):
            self.artifacts = artifacts
            with self.assertRaisesRegex(release.Rejected, "unique"):
                self.checked()
        self.artifacts = [self.artifact]
        for field, value in (
            ("expired", True), ("expires_at", "2026-10-16T11:00:00Z"),
            ("created_at", "2026-10-15T10:19:00Z"), ("digest", None),
            ("size_in_bytes", release.MAX_ARCHIVE + 1),
        ):
            with self.subTest(field=field):
                original = self.artifact[field]
                self.artifact[field] = value
                with self.assertRaises(release.Rejected):
                    self.checked()
                self.artifact[field] = original
        self.artifact["workflow_run"]["head_sha"] = "d" * 40
        with self.assertRaisesRegex(release.Rejected, "provenance differs"):
            self.checked()

    def test_truncated_api_response_is_rejected(self):
        with self.assertRaisesRegex(release.Rejected, "Incomplete"):
            release.collection(lambda _: {"total_count": 101, "jobs": [1] * 100}, "example", "jobs")

    def test_peels_annotated_tag_and_rejects_changed_target(self):
        api = mock.Mock(side_effect=[{"object": {"type": "tag", "sha": "f" * 40}},
                                     {"object": {"type": "commit", "sha": SHA}}])
        release.verify_tag(api, REPO, "v0.2.0", SHA)
        with self.assertRaisesRegex(release.Rejected, "tag does not identify"):
            release.verify_tag(lambda _: {"object": {"type": "commit", "sha": "e" * 40}}, REPO, "v0.2.0", SHA)


def archive(files, extra=None):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        for name, data in files.items():
            bundle.writestr(name, data)
        if extra:
            bundle.writestr(*extra)
    return output.getvalue()


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.source = "echocord-0.2.0-third-party-sources.zip"
        self.package = b"exact covered source archive"
        self.manifest = {"echocord_version": "0.2.0", "packages": [{
            "name": "covered", "version": "1.0.0", "license": "MPL-2.0",
            "source_sha256": hashlib.sha256(self.package).hexdigest(),
        }]}
        self.sources = {
            "MANIFEST.json": json.dumps(self.manifest).encode(),
            "README.txt": b"Corresponding sources", "MPL-2.0.txt": b"License terms",
            "crates/covered-1.0.0.crate": self.package,
        }
        self.files = {"echocord.exe": b"MZchecked binary", self.source: archive(self.sources),
                      "RELEASE-NOTES.md": b"Expected tagged notes\n"}
        self.checksums()

    def checksums(self):
        self.files["SHA256SUMS"] = "".join(
            f"{hashlib.sha256(self.files[name]).hexdigest()}  {name}\n"
            for name in ("echocord.exe", self.source)
        ).encode()

    def verify(self, data=None, digest=None):
        data = data if data is not None else archive(self.files)
        digest = digest if digest is not None else "sha256:" + hashlib.sha256(data).hexdigest()
        return release.verify_bundle(data, digest, "0.2.0", "Expected tagged notes\n")

    def test_offline_validation_returns_only_the_four_exact_files(self):
        self.assertEqual(self.verify(), self.files)

    def test_wrong_github_digest_or_internal_checksum_is_rejected(self):
        with self.assertRaisesRegex(release.Rejected, "digest"):
            self.verify(digest="sha256:" + "0" * 64)
        self.files["echocord.exe"] += b"modified"
        with self.assertRaisesRegex(release.Rejected, "checksum mismatch"):
            self.verify()

    def test_unexpected_traversal_duplicate_and_missing_files_are_rejected(self):
        for name in ("extra.txt", "../outside", "echocord.exe"):
            warning = self.assertWarns(UserWarning) if name == "echocord.exe" else nullcontext()
            with self.subTest(name=name), warning:
                data = archive(self.files, (name, b"extra"))
                with self.assertRaisesRegex(release.Rejected, "missing, duplicate or unexpected"):
                    self.verify(data)
        del self.files["RELEASE-NOTES.md"]
        with self.assertRaises(release.Rejected):
            self.verify()

    def test_symlink_asset_is_rejected(self):
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as bundle:
            for name, data in self.files.items():
                entry = zipfile.ZipInfo(name)
                entry.external_attr = (stat.S_IFLNK | 0o777) << 16
                bundle.writestr(entry, data)
        with self.assertRaisesRegex(release.Rejected, "link"):
            self.verify(output.getvalue())

    def test_notes_and_source_version_must_match_tag(self):
        self.files["RELEASE-NOTES.md"] = b"Other release notes\n"
        with self.assertRaisesRegex(release.Rejected, "notes do not match"):
            self.verify()
        self.files["RELEASE-NOTES.md"] = b"Expected tagged notes\n"
        self.manifest["echocord_version"] = "0.1.0"
        self.sources["MANIFEST.json"] = json.dumps(self.manifest).encode()
        self.files[self.source] = archive(self.sources)
        self.checksums()
        with self.assertRaisesRegex(release.Rejected, "version does not match"):
            self.verify()

    def test_source_package_checksum_is_verified_independently(self):
        self.sources["crates/covered-1.0.0.crate"] = b"wrong sources"
        self.files[self.source] = archive(self.sources)
        self.checksums()
        with self.assertRaisesRegex(release.Rejected, "package checksum"):
            self.verify()

    def test_checksum_manifest_cannot_omit_or_add_entries(self):
        self.files["SHA256SUMS"] = self.files["SHA256SUMS"].splitlines(keepends=True)[0]
        with self.assertRaisesRegex(release.Rejected, "inventory differs"):
            self.verify()

if __name__ == "__main__":
    unittest.main()
