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


def require_mac():
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("Build this package on an Apple Silicon Mac.")


def sha256(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def write_checksum(path):
    path.with_suffix(path.suffix + ".sha256").write_text(f"{sha256(path)}  {path.name}\n")


def build_dmg(app):
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=dist) as staging, tempfile.TemporaryDirectory() as test:
        work = Path(staging)
        volume = work / "volume"
        volume.mkdir()
        run("ditto", app, volume / app.name)
        (volume / "Applications").symlink_to("/Applications", target_is_directory=True)
        image = work / "stream-dlp-macOS-arm64-experimental.dmg"
        run("hdiutil", "create", "-srcfolder", volume, "-volname", "stream-dlp",
            "-fs", "HFS+", "-format", "UDZO", image)
        run("hdiutil", "verify", image)
        mount = Path(test) / "mounted"
        mount.mkdir()
        run("hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint", mount, image)
        try:
            if (mount / "Applications").readlink() != Path("/Applications"):
                raise RuntimeError("The DMG's Applications shortcut is invalid.")
            installed = Path(test) / app.name
            run("ditto", mount / app.name, installed)
        finally:
            run("hdiutil", "detach", mount)
        run("codesign", "--verify", "--deep", "--strict", installed)
        subprocess.run([installed / "Contents/MacOS/stream-dlp", "--check-tools"], cwd=test, check=True)
        write_checksum(image)
        for output in (image, image.with_suffix(".dmg.sha256")):
            output.replace(dist / output.name)
        print(f"Verified Mac disk image: {dist / image.name}")


def build(skip_tools=False):
    require_mac()
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
        shutil.copy2(ROOT / "docs/guide.md", resources / "User guide.md")
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
        write_checksum(archive)
        print(f"Experimental Mac package: {archive}")
        build_dmg(app)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--skip-tools", action="store_true", help="Reuse previously installed Mac helpers")
    mode.add_argument("--dmg-from-zip", type=Path, metavar="ZIP", help="Package an existing release ZIP")
    parser.add_argument("--sha256", metavar="DIGEST", help="Expected release ZIP digest for --dmg-from-zip")
    args = parser.parse_args()
    if bool(args.dmg_from_zip) != bool(args.sha256):
        parser.error("--dmg-from-zip and --sha256 must be supplied together")
    if args.dmg_from_zip:
        archive = args.dmg_from_zip.resolve()
        if sha256(archive) != args.sha256.lower():
            raise SystemExit("Release ZIP SHA-256 mismatch.")
        require_mac()
        with tempfile.TemporaryDirectory() as extracted:
            run("ditto", "-x", "-k", archive, extracted)
            build_dmg(Path(extracted) / "stream-dlp.app")
    else:
        build(args.skip_tools)
