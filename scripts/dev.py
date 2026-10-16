#!/usr/bin/env python3
"""Shared Windows and Linux/WSL development and release commands (Python 3.12+)."""

import argparse
import hashlib
from html.parser import HTMLParser
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
    run(sys.executable, ROOT / "scripts/prepare-cpal.py")


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


def msvc_path_environment(env, roots, *, trim=False):
    # MSVC reads _CL_ after its command line, including compiler invocations
    # launched by CMake/MSBuild. This avoids CMake flattening quoted CFLAGS.
    mapped = env.copy()
    flags = (
        [f"/d1trimfile:{roots[0][0]}"]
        if trim
        else [
            f"/pathmap:{path}={replacement.lstrip('/')}" for path, replacement in roots
        ]
    )
    mapped["_CL_"] = " ".join(
        filter(None, (env.get("_CL_", ""), subprocess.list2cmdline(flags)))
    )
    return mapped


def verify_msvc_path_mapping(env, *, trim=False):
    """Prove emitted __FILE__ strings using an owned, side-effect-free C program."""
    directory = ROOT / ".tools"
    directory.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="path-map-", dir=directory) as temporary:
        source = Path(temporary) / "probe.c"
        executable = Path(temporary) / "probe.exe"
        source.write_text(
            "#include <stdio.h>\nint main(void) { puts(__FILE__); return 0; }\n",
            encoding="utf-8",
        )
        result = subprocess.run(
            [
                shutil.which("cl") or "cl",
                "/nologo",
                "/O1",
                "/MD",
                "/FC",
                str(source),
                f"/Fo{Path(temporary) / 'probe.obj'}",
                f"/Fe{executable}",
            ],
            cwd=temporary,
            text=True,
            capture_output=True,
            env=env,
        )
        relative = source.relative_to(ROOT).as_posix()
        codes = sorted(
            set(re.findall(r"\b(?:[CD]\d{4}|LNK\d{4})\b", result.stdout + result.stderr))
        )
        diagnostics = (
            f"compiler exit {result.returncode}; codes {','.join(codes) or 'none'}"
        )
        mapped = ""
        if result.returncode == 0:
            emitted = subprocess.run(
                [str(executable)],
                cwd=temporary,
                text=True,
                capture_output=True,
                env=env,
            )
            if emitted.returncode:
                raise SystemExit(
                    f"MSVC source-path probe program failed (exit {emitted.returncode})"
                )
            mapped = emitted.stdout.strip().replace("\\", "/")
    expected = relative if trim else f"echocord/{relative}"
    if result.returncode or mapped.lstrip("/") != expected:
        category = (
            "missing marker"
            if not mapped
            else "absolute"
            if re.match(r"^(?:[A-Za-z]:|/)", mapped)
            else "unexpected relative"
        )
        raise SystemExit(
            f"MSVC source-path probe failed ({diagnostics}; marker {category})"
        )
    print(f"MSVC source-path mapping: verified ({'trimfile' if trim else 'pathmap'})")


