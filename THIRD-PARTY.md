# Third-party components

The stream-dlp application source is MIT-licensed. Rust dependency license details are available in each crate's source and `Cargo.lock` identifies the exact dependency versions.

Download helpers are independent executables installed from their upstream projects by `Setup tools.ps1`. They are not included in the app/setup ZIP. Installed versions, release URLs, and SHA-256 digests are recorded in `tools/versions.json`; available license notices are copied to `tools/licenses`.

- **yt-dlp**: https://github.com/yt-dlp/yt-dlp — source is Unlicense; the packaged Windows executable includes third-party components and is distributed under the upstream release's combined licensing (including GPLv3+). See its `THIRD_PARTY_LICENSES.txt`.
- **FFmpeg / ffprobe**: https://github.com/yt-dlp/FFmpeg-Builds and https://ffmpeg.org — the selected win64 GPL build includes GPL components. Source and build scripts: https://github.com/yt-dlp/FFmpeg-Builds and https://github.com/FFmpeg/FFmpeg. Review the installed archive's license notices before redistribution.
- **Deno**: https://github.com/denoland/deno — MIT license, with third-party dependencies.
- **eframe / egui**: https://github.com/emilk/egui — MIT OR Apache-2.0.
- **rfd**: https://github.com/PolyMeilex/rfd — MIT.
- **serde / serde_json**: https://github.com/serde-rs — MIT OR Apache-2.0.
- **url**: https://github.com/servo/rust-url — MIT OR Apache-2.0.
- **tempfile**: https://github.com/Stebalien/tempfile — MIT OR Apache-2.0.
- **dirs**: https://github.com/dirs-dev/dirs-rs — MIT OR Apache-2.0.

The Windows Segoe UI font is loaded from the operating system when present and is not redistributed.
