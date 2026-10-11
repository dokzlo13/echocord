#!/usr/bin/env python3
"""Shared Windows and Linux/WSL development and release commands (Python 3.12+)."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import shlex
import subprocess
import sys
import tempfile
import tomllib
import venv
import zipfile

ROOT = Path(__file__).resolve().parent.parent
WINDOWS = sys.platform == "win32"
TARGET = "x86_64-pc-windows-msvc"
TOOLS = ROOT / ".tools" / "python"
BIN = TOOLS / ("Scripts" if WINDOWS else "bin")
PYTHON = BIN / ("python.exe" if WINDOWS else "python")
TARGET_DIR = Path(
    os.environ.get(
        "ECHOCORD_APP_TARGET_DIR", os.environ.get("CARGO_TARGET_DIR", ROOT / "target")
    )
).resolve()
TOOL_PINS = ["cmake==4.4.4", "ninja==1.13.2"]
if not WINDOWS:
    TOOL_PINS.append("cargo-xwin==0.23.1")


def run(*args, capture=False, env=None):
    args = [str(arg) for arg in args]
    executable = shutil.which(args[0])
    if not executable:
        raise SystemExit(
            f"Missing tool: {args[0]}. See docs/DEVELOPMENT.md for prerequisites."
        )
    args[0] = executable
    result = subprocess.run(
        args, cwd=ROOT, check=True, text=True, capture_output=capture, env=env
    )
    return result.stdout.strip() if capture else None


def prepare():
    run(sys.executable, ROOT / "scripts/prepare-opus.py")


def version(tag=None):
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    expected = workspace["workspace"]["package"]["version"]
    if not re.fullmatch(
        r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", expected
    ):
        raise SystemExit("Release version must be stable SemVer major.minor.patch")
    ui = json.loads((ROOT / "ui/package.json").read_text(encoding="utf-8"))
    lock = json.loads((ROOT / "ui/package-lock.json").read_text(encoding="utf-8"))
    tauri = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    versions = {
        "ui/package.json": ui["version"],
        "ui/package-lock.json": lock["version"],
        "ui/package-lock.json root": lock["packages"][""]["version"],
        "Tauri": tauri["version"],
    }
    cargo_lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    for name in ("echocord", "echocord-core"):
        versions[f"Cargo.lock {name}"] = next(
            p["version"] for p in cargo_lock["package"] if p["name"] == name
        )
    for path in ("src-tauri/Cargo.toml", "crates/core/Cargo.toml"):
        package = tomllib.loads((ROOT / path).read_text(encoding="utf-8"))["package"]
        if package["version"] != {"workspace": True}:
            raise SystemExit(f"{path} must inherit workspace version")
    for name, value in versions.items():
        if value != expected:
            raise SystemExit(
                f"Version mismatch: {name} has {value}, workspace has {expected}"
            )
    if tag is not None and tag != f"v{expected}":
        raise SystemExit(f"Tag {tag!r} does not match v{expected}")
    print(expected)
    return expected


def windows_environment():
    """Load an installed x64 MSVC toolchain without requiring a special shell."""
    if not WINDOWS:
        return
    if shutil.which("cl") and os.environ.get("VSCMD_ARG_TGT_ARCH") == "x64":
        return
    vswhere = (
        Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)"))
        / "Microsoft Visual Studio/Installer/vswhere.exe"
    )
    if not vswhere.is_file():
        raise SystemExit(
            "Install Visual Studio Build Tools with Desktop development with C++ and a Windows SDK; see docs/DEVELOPMENT.md."
        )
    installation = run(
        vswhere,
        "-latest",
        "-products",
        "*",
        "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-property",
        "installationPath",
        capture=True,
    )
    developer = Path(installation) / "Common7/Tools/VsDevCmd.bat"
    if not installation or not developer.is_file():
        raise SystemExit(
            "No installed MSVC x64 build tools found. See docs/DEVELOPMENT.md."
        )
    command = f'"{os.environ.get("COMSPEC", "cmd.exe")}" /u /d /s /c ""{developer}" -no_logo -arch=x64 -host_arch=x64 >nul && set"'
    result = subprocess.run(
        command, check=True, capture_output=True, encoding="utf-16-le"
    )
    for line in result.stdout.splitlines():
        key, sep, value = line.partition("=")
        if key and sep:
            os.environ[key] = value
    os.environ["PATH"] = str(BIN) + os.pathsep + os.environ["PATH"]


def build_environment():
    windows_environment()
    required = ["cargo", "node", "npm", "cmake", "ninja"]
    if not WINDOWS:
        required += ["cargo-xwin", "clang", "lld-link", "llvm-rc"]
    for tool in required:
        if not shutil.which(tool):
            raise SystemExit(
                f"Missing tool: {tool}. Run setup after installing docs/DEVELOPMENT.md prerequisites."
            )
    if not WINDOWS and not shutil.which("clang-cl"):
        local_bin = ROOT / ".tools/bin"
        local_bin.mkdir(parents=True, exist_ok=True)
        shim = local_bin / "clang-cl"
        if not shim.exists():
            shim.symlink_to(shutil.which("clang"))
        os.environ["PATH"] = str(local_bin) + os.pathsep + os.environ["PATH"]
    os.environ.setdefault("XWIN_CACHE_DIR", str(ROOT / ".cache/xwin"))


def msvc_path_environment(env, roots):
    # MSVC reads _CL_ after its command line, including compiler invocations
    # launched by CMake/MSBuild. This avoids CMake flattening quoted CFLAGS.
    mapped = env.copy()
    flags = [f"/pathmap:{path}={replacement}" for path, replacement in roots]
    mapped["_CL_"] = " ".join(
        filter(None, (env.get("_CL_", ""), subprocess.list2cmdline(flags)))
    )
    return mapped


def verify_msvc_path_mapping(env):
    """Prove the installed compiler remaps __FILE__ before a long release build."""
    directory = ROOT / ".tools"
    directory.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="path-map-", dir=directory) as temporary:
        source = Path(temporary) / "probe.c"
        source.write_text(
            "const char *echocord_probe_source = __FILE__;\n", encoding="utf-8"
        )
        try:
            expanded = run("cl", "/nologo", "/EP", "/FC", source, capture=True, env=env)
        except subprocess.CalledProcessError:
            raise SystemExit(
                "MSVC source-path mapping probe failed; install current Visual Studio C++ Build Tools"
            ) from None
    match = re.search(r'echocord_probe_source\s*=\s*("(?:\\.|[^"\\])*")', expanded)
    try:
        mapped = json.loads(match[1]).replace("\\", "/") if match else ""
    except ValueError:
        mapped = ""
    if not mapped.startswith("/echocord/"):
        raise SystemExit(
            "MSVC did not remap __FILE__; install current Visual Studio C++ Build Tools before making a release"
        )
    print("MSVC source-path mapping: verified")


def windows_flags():
    # Encoded flags keep paths containing spaces intact. The explicit Cargo target
    # keeps Windows linker options out of host build scripts and proc macros.
    env = os.environ.copy()
    existing = env.get("CARGO_ENCODED_RUSTFLAGS")
    flags = (
        existing.split("\x1f") if existing else shlex.split(env.get("RUSTFLAGS", ""))
    )
    flags += ["-Ctarget-feature=-crt-static", "-Clink-arg=/PDBALTPATH:echocord.pdb"]
    roots = [
        (ROOT, "/echocord"),
        (Path(env.get("CARGO_HOME", Path.home() / ".cargo")).resolve(), "/cargo"),
        (Path(run("rustc", "--print", "sysroot", capture=True)), "/rust"),
    ]
    flags += [
        f"--remap-path-prefix={path}={replacement}" for path, replacement in roots
    ]
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    if WINDOWS:
        env = msvc_path_environment(env, roots)
        verify_msvc_path_mapping(env)
    else:
        # Opus assertion filenames come from C, not Rust. cc/cmake parse these
        # quoted flags; clang-cl supports the same file-prefix mapping as clang.
        prefix_flags = " ".join(
            shlex.quote(f"/clang:-ffile-prefix-map={path}={replacement}")
            for path, replacement in roots
        )
        # cargo-xwin 0.23.1 forwards these into target-specific C/C++ flags.
        for key in ("CFLAGS", "CXXFLAGS"):
            env[key] = env.get(key, "") + " " + prefix_flags
    return env


def setup(browser=False):
    run("rustup", "show", "active-toolchain")
    run("node", "--version")
    if not PYTHON.is_file():
        venv.EnvBuilder(with_pip=True).create(TOOLS)
    run(PYTHON, "-m", "pip", "install", "--disable-pip-version-check", *TOOL_PINS)
    prepare()
    run("npm", "--prefix", "ui", "ci", "--no-audit", "--no-fund")
    if browser:
        browser_install()
    build_environment()
    if WINDOWS:
        windows_flags()
    print(
        "Build tools ready. Use check or build; see docs/DEVELOPMENT.md for platform limits."
    )


def browser_install():
    run("npm", "--prefix", "ui", "exec", "--", "playwright", "install", "chromium")


def core_check():
    prepare()
    version()
    if WINDOWS:
        build_environment()
    run(
        sys.executable,
        "-m",
        "unittest",
        "discover",
        "-s",
        "scripts",
        "-p",
        "test_*.py",
    )
    run("cargo", "fmt", "--all", "--check")
    run(
        "cargo",
        "clippy",
        "--locked",
        "-p",
        "echocord-core",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    )
    run("cargo", "test", "--locked", "-p", "echocord-core")
    bindings()


def bindings():
    run(
        "cargo",
        "run",
        "--locked",
        "-p",
        "echocord-core",
        "--bin",
        "export-bindings",
        "--",
        "--check",
        "ui/src/generated.ts",
    )


def frontend():
    run("npm", "--prefix", "ui", "run", "build")
    run("node", "scripts/build-app-icon.mjs")


def build(check=False, prepared=False):
    build_environment()
    if not prepared:
        prepare()
        version()
        bindings()
        frontend()
    cargo = ["cargo"] if WINDOWS else ["cargo", "xwin"]
    args = [
        "clippy" if check else "build",
        "--locked",
        "--release",
        "-j",
        os.environ.get("CARGO_BUILD_JOBS", "2"),
        "--target",
        TARGET,
        "--target-dir",
        TARGET_DIR,
        "-p",
        "echocord",
        "--bin",
        "echocord",
    ]
    if check:
        args += ["--", "-D", "warnings"]
    run(*cargo, *args, env=windows_flags())


def check(portable=False):
    core_check()
    run("npm", "--prefix", "ui", "run", "check")
    if not portable:
        run("node", "scripts/build-app-icon.mjs")
        build(check=True, prepared=True)


def native_tool(name):
    build_environment()
    prepare()
    cargo = ["cargo"] if WINDOWS else ["cargo", "xwin"]
    run(
        *cargo,
        "build",
        "--locked",
        "--release",
        "-j",
        os.environ.get("CARGO_BUILD_JOBS", "2"),
        "--target",
        TARGET,
        "--target-dir",
        TARGET_DIR,
        "-p",
        "echocord-core",
        "--bin",
        name,
        env=windows_flags(),
    )


def app_test():
    if not WINDOWS:
        raise SystemExit("Application unit tests must run on native Windows")
    build_environment()
    prepare()
    frontend()
    run(
        "cargo",
        "test",
        "--locked",
        "--release",
        "--target",
        TARGET,
        "--target-dir",
        TARGET_DIR,
        "-p",
        "echocord",
        "--bin",
        "echocord",
        env=windows_flags(),
    )


def release_notes(changelog, release_version):
    """Select the tagged version's user-facing changes, never Unreleased text."""
    headings = list(re.finditer(r"(?m)^##[ \t]+([^\n]+)$", changelog))
    matches = [
        index
        for index, heading in enumerate(headings)
        if heading[1].split()[0].strip("[]") in (release_version, f"v{release_version}")
    ]
    if len(matches) != 1:
        raise SystemExit(
            f"CHANGELOG.md must contain exactly one version section for {release_version}"
        )
    index = matches[0]
    end = headings[index + 1].start() if index + 1 < len(headings) else len(changelog)
    changes = changelog[headings[index].end() : end].strip()
    if not changes:
        raise SystemExit(
            f"CHANGELOG.md has an empty version section for {release_version}"
        )
    filename = f"echocord-{release_version}-windows-x64.zip"
    sources = f"echocord-{release_version}-third-party-sources.zip"
    return f"""# Echocord {release_version}

{changes}

## Download and setup

Download **{filename}**, extract the complete ZIP, and run `echocord.exe` on Windows 11 x64. This early hobby release is unsigned and provided without guarantees.

Install [Microsoft Edge WebView2 Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) and the [Visual C++ v14 x64 runtime](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist) if needed. See the [bot setup and usage guide](https://github.com/dokzlo13/echocord#readme) before connecting.

Use `SHA256SUMS` to verify the release downloads. The application ZIP includes its MIT license and complete third-party notices. **{sources}** contains the corresponding covered third-party sources; retain those notices when redistributing.
"""