def verified_msvc_environment(env, roots):
    mapped = msvc_path_environment(env, roots)
    try:
        verify_msvc_path_mapping(mapped)
        return mapped
    except SystemExit as failure:
        # Windows App SDK uses this older MSVC option for reproducible builds.
        # Only select it after the same compiler proves the exact relative path.
        print(f"{failure}; testing trimfile compatibility")
    trimmed = msvc_path_environment(env, roots, trim=True)
    verify_msvc_path_mapping(trimmed, trim=True)
    return trimmed


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
        env = verified_msvc_environment(env, roots)
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
    if not check:
        run(sys.executable, ROOT / "scripts/package-third-party.py")
        stage_license_assets()
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
    if not check:
        write_build_receipt()


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
    sources = f"echocord-{release_version}-third-party-sources.zip"
    return f"""# Echocord {release_version}

{changes}

## Download and setup

Download and run **[echocord.exe](https://github.com/dokzlo13/echocord/releases/download/v{release_version}/echocord.exe)** on Windows 11 x64. This early hobby release is unsigned.

Install [Microsoft Edge WebView2 Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) and the [Visual C++ v14 x64 runtime](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist) if needed. See the [bot setup and usage guide](https://github.com/dokzlo13/echocord#readme) before connecting.

Licenses and complete third-party notices are available offline in **Settings → Licenses**. `SHA256SUMS` verifies the downloads; **{sources}** supplies corresponding covered sources and is not needed to run the app.
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
            if patch := package.get("patch"):
                relative = PurePosixPath(patch["path"])
                if (
                    relative.is_absolute()
                    or ".." in relative.parts
                    or not relative.parts
                    or relative.parts[0] != "patches"
                    or "\\" in patch["path"]
                    or ":" in patch["path"]
                ):
                    raise ValueError("unsafe dependency patch path")
                path = ROOT / relative
                if (
                    not path.is_file()
                    or path.is_symlink()
                    or not path.resolve().is_relative_to(ROOT.resolve())
                    or hashlib.sha256(path.read_bytes()).hexdigest() != patch["sha256"]
                ):
                    raise ValueError("dependency patch changed since notice generation")
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


class NoticeText(HTMLParser):
    """Readable HTML notice text; the original HTML remains bundled verbatim."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []
        self.hidden = 0

    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style"):
            self.hidden += 1
        if tag in ("p", "div", "br", "li", "h1", "h2", "h3", "pre", "tr"):
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag in ("script", "style"):
            self.hidden = max(0, self.hidden - 1)
        if tag in ("p", "div", "li", "h1", "h2", "h3", "pre", "tr"):
            self.parts.append("\n")

    def handle_data(self, data):
        if not self.hidden:
            self.parts.append(data)


def embedded_notice_path(relative):
    # Tauri parses HTML assets to inject CSP. Keep original HTML as plain text
    # so its notice bytes and recorded upstream checksum remain unchanged.
    suffix = ".txt" if PurePosixPath(relative).suffix.lower() in (".html", ".htm") else ""
    return f"generated/{relative}{suffix}"


def license_assets(release_version, notice_files):
    """Build a deterministic, self-contained offline license catalog."""
    assets = {
        "LICENSE.txt": (ROOT / "LICENSE").read_bytes(),
        "THIRD_PARTY_NOTICES.txt": (ROOT / "THIRD_PARTY_NOTICES.md").read_bytes(),
    }
    documents = [
        {"title": "Echocord · MIT license", "path": "/licenses/LICENSE.txt"},
        {
            "title": "Echocord · Third-party notices and corresponding source",
            "path": "/licenses/THIRD_PARTY_NOTICES.txt",
        },
    ]
    generated = ROOT / "licenses/generated"
    for path in notice_files:
        relative = path.relative_to(generated).as_posix()
        assets[embedded_notice_path(relative)] = path.read_bytes()
    inventory = json.loads(assets["generated/inventory.json"])
    for package in inventory["packages"]:
        entries = package["notices"] + (
            [package["attribution"]] if "attribution" in package else []
        )
        for entry in entries:
            path = embedded_notice_path(entry["path"])
            original_name = entry.get("original_name", "Published attribution")
            title = f"{package['name']} {package['version']} · {original_name}"
            if PurePosixPath(entry["path"]).suffix.lower() in (".html", ".htm"):
                parser = NoticeText()
                parser.feed(assets[path].decode("utf-8"))
                readable = f"readable/{entry['sha256']}.txt"
                assets[readable] = ("".join(parser.parts).strip() + "\n").encode()
                # Offer both: readable text for the app and the untouched source.
                documents.append({
                    "title": title + " (original HTML)", "path": "/licenses/" + path,
                })
                path = readable
            documents.append({"title": title, "path": "/licenses/" + path})
    documents.extend([
        {"title": "Complete dependency inventory", "path": "/licenses/generated/INDEX.md"},
        {"title": "Machine-readable provenance", "path": "/licenses/generated/inventory.json"},
    ])
    filename = inventory["source_archive"]["name"]
    catalog = {
        "version": release_version,
        "sourceUrl": f"https://github.com/dokzlo13/echocord/releases/download/v{release_version}/{filename}",
        "documents": documents,
    }
    assets["catalog.json"] = (
        json.dumps(catalog, ensure_ascii=False, indent=2) + "\n"
    ).encode()
    return assets


