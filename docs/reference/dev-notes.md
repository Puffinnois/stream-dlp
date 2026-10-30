# Dev mode and dev notes — reference

Ported from FFC's Dev mode (`~/project/fantasy-football-companion/docs/reference/dev-notes.md` in WSL), with the same notes file format, so the same workflow serves both projects. Dev mode shows developer tools inside the one app; today that is the notes panel, for writing down changes while using the app.

## Dev mode

- **Ctrl+Shift+D** anywhere switches it on or off; a toast confirms. Stored as `dev_mode` in `settings.json` in the app data folder (`%LOCALAPPDATA%\stream-dlp\`; Mac `~/Library/Application Support/stream-dlp/`). With nothing stored it is **on** in a debug build (`cargo run`) and **off** in a release build.
- `--dev-mode` on the command line turns it on for that launch only (until it is switched), e.g. a second shortcut: `stream-dlp.exe --dev-mode`.
- While on, the sidebar shows an amber **DEV** chip next to the version: New note (Ctrl+Shift+N), All notes, Open notes folder, Turn off Dev mode. A **+ Note** button sits bottom-right.
- It only shows tools; it changes no download and no setting of the app.

## Writing a note

**+ Note**, **Ctrl+Shift+N** or the DEV chip opens the panel. It opens bottom-right, drags by its title, collapses (▼) and closes (✕ or **Esc**) without losing the draft. The app stays usable around it.

| Part | What it does |
| --- | --- |
| Text | Required; the first line is the title. The draft stays until it is saved. |
| Tag | Bug · Idea (default) · Polish |
| Screenshot | The whole window without the panel, up to six; × removes one. |
| Context line | `Download · youtube.com/watch?v=… · error`: where the app was when the panel opened. **Context** shows the JSON; **Recapture** records where it is now. |
| Include recent errors | Errors the app showed in the last 15 minutes (download, GIF, session import, My videos, updates, download tools, panics), at most ten, repeats counted. Ticked by default when there are any. |
| Save | Ctrl+Enter. Writes the note and its images in the background, closes the panel, "Saved N-014". |

What a page adds to the context: Download: the link, the video's title, the chosen format, the phase, the status line, the error, whether a session is loaded (never its cookies), and yt-dlp's last ten output lines. My videos: the folder, the number of videos, the selected file, the message, the error, the upload status. GIF converter: the source, its duration and the clip settings, the message, the error. Settings: the available update, the update and tools status. Every note adds the app version, debug or release build, the OS, the window size and scale, and the time.

## All notes

Newest first; Open (default) / Done / All. Per note: Mark done / Reopen, Copy for Claude (the note's Markdown with absolute image paths, to paste into a session), Delete (asks; removes its images). Footer: Open folder, Change folder… (stored as `dev_notes_dir` in `settings.json`), Copy all open, and the path of `notes.md`.

## The notes folder

Default: `%LOCALAPPDATA%\stream-dlp\dev-notes\` (Mac `~/Library/Application Support/stream-dlp/dev-notes/`). It holds `notes.md` and `img/<id>-<n>.png`. `notes.md` starts with a short header, then one `##` section per note, oldest first:

```markdown
## N-014 · Bug · open — Format list cuts off long codec names

<!-- note {"id":"N-014","tag":"bug","status":"open","created":"2026-10-30T14:03:11+01:00","images":["img/N-014-1.png"]} -->

**Where:** Download · youtube.com/watch?v=… · 1080p mp4 · choosing a format
**When:** 2026-10-30 14:03 · v0.3.9 · debug · windows

Format list cuts off long codec names
"avc1.640028" is cut at "avc1.64…".

![Screenshot 1](img/N-014-1.png)

<details><summary>Context</summary> … the context as JSON … </details>

<details><summary>Recent errors (1)</summary> … time, origin, message … </details>
```

- The `<!-- note {...} -->` line is what the app reads: id, tag, status, created, images. Its `status` and `tag` win over the heading's; an unknown tag reads as Idea.
- Hand edits are fine: the app rewrites only the heading and meta line of a note it changes, and keeps everything else (other notes, extra lines, a section without a meta line, the header) byte for byte. Lines of a note's text that start with `#` or `##` are written escaped (`\#`), and a code fence the text leaves open is closed, so the text never starts a section.
- Every change reads the file again and writes it atomically, so a hand edit made meanwhile is kept. Delete removes only images inside `img/`.

## Working the notes with Claude Code

- "Go through my open dev notes": read `%LOCALAPPDATA%\stream-dlp\dev-notes\notes.md` (and the images it links), propose what to do per note, and after implementing one set its meta line to `"status":"done"` and add a line `Done in vX.Y.Z (<commit>)` under its text.
- Or paste one note from **Copy for Claude** into a session.

## Code

| Where | What |
| --- | --- |
| `src/dev/mod.rs` | `settings.json` (Dev mode, notes folder), the data folder, atomic writes |
| `src/dev/notes.rs` | The file's format: parse, append, set status, remove, Copy for Claude; the notes folder; PNG encoding |
| `src/dev/errors.rs` | The recent-errors log and the panic hook; pages call `dev::errors::record` where they show an error |
| `src/app/dev.rs` | Shortcuts, the DEV chip, the + Note button, the panel (screenshots through `ViewportCommand::Screenshot`), the list, each page's context |
| `note_context` in `src/app/gif.rs`, `src/app/videos.rs` | What those pages add to the context |