def validated_notices(release_version, source_archive):
    """Reject incomplete or stale licensing output before assembling a release."""
    generated = ROOT / "licenses/generated"
    index = generated / "INDEX.md"
    inventory_path = generated / "inventory.json"
    if generated.is_symlink() or not generated.resolve().is_relative_to(ROOT.resolve()):
        raise SystemExit("Generated notices must be inside this checkout")
    for path in (index, inventory_path):
        if not path.is_file() or path.is_symlink():
            raise SystemExit(
                "Missing generated license index or inventory; run scripts/package-third-party.py"
            )
    try:
        inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
        pinned_rust = tomllib.loads(
            (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
        )["toolchain"]["channel"]
        if (
            inventory["schema"] != 1
            or inventory["echocord_version"] != release_version
            or inventory["target"] != TARGET
            or inventory["rust_version"] != pinned_rust
        ):
            raise ValueError("version, toolchain or target differs")
        for key, name in (
            ("cargo_lock_sha256", "Cargo.lock"),
            ("npm_lock_sha256", "ui/package-lock.json"),
        ):
            if inventory[key] != hashlib.sha256((ROOT / name).read_bytes()).hexdigest():
                raise ValueError("lockfile changed since notice generation")
        source = inventory["source_archive"]
        if (
            source["name"] != source_archive.name
            or not source_archive.is_file()
            or source_archive.is_symlink()
            or source["sha256"]
            != hashlib.sha256(source_archive.read_bytes()).hexdigest()
        ):
            raise ValueError("corresponding-source archive is missing or changed")
        files = {index, inventory_path}
        if not inventory["packages"]:
            raise ValueError("dependency inventory is empty")
        for package in inventory["packages"]:
            if not package["notices"]:
                raise ValueError("dependency has no indexed notices")
            entries = package["notices"] + (
                [package["attribution"]] if "attribution" in package else []
            )
            for entry in entries:
                relative = PurePosixPath(entry["path"])
                if (
                    relative.is_absolute()
                    or ".." in relative.parts
                    or "\\" in entry["path"]
                    or ":" in entry["path"]
                ):
                    raise ValueError("unsafe notice path")
                path = generated / relative
                if (
                    not path.is_file()
                    or path.is_symlink()
                    or not path.resolve().is_relative_to(generated.resolve())
                    or hashlib.sha256(path.read_bytes()).hexdigest() != entry["sha256"]
                ):
                    raise ValueError("indexed notice is missing or changed")
                files.add(path)
        return sorted(files)
    except (KeyError, TypeError, ValueError) as error:
        raise SystemExit(
            f"Release notices are invalid: {error}; run scripts/package-third-party.py"
        ) from None


def package(tag=None):
    release_version = version(tag)
    executable = TARGET_DIR / TARGET / "release/echocord.exe"
    required = [
        executable,
        ROOT / "LICENSE",
        ROOT / "THIRD_PARTY_NOTICES.md",
        ROOT / "README.md",
        ROOT / "CHANGELOG.md",
    ]
    for path in required:
        if not path.is_file():
            raise SystemExit(
                f"Missing release input: {path.name}; build and prepare notices before packaging"
            )
    notes = release_notes(
        (ROOT / "CHANGELOG.md").read_text(encoding="utf-8"), release_version
    )
    # Refuse accidental packaging of an old, unsanitized local executable.
    binary = executable.read_bytes()
    private_roots = (
        ("workspace", ROOT),
        ("user profile", Path.home()),
        ("Cargo home", Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))),
    )
    for origin, root in private_roots:
        for text in (str(root), str(root).replace("\\", "/")):
            for encoding in ("utf-8", "utf-16-le"):
                occurrences = binary.count(text.encode(encoding))
                if occurrences:
                    raise SystemExit(
                        f"A local build path remains in the executable ({origin}, {encoding}, {occurrences} occurrences); clean native dependency output and rebuild with scripts/dev.py build before packaging"
                    )
    output = ROOT / "artifacts/release"
    source_archive = output / f"echocord-{release_version}-third-party-sources.zip"
    notice_files = validated_notices(release_version, source_archive)
    name = f"echocord-{release_version}-windows-x64"
    archive = output / f"{name}.zip"
    notes_path = output / "RELEASE-NOTES.md"
    notes_path.write_text(notes, encoding="utf-8", newline="\n")
    usage = """Echocord — stream local audio to Discord as a bot

Extract the complete ZIP, then run echocord.exe. Windows 11 x64 is supported.
Install Microsoft Edge WebView2 Evergreen Runtime and the Visual C++ v14 x64
runtime if they are not installed:
https://developer.microsoft.com/microsoft-edge/webview2/
https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist

This early hobby release is unsigned. No guarantees are given.
Open the complete setup and usage guide online:
https://github.com/dokzlo13/echocord#readme
The bundled README is a source copy; its relative links/images use the online
repository. Use the online guide for bot setup and permissions.

App settings and the protected bot token are stored in your Windows user profile,
not alongside the executable. Closing the window keeps it running in the tray
by default; use the tray menu's Quit to exit.

See LICENSE, THIRD_PARTY_NOTICES.md and licenses/ for redistribution notices.
"""
    with zipfile.ZipFile(
        archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as bundle:
        for path in required:
            bundle.write(path, f"{name}/{path.name}")
        for path in notice_files:
            bundle.write(path, f"{name}/{path.relative_to(ROOT).as_posix()}")
        bundle.writestr(f"{name}/START-HERE.txt", usage)
        bundle.write(notes_path, f"{name}/RELEASE-NOTES.md")
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    checksum_path = archive.with_suffix(".zip.sha256")
    checksum_path.write_text(
        f"{checksum}  {archive.name}\n", encoding="utf-8", newline="\n"
    )
    assets = sorted((archive, checksum_path, source_archive, notes_path))
    (output / "SHA256SUMS").write_text(
        "".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
            for path in assets
        ),
        encoding="utf-8",
        newline="\n",
    )
    print(f"Packaged {archive.name} and SHA-256 checksums in artifacts/release/")


