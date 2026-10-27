# Experimental Mac support

The initial target is Apple Silicon (M1 or newer), macOS 15 or newer. Intel Macs
are not supported by this package. Windows continues to use its existing installer.
The Mac build has not yet passed a real-device acceptance check; do not describe
it as a stable Mac release until the checklist below is complete.

## Install and use

Download the `stream-dlp-macOS-arm64-experimental` artifact from a successful
run of the Checks workflow (`.github/workflows/checks.yml`). Extract the artifact download, then extract the app ZIP inside
it. Move `stream-dlp.app` to Applications and open it. The app includes yt-dlp,
FFmpeg, ffprobe, and Deno; users do not need Python, Homebrew, Rust, or Terminal.
GitHub Actions artifacts require signing into GitHub; public Releases assets can
be downloaded without signing in when a maintainer publishes them later.

This app has a local ad-hoc signature, not an Apple Developer ID signature, and
is not notarized. No paid Apple developer account is used. macOS may initially
block a downloaded copy. After trying to open it, use **System Settings → Privacy
& Security → Open Anyway** if you trust the download. Managed Macs may prohibit
this exception. Do not disable Gatekeeper globally. See
[Apple's instructions](https://support.apple.com/en-au/102445).

Paste a YouTube link, find the video, choose its format and destination, and
download. **Open folder** opens Finder. For sign-in challenges, use **I have a
cookies file** or **Export guide**. Automatic Firefox import and browser session
refresh are Windows-only for now. Chrome/Firefox cookie export works through the
existing manual import workflow; Safari cookie integration is not provided.

Mac app and tool updates are manual: close the app, download the latest complete
Mac package, and replace the old app. **About & tools → Get Mac updates** links to
Releases. There is no automatic Mac version check or installation yet. Downloads
saved outside the app remain in their selected destination.

The bundled FFmpeg is built for local stream copying and merging, the GIF
converter, and video previews. It has no external codec libraries; besides the
system zlib, its only encoders are the built-in PNG, GIF, and raw video ones those
features need, plus MPEG-4/AAC for the local test. Other video/audio conversion is
outside this release's scope. Downloaded codecs are not guaranteed to play in
QuickTime. The local test uses the built-in MPEG-4/AAC encoders to avoid requiring
Homebrew or hardware encoding.

## Build on a Mac

Install Xcode Command Line Tools, Python 3.12 or newer, and Rust 1.93.1. Then run:

```sh
python3 scripts/build-macos.py
cargo test --locked local_download_pipeline -- --ignored --nocapture
```

The first command downloads SHA-256-verified official yt-dlp and Deno release
assets and builds FFmpeg from a pinned, SHA-256-verified official source archive.
Missing digests or failed checks stop the build. FFmpeg source, configure options,
license notices, and provenance are included in the app. No Homebrew dylibs are
bundled. Building FFmpeg takes additional time; `--skip-tools` reuses an existing
complete Mac `tools` directory. Never reuse Windows helpers for a Mac build.

The package is written to `dist/stream-dlp-macOS-arm64-experimental.zip` with a
SHA-256 sidecar. Apple tools generate the icon, apply ad-hoc signatures, verify
the bundle, and preserve executable permissions in the ZIP. Ad-hoc signing is
free and does not remove the downloaded-app warning.

The `--check-tools` app argument runs all bundled helpers without starting the UI.
CI extracts the ZIP outside the source tree and uses this argument to verify the
packaged lookup paths. Nothing reads a user's browser profile during CI.

## Automated checks and release policy

The Checks workflow is `.github/workflows/checks.yml`. Pushing workflow files
needs a GitHub login with the `workflow` permission. It runs on pushed branches,
pull requests, and manual dispatch. Windows runs `cargo xtask check`
(formatting, Clippy, Rust and Deno tests). An Apple Silicon macOS 15 runner
builds the Mac helpers and app, runs unit tests and Clippy, downloads and merges a
synthetic local DASH video, checks audio/video with ffprobe, tests cookie
isolation and cancellation, verifies bundle signatures and helper execution, and
uploads the experimental ZIP. Mac runs do not publish releases automatically.

These checks do not establish real YouTube availability, GUI behavior, clean
download Gatekeeper behavior, or real browser-session acceptance. Public YouTube
testing can require user verification and should not gate CI on network luck.

## First real-device acceptance check

- [ ] Download the ZIP through a browser on an Apple Silicon Mac and record macOS version.
- [ ] Extract, move to Applications, follow the security exception, and launch.
- [ ] Check text, window sizing, clipboard paste, and the native destination picker.
- [ ] Download an authorized public YouTube clip with both video and audio.
- [ ] Open the destination in Finder and play the result in a compatible player.
- [ ] Cancel a download and close during a download; confirm helper processes stop.
- [ ] Import a user-supplied YouTube cookie export and test an authorized download.
- [ ] Replace the app with another package; check launch and existing saved files.

Repeat manual checks when changing packaging, permissions, browser integration,
updates, or substantial UI behavior. Routine shared-engine changes use automated
checks; Mac releases may follow Windows when manual validation is still needed.
