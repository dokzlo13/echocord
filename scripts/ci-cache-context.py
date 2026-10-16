#!/usr/bin/env python3
"""Bound compiled-cache reuse to matching native inputs; never skip build gates."""

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import subprocess

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"
POLICY_FILES = {
    "rust-toolchain.toml", "scripts/dev.py", "scripts/ci-cache-context.py",
    ".github/workflows/ci.yml", ".node-version",
}
DEPENDENCY_FILES = {"Cargo.lock", "Cargo.toml", "src-tauri/tauri.conf.json"}
BUILD_ENV = set("""
    RUSTFLAGS CARGO_ENCODED_RUSTFLAGS RUSTC RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
    RUSTUP_TOOLCHAIN RUSTUP_HOME CL _CL_ LINK _LINK_ INCLUDE LIB LIBPATH
    VCINSTALLDIR VCTOOLSINSTALLDIR VCTOOLSVERSION VSINSTALLDIR WINDOWSSDKDIR
    WINDOWSSDKVERSION WINDOWSSDKLIBVERSION UCRTVERSION UNIVERSALCRTSDKDIR
    CMAKE CMAKE_GENERATOR CMAKE_GENERATOR_PLATFORM CMAKE_GENERATOR_TOOLSET
    CMAKE_GENERATOR_INSTANCE CMAKE_TOOLCHAIN_FILE CMAKE_PREFIX_PATH
    OPUS_LIB_DIR LIBOPUS_LIB_DIR OPUS_BUNDLED LIBOPUS_BUNDLED OPUS_STATIC
    LIBOPUS_STATIC OPUS_PACKAGE_VERSION
""".split())
COMPILER_ENV = {"CC", "CXX", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "AR", "ARFLAGS", "RANLIB"}


def native_input(name):
    return (
        name in BUILD_ENV
        or name.startswith(("CARGO_TARGET_", "CARGO_PROFILE_", "CMAKE_"))
        or any(
            name == base or name.startswith(base + "_")
            or name in ("HOST_" + base, "TARGET_" + base)
            for base in COMPILER_ENV
        )
    )


def file_digest(root, names):
    digest = hashlib.sha256()
    for name in sorted(names):
        path = root / name
        if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
            raise ValueError("Cache input must be an owned repository file")
        data = path.read_bytes()
        digest.update(name.encode("utf-8") + b"\0")
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)
    return digest.hexdigest()


def cache_context(root, environment, tracked):
    # Windows environment names are case-insensitive; tests run on both hosts.
    env = {name.upper(): value for name, value in environment.items()}
    if (
        env.get("GITHUB_ACTIONS") != "true"
        or env.get("RUNNER_ENVIRONMENT") != "github-hosted"
        or env.get("RUNNER_OS") != "Windows"
        or env.get("RUNNER_ARCH") != "X64"
        or not all(env.get(name) for name in ("IMAGEOS", "IMAGEVERSION", "GITHUB_WORKSPACE"))
    ):
        return {"usable": "false"}
    tracked = set(tracked)
    if not (POLICY_FILES | DEPENDENCY_FILES).issubset(tracked):
        return {"usable": "false"}
    policy = {
        name for name in tracked
        if name in POLICY_FILES or name.startswith(("patches/", ".cargo/"))
        or (name.startswith("scripts/prepare-") and name.endswith(".py"))
    }
    dependencies = {
        name for name in tracked
        if name in DEPENDENCY_FILES or PurePosixPath(name).name == "Cargo.toml"
    }
    native = {
        "image": [env["IMAGEOS"], env["IMAGEVERSION"]],
        "target": TARGET,
        "workspace": str(root.resolve()),
        "runnerWorkspace": env["GITHUB_WORKSPACE"],
        "cargoHome": str(Path(env.get("CARGO_HOME", Path.home() / ".cargo")).resolve()),
        "buildEnv": {name: value for name, value in env.items() if native_input(name)},
        "policy": file_digest(root, policy),
    }
    encoded = json.dumps(native, sort_keys=True, separators=(",", ":")).encode("utf-8")
    compatibility = hashlib.sha256(encoded).hexdigest()
    prefix = f"cargo-v2-Windows-X64-{TARGET}-{compatibility}-"
    return {"usable": "true", "prefix": prefix, "key": prefix + file_digest(root, dependencies)}


def main():
    try:
        # Only tracked files participate: ignored build/cache fixtures cannot change keys.
        tracked = subprocess.check_output(
            ["git", "ls-files", "-z"], cwd=ROOT, stderr=subprocess.DEVNULL,
        ).decode("utf-8").split("\0")
        values = cache_context(ROOT, os.environ, filter(None, tracked))
    except (OSError, ValueError, subprocess.SubprocessError):
        # Caching is optional. Missing context never disables the actual build/check steps.
        values = {"usable": "false"}
    lines = "".join(f"{name}={value}\n" for name, value in values.items())
    if output := os.environ.get("GITHUB_OUTPUT"):
        with Path(output).open("a", encoding="utf-8") as destination:
            destination.write(lines)
    else:
        print(lines, end="")
    print(f"Native Cargo cache: {'compatible inputs identified' if values['usable'] == 'true' else 'disabled; build checks still run'}")


if __name__ == "__main__":
    main()