def main():
    if sys.version_info < (3, 12):
        raise SystemExit("Python 3.12 or later is required")
    os.environ["PATH"] = str(BIN) + os.pathsep + os.environ["PATH"]
    os.environ.setdefault("CARGO_TARGET_DIR", str(TARGET_DIR))
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser(
        "setup", help="Install repo-local tools and locked UI dependencies"
    ).add_argument("--browser", action="store_true")
    sub.add_parser("browser-install", help="Download Chromium for UI checks")
    sub.add_parser("core-check", help="Check native core on this host")
    sub.add_parser(
        "check", help="Core, UI and Windows application checks"
    ).add_argument("--portable", action="store_true")
    sub.add_parser("build", help="Build Windows x64 release executable").add_argument(
        "--check", action="store_true"
    )
    sub.add_parser("app-test", help="Run application unit tests on Windows")
    sub.add_parser(
        "native-tool", help="Build a manual native diagnostic tool"
    ).add_argument(
        "name",
        choices=["process-probe", "capture-probe", "engine-probe", "storage-probe"],
    )
    sub.add_parser(
        "version", help="Verify all release version declarations"
    ).add_argument("--tag")
    sub.add_parser(
        "package", help="Package an existing Windows release and notices"
    ).add_argument("--tag")
    args = parser.parse_args()
    if args.command == "setup":
        setup(args.browser)
    elif args.command == "browser-install":
        browser_install()
    elif args.command == "core-check":
        core_check()
    elif args.command == "check":
        check(args.portable)
    elif args.command == "build":
        build(args.check)
    elif args.command == "app-test":
        app_test()
    elif args.command == "native-tool":
        native_tool(args.name)
    elif args.command == "version":
        version(args.tag)
    elif args.command == "package":
        package(args.tag)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode) from None
