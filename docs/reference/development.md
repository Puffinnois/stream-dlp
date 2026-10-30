# Develop, build and release — reference

## Set up

Requires Windows 10/11 x64, the Visual Studio C++ build tools / Windows SDK, and rustup: `rust-toolchain.toml` pins Rust 1.93.1 (what CI uses) with rustfmt and Clippy, and rustup installs it on first use. Keep the checkout outside OneDrive (the working copy is `C:\dev\stream-dlp`): `target/` and `dist/` grow to several GB. Download tools for development go in the gitignored `tools/` (a debug build falls back to it): use **Settings → Download tools** in the app, or `Setup tools.ps1`. Inno Setup for local installer builds goes in `tools/inno-setup`.

```powershell
cargo xtask check
cargo run
```

`cargo run` starts a debug build in Dev mode (see [dev-notes.md](dev-notes.md)). The UI and orchestration are Rust (`eframe`/`egui`); extraction is delegated to yt-dlp, media merging to FFmpeg, and JavaScript challenges to Deno. No web server or Electron runtime is used.

## Build

```powershell
.\build.ps1
```

`cargo xtask check` runs formatting, Clippy, the Rust tests, and the Deno script tests (`scripts/*.test.js`, using `tools/deno` or `deno` on PATH), exactly as CI does. `build.ps1` runs the Rust tests, then creates a ready-to-run portable folder, an app/setup ZIP, and `SHA256SUMS.txt` in `dist`. `-SkipTools` builds the app without downloading helpers; `-SkipTests` skips the tests. The portable folder and ZIP ship `README.md`, `LICENSE`, `THIRD-PARTY.md` and the Markdown files directly in `docs/` (the user guide and the Mac notes), not the subfolders.

To also build the Windows installers, run `./build.ps1 -Installer -IsccPath 'tools\inno-setup\ISCC.exe'`. This builds the full installer and, from the same `installer/stream-dlp.iss` with `/DUpdateOnly`, the update installer without the tools, which refuses to install where stream-dlp is not installed yet. Installer builds require all helpers in the portable folder; `-SkipTools -Installer` reuses helpers already present there. The installer version comes from `Cargo.toml`. `scripts/make-icon.ps1` regenerates the multi-size app icon and PNG preview. `build.rs` embeds that icon and the app version into the Windows executable. Do not run the installers on a machine where stream-dlp is really installed; the Package check tests them.

If the existing portable executable is open, use `-PortableDirectory 'dist\stream-dlp-0.3.4'` to build a separate folder. Without `-SkipTools`, setup populates that folder's helpers as usual. The generated installer and ZIP use the selected portable folder.

## Develop and release

- Checks: `cargo xtask check`. The **Checks** workflow runs it (and the Mac build) on every pushed branch and pull request.
- Try a change on Windows without releasing: push a branch named `check/<anything>`. The **Package check** workflow builds both installers, checks that the packaged and the silently installed app can run their bundled tools (`--check-tools`), that the update installer refuses a first installation and then updates that installation in place, and attaches the installers, ZIP, and checksums to the run.
- Release: on a clean `main`, `cargo xtask release <patch|minor|major>` (or an explicit newer `X.Y.Z`) bumps `Cargo.toml` and `Cargo.lock`, commits `build: bump version to X.Y.Z`, and tags `vX.Y.Z`. Then `git push --follow-tags`. The **Release** workflow checks the tag matches `Cargo.toml`, runs the checks, builds and checks both installers, and uploads them, the ZIP, and `SHA256SUMS.txt` to a **draft** GitHub release with generated notes. Edit the notes (the app shows them in its update dialog) and press **Publish**; installed apps only see published releases. There are no prereleases: try changes through Package check instead.
- Changelog: the GitHub release notes. Notes of 0.3.4–0.3.8 are also in [docs/releases/](../releases/).
- Commits: Conventional Commits; feature branches merged with `merge: <branch> (<what>)`.

## Verification

See the [v0.3.4 release notes](../releases/release-0.3.4.md) for release validation and
known acceptance limits.

