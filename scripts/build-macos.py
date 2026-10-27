#!/usr/bin/env python3
"""Package the experimental Apple Silicon app. Requires Python 3.12+ and Rust."""
import argparse
import hashlib
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def build(skip_tools=False):
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("Build this package on an Apple Silicon Mac.")
    if not skip_tools:
        run(sys.executable, str(ROOT / "scripts/setup-macos.py"))
    for name in ("yt-dlp", "ffmpeg", "ffprobe", "deno", "versions.json", "licenses", "sources"):
        if not (ROOT / "tools" / name).exists():
            raise SystemExit(f"Missing tools/{name}; run setup-macos.py first.")
    run("cargo", "test", "--locked")
    run("cargo", "build", "--release", "--locked")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    # A unique staging directory avoids deleting or modifying an existing app.
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="macos-build-", dir=dist) as staging:
        app = Path(staging) / "stream-dlp.app"
        contents = app / "Contents"
        executable = contents / "MacOS"
        resources = contents / "Resources"
        executable.mkdir(parents=True)
        resources.mkdir()
        shutil.copy2(ROOT / "target/release/stream-dlp", executable / "stream-dlp")
        (executable / "stream-dlp").chmod(0o755)
        shutil.copytree(ROOT / "tools", resources / "tools", ignore=shutil.ignore_patterns("*.exe", "inno-setup", "innosetup*"))
        for name in ("README.md", "LICENSE", "THIRD-PARTY.md"):
            shutil.copy2(ROOT / name, resources / name)
        shutil.copy2(ROOT / "docs/macos.md", resources / "Mac instructions.md")
        # Produce the native icon with Apple's own image tools.
        iconset = Path(staging) / "app.iconset"
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                name = f"icon_{size}x{size}{'@2x' if scale == 2 else ''}.png"
                run("sips", "-z", str(size * scale), str(size * scale), str(ROOT / "assets/app.png"), "--out", str(iconset / name))
        run("iconutil", "-c", "icns", str(iconset), "-o", str(resources / "app.icns"))
        info = {
            "CFBundleExecutable": "stream-dlp", "CFBundleName": "stream-dlp",
            "CFBundleIdentifier": "io.github.puffinnois.stream-dlp",
            "CFBundlePackageType": "APPL", "CFBundleIconFile": "app.icns",
            "CFBundleShortVersionString": version.split("-")[0], "CFBundleVersion": version.split("-")[0],
            "LSMinimumSystemVersion": "15.0", "NSHighResolutionCapable": True,
            "NSHumanReadableCopyright": "stream-dlp contributors. MIT license.",
        }
        (contents / "Info.plist").write_bytes(plistlib.dumps(info))
        for name in ("yt-dlp", "ffmpeg", "ffprobe", "deno"):
            binary = resources / "tools" / name
            run("lipo", str(binary), "-verify_arch", "arm64")
            # Reject accidental Homebrew/developer-machine dependencies.
            libraries = subprocess.check_output(["otool", "-L", str(binary)], text=True)
            for line in libraries.splitlines():
                if not line.startswith("\t"):
                    continue
                dependency = line.strip().split(" (", 1)[0]
                if not dependency.startswith(("/usr/lib/", "/System/Library/")):
                    raise RuntimeError(f"Non-system dependency in {name}: {dependency}")
            run("codesign", "--force", "--sign", "-", str(binary))
        run("codesign", "--force", "--sign", "-", str(app))
        run("codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app))
        archive = dist / "stream-dlp-macOS-arm64-experimental.zip"
        # ditto preserves executable permissions and the app bundle structure.
        run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(archive))
        with archive.open("rb") as file:
            digest = hashlib.file_digest(file, "sha256").hexdigest()
        archive.with_suffix(".zip.sha256").write_text(f"{digest}  {archive.name}\n")
        print(f"Experimental Mac package: {archive}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-tools", action="store_true", help="Reuse previously installed Mac helpers")
    build(parser.parse_args().skip_tools)
