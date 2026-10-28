# Backend restructure, update rework, and dev environment — plan

**Started:** 2026-10-27 · **Goal:** restructure the backend, make the repository easier to work on with Claude, and align the release/update workflow and dev environment with the fantasy-football-companion project (FFC, in WSL at `~/project/fantasy-football-companion`: `.github/workflows/`, `src/main/updater.ts`, `docs/reference/dev-notes.md`).

This file holds the plan and its status. Feature work stays in [ROADMAP.md](../../ROADMAP.md); how to build and release is in the README's "Develop and release".

## Status

- **Phase 1 — done (2026-10-27).** Merged to `main` (`merge: feat/release-pipeline (phase 1)`, then `merge: fix/mac-ffmpeg-encoders`). Includes the TikTok share-link work, quick fixes (below), `cargo xtask check|release`, the Checks / Package check / Release workflows, `build.ps1` checksums and paths, and the Mac FFmpeg encoders for GIFs and previews. Checks pass on Windows and Mac.
- **v0.3.9 is a draft release, not published.** Its notes are only a changelog link; they must cover 0.3.8 (never published: local video previews) and 0.3.9 (TikTok share links, periodic update checks, fixes). Publishing moves the assets to `/releases/download/v0.3.9/…`, the URL installed apps require.
- Installs on 0.3.5-dev.1 run the old updater, which only looks for Dev prereleases: they switch to Public once in Settings.
- Push from WSL: its `gh` login has the `workflow` scope (needed for `.github/workflows`); Windows `gh` is not logged in. The first tag pushed together with a new workflow file did not start the Release run; re-pushing the tag fixed it.
- **Phase 2 — done on branch `refactor/backend` (2026-10-27), not pushed or merged.** `cargo xtask check` passes on Windows, Clippy passes for `aarch64-apple-darwin`, and the localhost download pipeline test passes; the Mac build and the live YouTube/TikTok tests have not run on it.

## Open decisions

1. **Release channels.** Drop separate Dev builds for an FFC-style Dev mode inside the one build (phase 4), or keep the Dev channel (it now also receives newer stable releases).
2. **Where to develop.** The repo sits in OneDrive with ~6.4 GB of build output (`target/`, `dist/`). Either move it to WSL (`~/project/stream-dlp`, like FFC: `cargo run` under WSLg for UI and engine work, Windows-only parts — Firefox import, installer updates — verified through `check/**` Package check runs; the Linux build is not verified yet), or keep Windows-native outside OneDrive (e.g. `C:\dev\stream-dlp`).
3. **Update size.** Publish a slim update installer without the tools (the app is ~3 MB of the ~180 MB installer) for in-app updates.

## Phase 2 — backend restructure (done on `refactor/backend`)

Behaviour-preserving except the last item. The README's "Source layout" lists every module.

- **`src/process.rs`** runs every helper: `isolate` (no console window, own Unix process group; was `backend::hidden`), `stop` (whole tree), `Owned` (stops on drop), and `Helper` (time limit, per-stream output limit, cancellation; `output()` collects, `lines()` streams). The seven hand-written loops are gone; probe and preview now stop the tree too.
- **`src/tools.rs`**: location, `ready()` cached for two seconds, `check()` for `--check-tools`.
- **`src/engine/`** replaces `backend.rs` and `tiktok.rs`. `parse_link` returns a `Link { source, url }`, so the site is never re-derived from the URL; `sources/{youtube,tiktok}.rs` hold normalization, share links, format policy and error classification; yt-dlp's JSON is parsed once, straight into `Video`. The YouTube session goes only to canonical YouTube watch links.
- **`EngineError`** (unsupported, sign-in needed, rate limited, unavailable, restricted, network, timed out, helper) with `user_message()` and `needs_session()`, replacing the `YOUTUBE_SIGN_IN` text match. It covers the engine only; media, session and update errors stay `String`, since they are already user-facing text. YouTube errors other than sign-in are still passed through as yt-dlp's message (`Helper`); classifying them is F-03 work.
- **`src/app/`** splits `main.rs` (now 40 lines): `mod.rs`, `theme.rs`, `download.rs`, `session_panel.rs`, `settings.rs`, plus the UI halves of the GIF, My videos and preview modules (`gif.rs`, `videos.rs`, `preview.rs`). Their FFmpeg/FFprobe halves are `src/media/{gif,preview,local}.rs` ("local" lists and probes local videos). `session/`, `update/` and `streamable/` group the rest.
- Enums replace the `usize` states (`FormatFilter`, `SessionMode`, `ImportBrowser`, `ExportBrowser`, `ExportStep`).
- **Behaviour change:** the inspection/download and the session import have separate cancel tokens, and a GIF conversion or upload no longer disables the Download page (`download_busy()`); closing and installing an update still wait for everything (`busy()`).
- Left for later: `--dev-mode` (phase 4).

