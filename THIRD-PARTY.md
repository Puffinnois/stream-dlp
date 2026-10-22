# Third-party components

The stream-dlp application source is MIT-licensed. Rust dependency license details are available in each crate's source and `Cargo.lock` identifies the exact dependency versions.

Download helpers are independent executables installed from their upstream projects by `Setup tools.ps1`. They are not included in the app/setup ZIP; the Windows installer includes them and their available license notices. Installed versions, release URLs, and SHA-256 digests are recorded in `tools/versions.json`; available license notices are copied to `tools/licenses`.

The experimental Mac app bundles its helpers through `scripts/setup-macos.py`.
yt-dlp's universal macOS executable and Deno's Apple Silicon executable come from
their upstream GitHub releases and are checked against release asset digests.
The packaged yt-dlp executable also includes third-party components; consult its
bundled `THIRD_PARTY_LICENSES.txt`. Mac FFmpeg/ffprobe are built from a pinned
official FFmpeg source archive with no external codec libraries, GPL options, or
nonfree options enabled. The app includes the exact source archive and configure
flags in `Contents/Resources/tools/sources`, provenance in `versions.json`, and
FFmpeg's LGPL/GPL license texts in `licenses`. These Mac binaries differ from the
Windows GPL build described below. Source archive: https://ffmpeg.org/releases/.

- **yt-dlp**: https://github.com/yt-dlp/yt-dlp — source is Unlicense; the packaged Windows executable includes third-party components and is distributed under the upstream release's combined licensing (including GPLv3+). See its `THIRD_PARTY_LICENSES.txt`.
- **FFmpeg / ffprobe**: https://github.com/yt-dlp/FFmpeg-Builds and https://ffmpeg.org — the selected win64 GPL build includes GPL components. Source and build scripts: https://github.com/yt-dlp/FFmpeg-Builds and https://github.com/FFmpeg/FFmpeg. Review the installed archive's license notices before redistribution.
- **Deno**: https://github.com/denoland/deno — MIT license, with third-party dependencies.
- **eframe / egui**: https://github.com/emilk/egui — MIT OR Apache-2.0.
- **rfd**: https://github.com/PolyMeilex/rfd — MIT.
- **serde / serde_json**: https://github.com/serde-rs — MIT OR Apache-2.0.
- **url**: https://github.com/servo/rust-url — MIT OR Apache-2.0.
- **tempfile**: https://github.com/Stebalien/tempfile — MIT OR Apache-2.0.
- **dirs**: https://github.com/dirs-dev/dirs-rs — MIT OR Apache-2.0.
- **semver**: https://github.com/dtolnay/semver — MIT OR Apache-2.0.
- **libc** (Unix process cancellation): https://github.com/rust-lang/libc — MIT OR Apache-2.0.
- **winresource** (build only): https://github.com/BenjaminRi/winresource — MIT.
- **Inno Setup** (installer builder): https://jrsoftware.org — its license is available at https://github.com/jrsoftware/issrc/blob/main/license.txt.

The Windows Segoe UI font is loaded from the operating system when present and is not redistributed.

Firefox session import uses the Windows system SQLite component (`winsqlite3.dll`). It is provided by Windows and is not redistributed with the app. No Python runtime or additional browser-import helper is required.
