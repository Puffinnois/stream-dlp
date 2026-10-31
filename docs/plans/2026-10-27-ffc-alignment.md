# Backend restructure, update rework, and dev environment — plan

**Started:** 2026-10-27 · **Goal:** restructure the backend, make the repository easier to work on with Claude, and align the release/update workflow and dev environment with the fantasy-football-companion project (FFC, in WSL at `~/project/fantasy-football-companion`: `.github/workflows/`, `src/main/updater.ts`, `docs/reference/dev-notes.md`).

This file holds the plan and its status. Feature work stays in [ROADMAP.md](../../ROADMAP.md); how to build and release is in [docs/reference/development.md](../reference/development.md).

## Status

- **Phase 1 — done (2026-10-27).** Merged to `main` (`merge: feat/release-pipeline (phase 1)`, then `merge: fix/mac-ffmpeg-encoders`). Includes the TikTok share-link work, quick fixes (below), `cargo xtask check|release`, the Checks / Package check / Release workflows, `build.ps1` checksums and paths, and the Mac FFmpeg encoders for GIFs and previews. Checks pass on Windows and Mac.
- **v0.3.9 is a draft release, not published.** Its notes are only a changelog link; they must cover 0.3.8 (never published: local video previews) and 0.3.9 (TikTok share links, periodic update checks, fixes). Publishing moves the assets to `/releases/download/v0.3.9/…`, the URL installed apps require.
- Before 0.3.9, an install set to the Dev channel only looks for Dev prereleases, and there will be none: it switches to Public once in Settings.
- Push from WSL: its `gh` login has the `workflow` scope (needed for `.github/workflows`); Windows `gh` is not logged in. The first tag pushed together with a new workflow file did not start the Release run; re-pushing the tag fixed it.
- **Phase 2 — done (2026-10-27).** Merged to `main` (`merge: refactor/backend (phase 2)`). Checks pass on Windows and Mac, and the localhost download pipeline test passes; the live YouTube/TikTok tests have not run on it.
- **Phase 3 — done (2026-10-30).** Merged to `main` (`merge: feat/update-rework (phase 3)`). Checks pass on Windows and Mac (the Mac builds the new HTTP client). The Package check's update installer refuses a first installation, then updates an installation in place and keeps its tools. The live GitHub release check and a live install of all tools into an empty folder pass. Not yet verified: a real in-app update, which needs two published releases that both have this updater.
- **Phase 4 — done (2026-10-30).** Merged to `main` (`merge: feat/dev-environment (phase 4)`), developed in the new working copy `C:\dev\stream-dlp` (decision 2). Checks pass on Windows and Mac with the pinned toolchain (`rustup toolchain install` reads `rust-toolchain.toml`) and the new action versions, without deprecation warnings; the Mac builds `chrono`. The Package check passes, and both installers ship only `docs/guide.md` and `docs/macos.md`. In the running debug app, Ctrl+Shift+N opens the panel, a screenshot attaches without the panel in it, and the DEV chip and + Note button show; a headless test saves a note with a screenshot. Not yet seen on screen: the All notes window.

## Decisions

1. **Release channels — one stable channel** (decided 2026-10-30). Dev builds and the Public/Dev setting are gone; phase 4's Dev mode holds the developer tools.
2. **Where to develop — Windows-native in `C:\dev\stream-dlp`** (decided 2026-10-30). A fresh clone with the gitignored `tools/` copied over replaces the OneDrive copy, which synced ~6.4 GB of build output. WSL was set aside: the Firefox import, installers and updater are Windows-only, and the app has no Linux helpers or Linux build.
3. **Update size — slim update installer** (decided 2026-10-30), about 4 MB against 165 MB for the full installer.

## Phase 2 — backend restructure (done)

Behaviour-preserving except the last item. The "Source layout" in [docs/reference/development.md](../reference/development.md) lists every module.

- **`src/process.rs`** runs every helper: `isolate` (no console window, own Unix process group; was `backend::hidden`), `stop` (whole tree), `Owned` (stops on drop), and `Helper` (time limit, per-stream output limit, cancellation; `output()` collects, `lines()` streams). The seven hand-written loops are gone; probe and preview now stop the tree too.
- **`src/tools.rs`**: location, `ready()` cached for two seconds, `check()` for `--check-tools`.
- **`src/engine/`** replaces `backend.rs` and `tiktok.rs`. `parse_link` returns a `Link { source, url }`, so the site is never re-derived from the URL; `sources/{youtube,tiktok}.rs` hold normalization, share links, format policy and error classification; yt-dlp's JSON is parsed once, straight into `Video`. The YouTube session goes only to canonical YouTube watch links.
- **`EngineError`** (unsupported, sign-in needed, rate limited, unavailable, restricted, network, timed out, helper) with `user_message()` and `needs_session()`, replacing the `YOUTUBE_SIGN_IN` text match. It covers the engine only; media, session and update errors stay `String`, since they are already user-facing text. YouTube errors other than sign-in are still passed through as yt-dlp's message (`Helper`); classifying them is F-03 work.
- **`src/app/`** splits `main.rs` (now 40 lines): `mod.rs`, `theme.rs`, `download.rs`, `session_panel.rs`, `settings.rs`, plus the UI halves of the GIF, My videos and preview modules (`gif.rs`, `videos.rs`, `preview.rs`). Their FFmpeg/FFprobe halves are `src/media/{gif,preview,local}.rs` ("local" lists and probes local videos). `session/`, `update/` and `streamable/` group the rest.
- Enums replace the `usize` states (`FormatFilter`, `SessionMode`, `ImportBrowser`, `ExportBrowser`, `ExportStep`).
- **Behaviour change:** the inspection/download and the session import have separate cancel tokens, and a GIF conversion or upload no longer disables the Download page (`download_busy()`); closing and installing an update still wait for everything (`busy()`).
- Left for later: `--dev-mode` (phase 4).