The [R-02 Streamable decision](../research/streamable-feasibility.md) selects the
browser-assisted sharing workflow. Authorized guest website uploads of synthetic
and existing downloaded clips processed and played successfully. Direct uploads
remain blocked by the official read-only API. Signed-in website acceptance
remains open.

`cargo test --locked local_library_probe -- --ignored --nocapture` checks a real
generated video and invalid files through FFprobe, including Unicode filenames
and cancellation. Set `STREAM_DLP_VIDEO_TEST_FOLDER` only to a folder you explicitly
want this test to read to additionally scan and probe its first two videos.

Browser-session research is recorded in [the R-01 spike](../research/browser-session-spike.md).
`python scripts/browser_session_spike.py --yt-dlp tools/yt-dlp.exe` probes only
disposable synthetic profiles without a media URL. It does not read your browser
profiles or establish automatic browser-import support. Repeat `--yt-dlp` with a
verified candidate executable to compare helper versions.

`cargo test --locked` checks URL validation (including mobile and shared Shorts links), format filtering and ordering, audio selection, progress parsing, session validation and domain filtering, temporary-file cleanup, and preservation of the original export. `cargo clippy --all-targets --locked -- -D warnings` checks code quality.

With the tools installed, `cargo test --locked local_download_pipeline -- --ignored --nocapture` generates a local DASH clip with separate audio/video, serves it on localhost, detects its formats, downloads and merges both streams through the production backend, and checks the saved MP4 with FFprobe. It supplies a synthetic YouTube session to both operations, checks those cookies are not sent to localhost, and verifies cancellation. Test artifacts remain in `test-output`.

The optional anonymous live YouTube test is `cargo test --locked youtube_download -- --ignored --nocapture`. Set `STREAM_DLP_TEST_URL` to override the short public test video. During development, YouTube required bot verification on this connection, so the anonymous live test could not finish. Authenticated YouTube access requires a real user-supplied session and has not been verified here. The local pipeline and synthetic session integration tests exercise the engine and session handling without reading any user's browser data.

## Source layout

- `src/main.rs`: arguments (`--check-tools`, `--dev-mode`) and the window.
- `src/app/`: the UI only. `mod.rs` holds the state, sidebar and frame loop; `theme.rs` the colors and shared widgets; one module per page or panel (`download`, `session_panel`, `settings`, `gif`, `videos`) and the `preview` widget; `dev/` Dev mode and the dev notes panel.
- `src/dev/` (no egui): the Dev mode setting, the recent-errors log and the dev notes file.
- `src/engine/` (no egui): `sources/` has the per-site rules for YouTube and TikTok (link normalization, share links, offered formats, error explanations); `media.rs` the video details and formats; `ytdlp.rs` inspection, download and progress; `error.rs` the typed `EngineError`.
- `src/media/` (no egui): GIF encoding, preview frames, and listing and checking local videos with FFmpeg and FFprobe.
- `src/process.rs`: how every helper runs: no console window, cancellation, time and output limits, and stopping the whole process tree.
- `src/tools/`: where the bundled helpers are, whether they are all there, and `--check-tools`; `update.rs` installs and updates them from their GitHub releases, as `Setup tools.ps1` does.
- `src/session/`: session cookie validation, YouTube-domain filtering and disposable working files; Firefox profile import (`firefox.rs`); the export guide's folders (`export.rs`).
- `src/update/`: app updates. `release.rs` reads the latest GitHub release and picks the installer; `installer.rs` downloads and starts it; `mod.rs` holds the hourly check and background download.
- `src/net.rs`: HTTPS with the system's certificates and proxy, and SHA-256-verified downloads.
- `src/streamable/`: the Streamable website handoff and experimental upload automation.
- `src/platform.rs`: opening folders, files and links.
- `Setup tools.ps1`: verified installation and update of external tools.
- `build.ps1`: tests, release build, portable packaging, checksums.
- `xtask/`: `cargo xtask check` and `cargo xtask release`.
- `.github/workflows/`: Checks, Package check, and Release.
