#!/usr/bin/env python3
"""Promote only an exact, successful main-push CI artifact; never build or publish."""

import argparse
from datetime import datetime, timezone
import hashlib
import io
import json
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import zipfile

import dev

WORKFLOW = ".github/workflows/ci.yml"
JOBS = {
    "Portable core and UI": ("Check core and UI",),
    "Windows x64 build and native core": (
        "Check native Rust", "Check application Rust", "Build Windows application",
        "Test application unit behavior", "Package checked Windows release",
    ),
}
WINDOWS_JOB = "Windows x64 build and native core"
MAX_ARCHIVE = 512 * 1024 * 1024
MAX_SOURCES = 64 * 1024 * 1024
REMEDY = (
    "Wait for or rerun ALL jobs of the CI push run for this exact main commit, "
    "then rerun the unpublished release workflow. Do not use another commit's "
    "artifact or a local binary."
)


class Rejected(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise Rejected(message)


def timestamp(value):
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(parsed.tzinfo is not None, "Missing timestamp timezone")
    return parsed


def gh_api(endpoint):
    result = subprocess.run(
        ["gh", "api", "--hostname", "github.com", "-H", "Accept: application/vnd.github+json",
         "-H", "X-GitHub-Api-Version: 2022-11-28", endpoint],
        capture_output=True, check=False, timeout=60,
    )
    require(result.returncode == 0, "GitHub provenance request failed")
    return json.loads(result.stdout)


def collection(api, endpoint, field):
    response = api(endpoint + ("&" if "?" in endpoint else "?") + "per_page=100")
    values = response[field]
    require(isinstance(values, list) and response["total_count"] == len(values),
            "Incomplete or unexpectedly large GitHub response")
    return values


def verify_tag(api, repository, tag, commit):
    obj = api(f"repos/{repository}/git/ref/tags/{tag}")["object"]
    for _ in range(4):
        if obj["type"] == "commit":
            require(obj["sha"] == commit, "Remote tag does not identify the checked commit")
            return
        require(obj["type"] == "tag" and re.fullmatch(r"[0-9a-f]{40}", obj["sha"]),
                "Invalid tag object")
        obj = api(f"repos/{repository}/git/tags/{obj['sha']}")["object"]
    raise Rejected("Excessively nested annotated tag")


def checked_artifact(api, repository, commit, run_id=None, now=None):
    now = now or datetime.now(timezone.utc)
    workflow = api(f"repos/{repository}/actions/workflows/ci.yml")
    require(workflow["path"] == WORKFLOW and workflow["state"] == "active",
            "Expected active CI workflow is unavailable")
    if run_id is None:
        runs = collection(api, f"repos/{repository}/actions/workflows/{workflow['id']}/runs"
                          f"?event=push&branch=main&head_sha={commit}", "workflow_runs")
        require(runs, "No main-push CI run exists for the tagged commit")
        # Never fall back to an older success when a newer run is pending/failed.
        run_id = max(run["id"] for run in runs)
    require(type(run_id) is int and run_id > 0, "Invalid CI run ID")
    run = api(f"repos/{repository}/actions/runs/{run_id}")
    require(run["id"] == run_id and run["workflow_id"] == workflow["id"]
            and run["path"] == WORKFLOW, "Run belongs to a different workflow")
    require(run["event"] == "push" and run["head_branch"] == "main"
            and run["head_sha"] == commit, "Run is not the exact main-push commit")
    require(run["repository"]["full_name"].lower() == repository.lower()
            and run["head_repository"]["full_name"].lower() == repository.lower()
            and run["repository"]["id"] == run["head_repository"]["id"],
            "CI run belongs to another repository")
    require(run["status"] == "completed" and run["conclusion"] == "success",
            "Latest CI attempt has not completed successfully")
    attempt = run["run_attempt"]
    require(type(attempt) is int and attempt > 0, "Invalid CI attempt")
    jobs = collection(api, f"repos/{repository}/actions/runs/{run_id}/attempts/{attempt}/jobs", "jobs")
    require(len(jobs) == len(JOBS) and {j["name"] for j in jobs} == set(JOBS),
            "Both required jobs must belong to the latest CI attempt; rerun all jobs")
    for job in jobs:
        require(job["run_id"] == run_id and job["run_attempt"] == attempt
                and job["head_sha"] == commit and job["status"] == "completed"
                and job["conclusion"] == "success", "Required CI job did not succeed in this attempt")
        for name in JOBS[job["name"]]:
            steps = [step for step in job["steps"] if step["name"] == name]
            require(len(steps) == 1 and steps[0]["conclusion"] == "success",
                    f"Required CI gate did not succeed: {name}")
    windows = next(job for job in jobs if job["name"] == WINDOWS_JOB)
    artifacts = collection(api, f"repos/{repository}/actions/runs/{run_id}/artifacts", "artifacts")
    matches = [artifact for artifact in artifacts if artifact["name"] == "release-assets"]
    require(len(matches) == 1, "Expected one unique release-assets artifact")
    artifact = matches[0]
    require(type(artifact["id"]) is int and artifact["id"] > 0, "Invalid artifact ID")
    origin = artifact["workflow_run"]
    require(origin["id"] == run_id and origin["head_sha"] == commit
            and origin["head_branch"] == "main"
            and origin["repository_id"] == run["repository"]["id"]
            and origin["head_repository_id"] == run["repository"]["id"],
            "Artifact provenance differs from the checked run")
    require(artifact["expired"] is False and timestamp(artifact["expires_at"]) > now,
            "Checked artifact has expired")
    require(timestamp(windows["started_at"]) <= timestamp(artifact["created_at"])
            <= timestamp(windows["completed_at"]), "Artifact is from a different CI attempt")
    require(0 < artifact["size_in_bytes"] <= MAX_ARCHIVE, "Artifact exceeds the download bound")
    require(re.fullmatch(r"sha256:[0-9a-f]{64}", artifact["digest"] or ""),
            "Artifact lacks a verifiable SHA-256 digest")
    return {
        "commit": commit, "run_id": run_id, "run_attempt": attempt,
        "artifact_id": artifact["id"], "artifact_digest": artifact["digest"],
    }


def zip_contents(data, expected, limit):
    require(len(data) <= limit, "Archive exceeds size bound")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        names = [entry.filename for entry in entries]
        require(len(names) == len(set(names)) and set(names) == expected,
                "Archive has missing, duplicate or unexpected files")
        require(sum(entry.file_size for entry in entries) <= limit, "Unpacked archive exceeds size bound")
        for entry in entries:
            require(not entry.is_dir() and stat.S_IFMT(entry.external_attr >> 16) in (0, stat.S_IFREG)
                    and not entry.flag_bits & 1, "Archive contains a link, special or encrypted file")
        return {entry.filename: archive.read(entry) for entry in entries}


def verify_sources(data, version):
    require(len(data) <= MAX_SOURCES, "Source archive exceeds size bound")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        manifests = [entry for entry in archive.infolist() if entry.filename == "MANIFEST.json"]
        require(len(manifests) == 1 and manifests[0].file_size <= 1024 * 1024,
                "Invalid source manifest")
        manifest = json.loads(archive.read(manifests[0]))
    require(manifest["echocord_version"] == version, "Source archive version does not match the tag")
    packages = manifest["packages"]
    require(isinstance(packages, list) and packages, "Corresponding-source inventory is empty")
    expected = {"MANIFEST.json", "MPL-2.0.txt", "README.txt"}
    hashes = {}
    for package in packages:
        require(re.fullmatch(r"[A-Za-z0-9_-]+", package["name"])
                and re.fullmatch(r"[A-Za-z0-9.+-]+", package["version"])
                and "MPL-2.0" in package["license"]
                and re.fullmatch(r"[0-9a-f]{64}", package["source_sha256"]),
                "Invalid corresponding-source package")
        name = f"crates/{package['name']}-{package['version']}.crate"
        require(name not in hashes, "Duplicate corresponding-source package")
        expected.add(name)
        hashes[name] = package["source_sha256"]
    contents = zip_contents(data, expected, MAX_SOURCES)
    for name, digest in hashes.items():
        require(hashlib.sha256(contents[name]).hexdigest() == digest,
                "Corresponding-source package checksum mismatch")


def verify_bundle(data, digest, version, notes):
    require(hashlib.sha256(data).hexdigest() == digest.removeprefix("sha256:"),
            "Downloaded artifact digest does not match GitHub")
    source = f"echocord-{version}-third-party-sources.zip"
    files = zip_contents(data, {"echocord.exe", source, "SHA256SUMS", "RELEASE-NOTES.md"}, MAX_ARCHIVE)
    require(files["echocord.exe"].startswith(b"MZ"), "Missing Windows executable header")
    require(files["RELEASE-NOTES.md"] == notes.encode("utf-8"),
            "Release notes do not match the tagged changelog and version")
    require(len(files["SHA256SUMS"]) <= 1024, "Oversized checksum file")
    checksums = {}
    for line in files["SHA256SUMS"].decode("ascii").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
        require(match is not None and match[2] not in checksums, "Invalid or duplicate checksum entry")
        checksums[match[2]] = match[1]
    require(set(checksums) == {"echocord.exe", source}, "Checksum inventory differs from public assets")
    for name, expected in checksums.items():
        require(hashlib.sha256(files[name]).hexdigest() == expected, "Release file checksum mismatch")
    verify_sources(files[source], version)
    return files


def download_archive(repository, artifact_id, path):
    with path.open("wb") as output:
        result = subprocess.run(
            ["gh", "api", "--hostname", "github.com",
             f"repos/{repository}/actions/artifacts/{artifact_id}/zip"],
            stdout=output, stderr=subprocess.PIPE, check=False, timeout=120,
        )
    require(result.returncode == 0, "Cannot download the immutable checked artifact")
    require(0 < path.stat().st_size <= MAX_ARCHIVE, "Downloaded artifact exceeds size bound")
    return path.read_bytes()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("resolve", "download"))
    parser.add_argument("--repository", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--run-id", type=int)
    parser.add_argument("--run-attempt", type=int)
    parser.add_argument("--artifact-id", type=int)
    parser.add_argument("--artifact-digest")
    parser.add_argument("--output", type=Path, default=Path("artifacts/release"))
    args = parser.parse_args()
    try:
        require(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repository), "Invalid repository")
        require(re.fullmatch(r"[0-9a-f]{40}", args.commit), "Invalid source commit")
        require(re.fullmatch(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", args.tag), "Invalid release tag")
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        require(head == args.commit, "Checkout differs from the checked source commit")
        verify_tag(gh_api, args.repository, args.tag, args.commit)
        selected = checked_artifact(gh_api, args.repository, args.commit, args.run_id)
        if args.operation == "resolve":
            for name, value in selected.items():
                print(f"{name}={value}")
            return
        require(selected == {
            "commit": args.commit, "run_id": args.run_id, "run_attempt": args.run_attempt,
            "artifact_id": args.artifact_id, "artifact_digest": args.artifact_digest,
        }, "Checked CI provenance changed before publication")
        version = args.tag[1:]
        notes = dev.release_notes((dev.ROOT / "CHANGELOG.md").read_text(encoding="utf-8"), version)
        require(not args.output.exists(), "Release output must be a new directory")
        with tempfile.TemporaryDirectory(prefix="echocord-release-") as temporary:
            data = download_archive(args.repository, args.artifact_id, Path(temporary) / "artifact.zip")
            files = verify_bundle(data, args.artifact_digest, version, notes)
            # A rerun, deletion or tag move during transfer must not be ignored.
            require(checked_artifact(gh_api, args.repository, args.commit, args.run_id) == selected,
                    "Checked CI provenance changed during download")
            verify_tag(gh_api, args.repository, args.tag, args.commit)
            args.output.mkdir(parents=True)
            for name, content in files.items():
                (args.output / name).write_bytes(content)
        print(f"Verified CI run {args.run_id}, attempt {args.run_attempt}, artifact {args.artifact_id} for {args.commit}")
    except (Rejected, KeyError, TypeError, OSError, ValueError, zipfile.BadZipFile,
            subprocess.SubprocessError) as error:
        print(f"Release artifact rejected: {error}. {REMEDY}", file=sys.stderr)
        raise SystemExit(1) from None


if __name__ == "__main__":
    main()
