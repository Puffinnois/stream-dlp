#!/usr/bin/env python3
"""Build self-contained Apple Silicon helpers; run only on a Mac with Xcode CLT.

GitHub binaries require release SHA-256 digests. FFmpeg is built from a pinned
official source archive, without Homebrew libraries or system Python at runtime.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
import zipfile

FFMPEG_VERSION = "9.0.2"
FFMPEG_SHA256 = "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"
FFMPEG_URL = f"https://ffmpeg.org/releases/ffmpeg-{FFMPEG_VERSION}.tar.xz"
FFMPEG_FLAGS = [
    "--disable-autodetect", "--disable-shared", "--enable-static",
    "--disable-doc", "--disable-ffplay", "--disable-debug", "--disable-x86asm",
    # Downloads are performed by yt-dlp; FFmpeg only remuxes local media.
    # Retain two built-in encoders for the synthetic audio/video integration test.
    "--disable-encoders", "--enable-encoder=mpeg4,aac",
]


def fetch(url, destination):
    headers = {"User-Agent": "stream-dlp-macos-build"}
    if url.startswith("https://api.github.com/") and os.environ.get("GITHUB_TOKEN"):
        headers["Authorization"] = f"Bearer {os.environ['GITHUB_TOKEN']}"
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=120) as response:
        with Path(destination).open("wb") as output:
            shutil.copyfileobj(response, output)


def verify(path, expected):
    with Path(path).open("rb") as file:
        actual = hashlib.file_digest(file, "sha256").hexdigest()
    if actual != expected.lower():
        raise RuntimeError(f"SHA-256 verification failed for {Path(path).name}")


def release_asset(repo, name, staging):
    metadata = staging / (repo.replace("/", "-") + ".json")
    fetch(f"https://api.github.com/repos/{repo}/releases/latest", metadata)
    release = json.loads(metadata.read_bytes())
    asset = next(a for a in release["assets"] if a["name"] == name)
    digest = asset.get("digest") or ""
    expected = digest.removeprefix("sha256:")
    if not digest.startswith("sha256:") or len(expected) != 64 or any(c not in "0123456789abcdefABCDEF" for c in expected):
        raise RuntimeError(f"GitHub did not supply a valid SHA-256 digest for {name}")
    url = asset["browser_download_url"]
    if not url.startswith(f"https://github.com/{repo}/releases/download/"):
        raise RuntimeError("Unexpected release asset URL")
    archive = staging / name
    fetch(url, archive)
    verify(archive, expected)
    return archive, {"repository": repo, "version": release["tag_name"], "url": url, "sha256": expected}


def install(destination):
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("The experimental Mac build currently requires an Apple Silicon Mac.")
    subprocess.run(["xcrun", "--find", "clang"], check=True)
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="stream-dlp-macos-") as temporary:
        staging = Path(temporary)
        yt, yt_info = release_asset("yt-dlp/yt-dlp", "yt-dlp_macos", staging)
        deno, deno_info = release_asset("denoland/deno", "deno-aarch64-apple-darwin.zip", staging)
        archive = staging / f"ffmpeg-{FFMPEG_VERSION}.tar.xz"
        fetch(FFMPEG_URL, archive)
        verify(archive, FFMPEG_SHA256)
        with tarfile.open(archive) as source:
            source.extractall(staging, filter="data")
        source = staging / f"ffmpeg-{FFMPEG_VERSION}"
        subprocess.run(["./configure", *FFMPEG_FLAGS], cwd=source, check=True)
        subprocess.run(["make", f"-j{min(os.cpu_count() or 2, 8)}"], cwd=source, check=True)
        shutil.copy2(yt, destination / "yt-dlp")
        with zipfile.ZipFile(deno) as package:
            (destination / "deno").write_bytes(package.read("deno"))
        for name in ("ffmpeg", "ffprobe"):
            shutil.copy2(source / name, destination / name)
        notices = destination / "licenses"
        notices.mkdir(exist_ok=True)
        for license_file in source.glob("COPYING*"):
            shutil.copy2(license_file, notices / ("ffmpeg-" + license_file.name))
        # Ship the exact corresponding FFmpeg source and build flags with the app.
        sources = destination / "sources"
        sources.mkdir(exist_ok=True)
        shutil.copy2(archive, sources / archive.name)
        (sources / "ffmpeg-build.json").write_text(json.dumps({"configure": FFMPEG_FLAGS, "platform": "macOS arm64", "compiler": "Apple clang", "build": "make -j8", "url": FFMPEG_URL, "sha256": FFMPEG_SHA256}, indent=2) + "\n")
        for repo, tag, source_name, target in (
            ("yt-dlp/yt-dlp", yt_info["version"], "LICENSE", "yt-dlp-LICENSE.txt"),
            ("yt-dlp/yt-dlp", yt_info["version"], "THIRD_PARTY_LICENSES.txt", "yt-dlp-THIRD_PARTY_LICENSES.txt"),
            ("denoland/deno", deno_info["version"], "LICENSE.md", "deno-LICENSE.md"),
        ):
            fetch(f"https://raw.githubusercontent.com/{repo}/{tag}/{source_name}", notices / target)
        versions = [yt_info, deno_info, {"repository": "FFmpeg/FFmpeg", "version": FFMPEG_VERSION, "url": FFMPEG_URL, "sha256": FFMPEG_SHA256}]
        (destination / "versions.json").write_text(json.dumps(versions, indent=2) + "\n")
        for name in ("yt-dlp", "deno", "ffmpeg", "ffprobe"):
            binary = destination / name
            binary.chmod(0o755)
            # Local ad-hoc signatures cost nothing; they are not Developer ID
            # signatures and do not remove Gatekeeper's download warning.
            subprocess.run(["codesign", "--force", "--sign", "-", str(binary)], check=True)
            subprocess.run([str(binary), "--version" if name in ("yt-dlp", "deno") else "-version"], check=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path, default=Path(__file__).resolve().parents[1] / "tools")
    install(parser.parse_args().destination.resolve())