## Phase 3 — update rework (FFC parity)

| | FFC | stream-dlp now |
| --- | --- | --- |
| Check | launch + hourly | launch + every 4 h, through PowerShell |
| Download | background, delta | after confirmation, full installer via PowerShell |
| Install | silent, relaunch | Inno wizard, app closes |
| Notes | release notes in a popup | not shown |

- Replace the PowerShell calls in `updater.rs` with a Rust HTTP client (e.g. `ureq` with rustls): check and download with progress; Mac can at least show that an update exists.
- Check at launch and hourly. Download in the background once found; an "Update to X" button opens a modal with the release notes (the GitHub release body), progress, and **Update & restart**.
- Silent install: run the installer with `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART` and add an Inno `[Run]` entry that relaunches after a silent install (the current one has `skipifsilent`). Portable copies keep the interactive flow. Keep the SHA-256 digest and exact asset URL checks.
- If decision 3 is yes: a slim update installer for in-app updates, while the full installer keeps the name `stream-dlp-Setup-Windows-x64.exe` (installed versions up to 0.3.9 accept only that URL).
- Tools: yt-dlp breaks most often and updates only by reinstalling or running `Setup tools.ps1`. Port that script's digest-verified fetch to Rust and add **Update download tools**.
- Channels as decided (decision 1).

## Phase 4 — environment and Dev mode

- Short README index like FFC's; move the user guide to `docs/`; split docs into `specs/`, `plans/`, `research/` (the feasibility files), `reference/`.
- Dev mode (Ctrl+Shift+D, `--dev-mode`) and dev notes (Ctrl+Shift+N; screenshots through egui's `ViewportCommand::Screenshot`; `notes.md` in the app data folder; **Copy for Claude**), ported from FFC's `docs/reference/dev-notes.md`.
- Pin Rust in `rust-toolchain.toml` (CI uses 1.93.1), as `.nvmrc` does for FFC.
- Settle the name: folder `streamsave`, crate and app `stream-dlp`, an old `dist/StreamSave/StreamSave.exe`.
- The Mac job still uses Node 20 actions (`checkout@v4`, `setup-python@v5`, `cache@v4`, `upload-artifact@v4`); move to current major versions.

## Done in phase 1 (do not redo)

Dev channel follows newer stable releases; non-version tags are skipped; installer folders in %TEMP% are removed after a day; sign-in detection uses one constant; the Audio/Video filter moves a hidden selection; stale status lines are cleared; the retry-help button follows the failed link; the Firefox import's progress callback gets a shared pointer; `.gitattributes`; Deno tests run in `cargo xtask check`; Mac FFmpeg has the png/gif/rawvideo encoders and zlib; `build-macos.py` calls `lipo` correctly.

## Conventions

Conventional Commits; feature branches merged with `merge: <branch> (<what>)`; no Co-Authored-By trailer; `cargo xtask check` before committing; releases only through `cargo xtask release` and the Release workflow.
