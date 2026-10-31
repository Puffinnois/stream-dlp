# stream-dlp

A native Windows video downloader for YouTube, with experimental TikTok links and an experimental Apple Silicon Mac version. It also makes GIFs from local videos and hands videos to Streamable. Rust (eframe/egui); yt-dlp, FFmpeg and Deno run as bundled helpers.

- Install: `stream-dlp-Setup-Windows-x64.exe` from [Releases](https://github.com/Puffinnois/stream-dlp/releases) (per user, no administrator access), or the portable ZIP. Mac: [docs/macos.md](docs/macos.md).
- User guide (downloading, YouTube verification, updates, download tools, GIFs, My videos and Streamable): [docs/guide.md](docs/guide.md)
- Changelog: GitHub Releases. Each release's notes are what the in-app update dialog shows.
- Dev (Windows, outside OneDrive; Rust pinned in `rust-toolchain.toml`): `cargo xtask check` (format, Clippy, Rust and Deno tests, as CI) and `cargo run`. Setup, builds, tests and source layout: [docs/reference/development.md](docs/reference/development.md)
- Try a Windows package: push a `check/<anything>` branch; the Package check builds and tests both installers.
- Release: `cargo xtask release <patch|minor|major>` on a clean `main` → `git push --follow-tags` → the Release workflow uploads a **draft** GitHub release → write its notes, press **Publish**.
- Dev mode (Ctrl+Shift+D) and dev notes (Ctrl+Shift+N): [docs/reference/dev-notes.md](docs/reference/dev-notes.md)
- Dev data: `%LOCALAPPDATA%\stream-dlp\` (Mac: `~/Library/Application Support/stream-dlp/`): `settings.json` and `dev-notes/`.
- Plans: [docs/plans/](docs/plans/); feature backlog: [ROADMAP.md](ROADMAP.md); research and feasibility records: [docs/research/](docs/research/); Streamable upload automation: [docs/reference/automatic-streamable-upload.md](docs/reference/automatic-streamable-upload.md)

Download videos you own or have permission to save. Component licenses: [THIRD-PARTY.md](THIRD-PARTY.md).
