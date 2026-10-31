# stream-dlp

Native Rust (eframe/egui) video downloader for Windows, experimental on macOS; yt-dlp, FFmpeg and Deno are bundled helpers in `tools/`.

- Current plan, status and open decisions: `docs/plans/2026-10-27-ffc-alignment.md`. Read it before restructuring, updater or tooling work.
- Check before committing: `cargo xtask check` (format, Clippy, Rust and Deno tests; CI runs the same).
- Build and release: `docs/reference/development.md` (`cargo xtask release <bump>` on main, `git push --follow-tags`, then publish the draft release). Do not run the installers on the development machine (a real install lives there); the `check/**` Package check tests them.
- Docs: user guide `docs/guide.md` (ships with the app, as does `docs/macos.md`); `docs/reference/` how things work; `docs/plans/`; `docs/research/`; `docs/releases/` old release notes.
- Dev notes: Dev mode (Ctrl+Shift+D) writes `notes.md` in `%LOCALAPPDATA%\stream-dlp\dev-notes\`; see `docs/reference/dev-notes.md` for working through them.
- Commits: Conventional Commits, no Co-Authored-By trailer; branches merged with `merge: <branch> (<what>)`.