## Phase 3 — update rework (done)

| | FFC | stream-dlp before | stream-dlp now |
| --- | --- | --- | --- |
| Check | launch + hourly | launch + every 4 h, through PowerShell | launch + hourly, Rust HTTP |
| Download | background, delta | after confirmation, full installer | background, slim installer (installed copies) |
| Install | silent, relaunch | Inno wizard, app closes | silent, relaunch |
| Notes | release notes in a popup | not shown | release notes in the update dialog |

- **`src/net.rs`**: `ureq` with rustls, the system's certificate verifier and the Windows proxy setting (what the PowerShell calls used); a 30-second stall timeout instead of a total one; downloads hash while streaming and are deleted unless they match the SHA-256.
- **`src/update/`**: `release.rs` reads `/releases/latest` (so drafts and prereleases never show) and picks the installer: an installed copy (setup's `unins000.exe` beside the app) prefers `stream-dlp-Update-Windows-x64.exe` and falls back to the full installer, a portable copy takes the full one, the Mac none. The exact asset URL and digest checks are kept. `mod.rs` checks at launch and hourly and starts an installed copy's download as soon as an update is found; a failed download is retried at the next check or by hand. The sidebar's **Update to X** opens a dialog with the release notes (headings, bullets, bold), the progress and **Update & restart** (only while nothing else runs). A background download no longer blocks the pages or closing.
- **Silent install**: `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /RELAUNCH=1`; a `[Run]` entry reopens the app only with `/RELAUNCH=1`, so CI's silent installs do not. Portable copies download the full installer on request and open the wizard.
- **Slim installer**: the same `installer/stream-dlp.iss` with `/DUpdateOnly` (no tools, no folder page); it refuses to run without the uninstall entry. Package check and Release check that refusal, then an in-place update that keeps the tools. The full installer keeps its name for new installs and for 0.3.9 and older.
- **Tools**: `src/tools/update.rs` ports `Setup tools.ps1`; **Settings → Download tools → Update download tools** downloads only helpers whose release digest differs from `versions.json`, moves a running helper aside instead of failing, and disables the pages that use the tools meanwhile. Windows only: the Mac's tools are inside its signed app bundle.
- **Channels removed**: the setting (its `update-channel.json` is ignored), `-dev.N` versions in `xtask` and `build.ps1`, the Release workflow's prerelease flag, and `docs/release-channels.md` (the installer deletes the installed copy).
- Not done: installing on quit (FFC does), resuming an interrupted download, and Mac downloads.

## Phase 4 — environment and Dev mode (done)

- **Working copy**: `C:\dev\stream-dlp` (decision 2), which also settles the name: folder, crate and app are all `stream-dlp`; the old `dist/StreamSave/StreamSave.exe` stayed behind in the OneDrive copy.
- **Docs**: the README is a short index like FFC's. The user guide is `docs/guide.md`; `docs/` keeps only what ships with the app (the guide, `macos.md`), and `build.ps1` and the installer ship just those (the installer replaces `{app}\docs` as a whole; the Mac bundle adds `User guide.md`). Then `reference/` (development, dev notes, the Streamable upload automation), `plans/`, `research/` (the feasibility and spike records) and `releases/` (notes of 0.3.4–0.3.8). No `specs/` until there is a design to keep.
- **Dev mode and dev notes** ([reference](../reference/dev-notes.md)): Ctrl+Shift+D and `--dev-mode`; the DEV chip and + Note button; the panel with tag, screenshots (`ViewportCommand::Screenshot`, panel hidden), context, recent errors; the list with Mark done, Copy for Claude, Delete, Change folder. `notes.md` is FFC's format in `%LOCALAPPDATA%\stream-dlp\dev-notes\`. Not ported: FFC's snip (crop) tool and Go there.
- **Toolchain**: `rust-toolchain.toml` pins 1.93.1 with rustfmt and Clippy; CI installs it with `rustup toolchain install` instead of naming the version.
- **Actions**: all workflows use `checkout@v7`, `setup-python@v7`, `cache@v6` and `upload-artifact@v7` (Node 24).

## Done in phase 1 (do not redo)

Dev channel follows newer stable releases; non-version tags are skipped; installer folders in %TEMP% are removed after a day; sign-in detection uses one constant; the Audio/Video filter moves a hidden selection; stale status lines are cleared; the retry-help button follows the failed link; the Firefox import's progress callback gets a shared pointer; `.gitattributes`; Deno tests run in `cargo xtask check`; Mac FFmpeg has the png/gif/rawvideo encoders and zlib; `build-macos.py` calls `lipo` correctly.

## Conventions

Conventional Commits; feature branches merged with `merge: <branch> (<what>)`; no Co-Authored-By trailer; `cargo xtask check` before committing; releases only through `cargo xtask release` and the Release workflow.
