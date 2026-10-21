"""R-01 offline probe. Creates only synthetic profiles; never opens real profiles.

Usage: python scripts/browser_session_spike.py --yt-dlp tools/yt-dlp.exe
Repeat --yt-dlp to compare versions. No third-party Python dependencies.
"""

import argparse
from contextlib import closing
import ctypes
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile


def firefox_profile(path, *, empty=False, expired=False, schema=17):
    path.mkdir(parents=True)
    with closing(sqlite3.connect(path / "cookies.sqlite")) as db:
        db.execute(f"PRAGMA user_version={schema}")
        db.execute("CREATE TABLE moz_cookies (host, name, value, path, expiry, isSecure, originAttributes)")
        if not empty:
            expiry = 1 if expired else 4102444800
            if schema >= 16:
                expiry *= 1000
            db.executemany("INSERT INTO moz_cookies VALUES (?, ?, ?, ?, ?, ?, ?)", [
                (".youtube.com", "SYNTHETIC", "fake-youtube", "/", expiry, 1, ""),
                (".example.com", "SYNTHETIC", "fake-unrelated", "/", expiry, 1, ""),
                (".youtube.com.evil.test", "SYNTHETIC", "fake-lookalike", "/", expiry, 1, ""),
            ])
        db.commit()


def probe(executable, root, browser, profile, case):
    output = root / f"{browser}-{case}.txt"
    command = [str(executable), "--ignore-config", "--no-progress",
               "--cookies-from-browser", f"{browser}:{profile}", "--cookies", str(output)]
    # Deliberately no URL: extraction/export must finish before any media request.
    try:
        process = subprocess.run(command, capture_output=True, timeout=30)
    except subprocess.TimeoutExpired:
        return {"case": case, "browser": browser, "outcome": "timeout"}
    jar = http.cookiejar.MozillaCookieJar()
    if output.exists():
        jar.load(str(output), ignore_discard=True, ignore_expires=True)
    domains = {cookie.domain for cookie in jar}
    # Report only synthetic-domain presence and process status, never raw logs/values.
    return {"case": case, "browser": browser, "exit_code": process.returncode,
            "export_created": output.exists(), "cookies": len(jar),
            "youtube": ".youtube.com" in domains,
            "unrelated": ".example.com" in domains,
            "lookalike": ".youtube.com.evil.test" in domains,
            "expired": sum(cookie.is_expired() for cookie in jar)}


def locked_probe(executable, root, profile):
    """Deny sharing to reproduce Windows' locked-database copy failure."""
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_ulong, ctypes.c_ulong,
                                  ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_void_p]
    kernel.CreateFileW.restype = ctypes.c_void_p
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    handle = kernel.CreateFileW(str(profile / "cookies.sqlite"), 0x80000000, 0,
                                None, 3, 0, None)
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        return probe(executable, root, "firefox", profile, "locked-database")
    finally:
        kernel.CloseHandle(handle)


def run(executable):
    version = subprocess.run([str(executable), "--ignore-config", "--version"],
                             capture_output=True, text=True, check=True, timeout=30).stdout.strip()
    results = []
    with tempfile.TemporaryDirectory(prefix="stream-dlp-synthetic-spike-") as temporary:
        root = Path(temporary).resolve()
        for case, options in [("default", {}), ("secondary-unicode", {}),
                              ("signed-out", {"empty": True}),
                              ("expired", {"expired": True}),
                              ("legacy-schema", {"schema": 15})]:
            profile = root / ("secondary profile \u6d4b\u8bd5" if case == "secondary-unicode" else case)
            firefox_profile(profile, **options)
            before = hashlib.sha256((profile / "cookies.sqlite").read_bytes()).digest()
            result = probe(executable, root, "firefox", profile, case)
            assert before == hashlib.sha256((profile / "cookies.sqlite").read_bytes()).digest()
            results.append(result)
        for browser in ("firefox", "chrome", "edge"):
            results.append(probe(executable, root, browser, root / "missing-profile", "missing"))
        if os.name == "nt":
            results.append(locked_probe(executable, root, root / "default"))
        for result in results:
            case = result["case"]
            if "exit_code" not in result:
                raise RuntimeError(f"{result['browser']}/{case}: probe timed out")
            if case in ("missing", "locked-database"):
                expected = result["exit_code"] == 1 and not result["export_created"]
            else:
                expected = result["exit_code"] == 2 and result["export_created"]
                expected &= result["cookies"] == (0 if case == "signed-out" else 3)
                if case != "signed-out":
                    expected &= result["youtube"] and result["unrelated"] and result["lookalike"]
                expected &= result["expired"] == (3 if case == "expired" else 0)
            if not expected:
                raise RuntimeError(f"{result['browser']}/{case}: behavior changed; review probe results")
    return {"version": version, "sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
            "os": os.name, "synthetic_only": True, "results": results,
            "temporary_directory_removed": not root.exists()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--yt-dlp", type=Path, action="append", required=True)
    args = parser.parse_args()
    print(json.dumps([run(path.resolve(strict=True)) for path in args.yt_dlp], indent=2))


if __name__ == "__main__":
    main()