def asset_digest(assets):
    """Length-prefix paths/content so file boundaries cannot collide."""
    digest = hashlib.sha256()
    for name, data in sorted(assets.items()):
        encoded = name.encode("utf-8")
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)
    return digest.hexdigest()


def current_license_assets(release_version):
    source_archive = (
        ROOT / "artifacts/release" / f"echocord-{release_version}-third-party-sources.zip"
    )
    return license_assets(
        release_version, validated_notices(release_version, source_archive)
    )


def stage_license_assets():
    release_version = version()
    assets = current_license_assets(release_version)
    output = ROOT / "ui/dist/licenses"
    if output.is_symlink():
        raise SystemExit("Refusing to replace symlinked frontend licenses")
    if output.exists():
        shutil.rmtree(output)
    for name, data in assets.items():
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    print(f"Embedded license assets prepared ({len(assets)} files)")


def build_receipt_path():
    return TARGET_DIR / TARGET / "release/echocord-build.json"


def receipt_data(release_version, binary, assets):
    return {
        "schema": 1,
        "version": release_version,
        "target": TARGET,
        "executable_sha256": hashlib.sha256(binary).hexdigest(),
        "licenses_sha256": asset_digest(assets),
        "cargo_lock_sha256": hashlib.sha256(
            (ROOT / "Cargo.lock").read_bytes()
        ).hexdigest(),
        "npm_lock_sha256": hashlib.sha256(
            (ROOT / "ui/package-lock.json").read_bytes()
        ).hexdigest(),
    }


def write_build_receipt():
    release_version = version()
    assets = current_license_assets(release_version)
    # Check that compilation was given the exact complete assets, never the
    # development preview's license placeholder or an unindexed local file.
    staged = ROOT / "ui/dist/licenses"
    actual = {
        path.relative_to(staged).as_posix(): path.read_bytes()
        for path in staged.rglob("*") if path.is_file()
    }
    if actual != assets:
        raise SystemExit("Frontend license assets changed during the build")
    binary = (TARGET_DIR / TARGET / "release/echocord.exe").read_bytes()
    receipt = receipt_data(release_version, binary, assets)
    build_receipt_path().write_text(
        json.dumps(receipt, indent=2) + "\n", encoding="utf-8", newline="\n"
    )


def validated_build_receipt(release_version, binary, assets):
    try:
        receipt = json.loads(build_receipt_path().read_text(encoding="utf-8"))
    except (OSError, ValueError):
        raise SystemExit(
            "Missing or invalid build receipt; run scripts/dev.py build"
        ) from None
    if receipt != receipt_data(release_version, binary, assets):
        raise SystemExit(
            "Executable or embedded licenses are stale; run scripts/dev.py build"
        )


def package(tag=None):
    release_version = version(tag)
    executable = TARGET_DIR / TARGET / "release/echocord.exe"
    required = [
        executable, ROOT / "LICENSE", ROOT / "THIRD_PARTY_NOTICES.md",
        ROOT / "CHANGELOG.md",
    ]
    for path in required:
        if not path.is_file():
            raise SystemExit(f"Missing release input: {path.name}; build before packaging")
    notes = release_notes(
        (ROOT / "CHANGELOG.md").read_text(encoding="utf-8"), release_version
    )
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
    assets = current_license_assets(release_version)
    validated_build_receipt(release_version, binary, assets)
    output = ROOT / "artifacts/release"
    source_archive = output / f"echocord-{release_version}-third-party-sources.zip"
    published_executable = output / "echocord.exe"
    shutil.copyfile(executable, published_executable)
    (output / "RELEASE-NOTES.md").write_text(notes, encoding="utf-8", newline="\n")
    (output / "SHA256SUMS").write_text(
        "".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
            for path in sorted((published_executable, source_archive))
        ),
        encoding="utf-8", newline="\n",
    )
    print("Packaged echocord.exe, corresponding sources and SHA256SUMS in artifacts/release/")


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
