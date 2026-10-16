"""A cache may reuse changed dependencies, never changed native build policy."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location(
    "ci_cache_context", Path(__file__).with_name("ci-cache-context.py")
)
cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache)


class CacheContextTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.files = cache.POLICY_FILES | cache.DEPENDENCY_FILES | {
            "crates/core/Cargo.toml", "src-tauri/Cargo.toml",
            "scripts/prepare-opus.py", "scripts/prepare-cpal.py",
            "scripts/prepare-rubato.py", "patches/native.patch",
            "crates/core/src/lib.rs",
        }
        for name in self.files:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"fixture {name}\n", encoding="utf-8")
        self.env = {
            "GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
            "RUNNER_OS": "Windows", "RUNNER_ARCH": "X64",
            "ImageOS": "win22", "ImageVersion": "20261012.333.1",
            "GITHUB_WORKSPACE": str(self.root), "CARGO_HOME": str(self.root / "cargo"),
        }

    def context(self, env=None):
        return cache.cache_context(self.root, self.env if env is None else env, self.files)

    def test_application_version_and_dependency_changes_reuse_only_compatible_prefix(self):
        before = self.context()
        self.assertEqual(before["usable"], "true")
        for name in ("Cargo.toml", "Cargo.lock", "src-tauri/tauri.conf.json", "crates/core/Cargo.toml"):
            with self.subTest(name=name):
                path = self.root / name
                path.write_text(path.read_text() + "new version or dependency\n")
                after = self.context()
                self.assertEqual(after["prefix"], before["prefix"])
                self.assertNotEqual(after["key"], before["key"])
                self.assertTrue(before["key"].startswith(after["prefix"]))
                before = after

    def test_source_only_changes_and_unrelated_environment_keep_exact_cache_key(self):
        before = self.context()
        (self.root / "crates/core/src/lib.rs").write_text("new application code\n")
        after = self.context({**self.env, "GITHUB_RUN_ID": "other", "UNRELATED_TEST_SECRET": "synthetic-sensitive-marker"})
        self.assertEqual(after, before)
        self.assertNotIn("synthetic-sensitive-marker", json.dumps(after))

    def test_native_environment_changes_cannot_restore_previous_compiled_outputs(self):
        before = self.context()
        for name, value in {
            "ImageVersion": "20261019.1.0", "ImageOS": "win25",
            "GITHUB_WORKSPACE": "D:/different/check-out", "CARGO_HOME": "D:/other/cargo",
            "_CL_": "/pathmap:old=new", "CL": "/O2", "_LINK_": "/DEBUG",
            "CFLAGS_x86_64-pc-windows-msvc": "/different-native-flag",
            "TARGET_CXXFLAGS": "/O1", "CMAKE_GENERATOR_TOOLSET": "changed",
            "CMAKE_TOOLCHAIN_FILE_x86_64-pc-windows-msvc": "custom-toolchain",
            "RUSTFLAGS": "-C target-cpu=native", "RUSTUP_TOOLCHAIN": "other",
            "OPUS_PACKAGE_VERSION": "different", "CARGO_PROFILE_RELEASE_LTO": "true",
        }.items():
            with self.subTest(name=name):
                after = self.context({**self.env, name: value})
                self.assertNotEqual(after["prefix"], before["prefix"])
                self.assertFalse(before["key"].startswith(after["prefix"]))
                self.assertNotIn(value, after["key"])

    def test_toolchain_build_scripts_and_patches_are_hard_restore_boundaries(self):
        before = self.context()
        for name in (
            "rust-toolchain.toml", "scripts/dev.py", "scripts/ci-cache-context.py",
            "scripts/prepare-rubato.py", "patches/native.patch", ".github/workflows/ci.yml",
        ):
            with self.subTest(name=name):
                path = self.root / name
                original = path.read_text()
                path.write_text(original + "changed native policy\n")
                after = self.context()
                self.assertFalse(before["key"].startswith(after["prefix"]))
                path.write_text(original)
        name = ".cargo/config.toml"
        self.files.add(name)
        (self.root / ".cargo").mkdir()
        (self.root / name).write_text("[env]\n_CL_ = '/new-flag'\n")
        self.assertNotEqual(self.context()["prefix"], before["prefix"])

    def test_missing_or_unknown_runner_context_disables_only_caching(self):
        for name in self.env.keys() - {"CARGO_HOME"}:
            with self.subTest(missing=name):
                self.assertEqual(self.context({k: v for k, v in self.env.items() if k != name}), {"usable": "false"})
        for name, value in (("RUNNER_ENVIRONMENT", "self-hosted"), ("RUNNER_ARCH", "ARM64"), ("RUNNER_OS", "Linux")):
            self.assertEqual(self.context({**self.env, name: value}), {"usable": "false"})
        self.files.remove("scripts/ci-cache-context.py")
        self.assertEqual(self.context(), {"usable": "false"})

    def test_context_is_deterministic_and_ignores_untracked_build_fixtures(self):
        before = self.context()
        nested = self.root / "target/fixture/Cargo.toml"
        nested.parent.mkdir(parents=True)
        nested.write_text("untracked build artifact\n")
        env = {name.upper(): value for name, value in reversed(list(self.env.items()))}
        after = cache.cache_context(self.root, env, reversed(sorted(self.files)))
        self.assertEqual(after, before)
        self.assertLess(len(after["key"]), 512)

    def test_cli_emits_only_hashes_and_preserves_other_step_outputs(self):
        output = self.root / "outputs"
        output.write_text("earlier=kept\n")
        env = {**self.env, "GITHUB_OUTPUT": str(output), "_CL_": "synthetic-sensitive-flag"}
        tracked = ("\0".join(sorted(self.files)) + "\0").encode()
        stdout = io.StringIO()
        with (
            mock.patch.object(cache, "ROOT", self.root),
            mock.patch.dict(os.environ, env, clear=True),
            mock.patch.object(cache.subprocess, "check_output", return_value=tracked),
            contextlib.redirect_stdout(stdout),
        ):
            cache.main()
        result = output.read_text()
        self.assertTrue(result.startswith("earlier=kept\nusable=true\n"))
        self.assertIn("prefix=cargo-v2-Windows-X64-", result)
        self.assertNotIn("synthetic-sensitive", result + stdout.getvalue())
        self.assertNotIn(str(self.root), result + stdout.getvalue())

    def test_cli_missing_policy_file_skips_cache_without_failing_build_job(self):
        (self.root / "scripts/dev.py").unlink()
        output = self.root / "outputs"
        tracked = ("\0".join(sorted(self.files)) + "\0").encode()
        stdout = io.StringIO()
        with (
            mock.patch.object(cache, "ROOT", self.root),
            mock.patch.dict(os.environ, {**self.env, "GITHUB_OUTPUT": str(output)}, clear=True),
            mock.patch.object(cache.subprocess, "check_output", return_value=tracked),
            contextlib.redirect_stdout(stdout),
        ):
            cache.main()
        self.assertEqual(output.read_text(), "usable=false\n")
        self.assertIn("build checks still run", stdout.getvalue())


if __name__ == "__main__":
    unittest.main()
