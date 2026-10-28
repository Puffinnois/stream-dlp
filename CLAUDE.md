# stream-dlp

Native Rust (eframe/egui) video downloader for Windows, experimental on macOS; yt-dlp, FFmpeg and Deno are bundled helpers in `tools/`.

- Current plan, status and open decisions: `docs/plans/2026-10-27-ffc-alignment.md`. Read it before restructuring, updater or tooling work.
- Check before committing: `cargo xtask check` (format, Clippy, Rust and Deno tests; CI runs the same).
- Build and release: README "Develop and release" (`cargo xtask release <bump>` on main, `git push --follow-tags`, then publish the draft release).
- Commits: Conventional Commits, no Co-Authored-By trailer; branches merged with `merge: <branch> (<what>)`.
