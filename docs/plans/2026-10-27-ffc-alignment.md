# Backend restructure, update rework, and dev environment — plan

**Started:** 2026-10-27 · **Goal:** restructure the backend, make the repository easier to work on with Claude, and align the release/update workflow and dev environment with the fantasy-football-companion project (FFC, in WSL at `~/project/fantasy-football-companion`: `.github/workflows/`, `src/main/updater.ts`, `docs/reference/dev-notes.md`).

This file holds the plan and its status. Feature work stays in [ROADMAP.md](../../ROADMAP.md); how to build and release is in the README's "Develop and release".

## Status

- **Phase 1 — done (2026-10-27).** Merged to `main` (`merge: feat/release-pipeline (phase 1)`, then `merge: fix/mac-ffmpeg-encoders`). Includes the TikTok share-link work, quick fixes (below), `cargo xtask check|release`, the Checks / Package check / Release workflows, `build.ps1` checksums and paths, and the Mac FFmpeg encoders for GIFs and previews. Checks pass on Windows and Mac.
- **v0.3.9 is a draft release, not published.** Its notes are only a changelog link; they must cover 0.3.8 (never published: local video previews) and 0.3.9 (TikTok share links, periodic update checks, fixes). Publishing moves the assets to `/releases/download/v0.3.9/…`, the URL installed apps require.
- Installs on 0.3.5-dev.1 run the old updater, which only looks for Dev prereleases: they switch to Public once in Settings.
- Push from WSL: its `gh` login has the `workflow` scope (needed for `.github/workflows`); Windows `gh` is not logged in. The first tag pushed together with a new workflow file did not start the Release run; re-pushing the tag fixed it.

## Open decisions

1. **Release channels.** Drop separate Dev builds for an FFC-style Dev mode inside the one build (phase 4), or keep the Dev channel (it now also receives newer stable releases).
2. **Where to develop.** The repo sits in OneDrive with ~6.4 GB of build output (`target/`, `dist/`). Either move it to WSL (`~/project/stream-dlp`, like FFC: `cargo run` under WSLg for UI and engine work, Windows-only parts — Firefox import, installer updates — verified through `check/**` Package check runs; the Linux build is not verified yet), or keep Windows-native outside OneDrive (e.g. `C:\dev\stream-dlp`).
3. **Update size.** Publish a slim update installer without the tools (the app is ~3 MB of the ~180 MB installer) for in-app updates.

## Phase 2 — backend restructure (behaviour-preserving, tests stay green)

- **One helper runner (`src/process.rs`).** Spawn hidden, poll, timeout, output cap, cancel, stop the process tree. Replace the seven hand-written copies in `backend::inspect`, `backend::download`, `tiktok::resolve`, `gif_converter::run`, `video_library::probe`, `video_preview::extract`, `streamable_upload::run`; `probe` and `extract` use `child.kill()` instead of `backend::stop`. `backend::hidden` also sets the Unix process group; name it for what it does.
- **Typed errors (`EngineError`).** Sign-in needed, rate limited, unavailable, network, timeout, unsupported, helper failure, with a `user_message()`. Replaces `Result<_, String>` everywhere and the `backend::YOUTUBE_SIGN_IN` / `needs_session` stopgap from phase 1. ROADMAP F-03.
- **Per-site modules (`engine/sources/{youtube,tiktok}.rs`).** URL normalization, format policy, error classification, short links. Today these rules are spread over `normalize_url`, `source`, `error_for`, `parse_video_for` and `command_for`. ROADMAP F-01; each new site would otherwise add a branch in all five.
- `download()` should use `MediaIdentity::source` instead of recomputing `source(link)` (which re-normalizes the URL for every output line). Parse yt-dlp's JSON once (now up to three times per inspection).
- **`src/tools.rs`.** Tools location, a cached readiness check (`tools_ready()` stats four files several times per frame), `--check-tools`.
- **Split `main.rs`** (1,650 lines, a 40-field `App`) into `app/{theme,download,session_panel,settings}.rs`. Replace `usize` states (`session_mode`, `filter`, `browser_choice`, `export_browser`, `export_step`) with enums. Give each job its own cancel token instead of the shared `cancel`; the global `busy()` makes a GIF conversion or upload block downloading.

Proposed layout:

```
src/main.rs          args (--check-tools, --dev-mode), run_native
src/app/             UI only
src/engine/          no egui: media.rs, ytdlp.rs, error.rs, sources/
src/process.rs       the helper runner
src/tools.rs
src/session/  src/update/  src/media/ (gif, preview, probe)  src/streamable/
```

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
