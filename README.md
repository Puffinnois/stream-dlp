# stream-dlp

A small native YouTube downloader written in Rust, with experimental TikTok video support. Windows is the established release; experimental Apple Silicon Mac support is being added. A minimal workspace with a charcoal navigation rail, light cards, and indigo accents.

## Experimental TikTok support (development build)

Paste a full public `www.tiktok.com/@creator/video/<id>` or
`www.tiktok.com/share/video/<id>` link, then use **Find video** and the existing
download workflow. TikTok currently offers H.264/H.265 video formats containing
audio. Portrait dimensions appear directly in the quality list. TikTok filenames
have a source prefix, and YouTube sessions are never attached to TikTok requests.

Short/mobile links, photo posts, profiles, collections, live streams, authenticated
access, silent clips, and video-only formats are not supported in this first
implementation. For a short link, open it in your browser and copy the full video
URL. Website restrictions can still prevent a public video from downloading.
See [TikTok validation and remaining work](docs/tiktok-feasibility.md).

## Experimental Mac version

The first Mac target is Apple Silicon (M1 or newer), macOS 15 or newer. The Mac
package includes the download tools, uses manual cookie-file import and manual
app updates, and requires no paid Apple developer account. It is ad-hoc signed
and not notarized, so a browser-downloaded copy may require **Open Anyway** in
macOS Privacy & Security. Initial real-device acceptance is still pending.

See [Mac build, installation, automated checks, and acceptance checklist](docs/macos.md).
The Checks workflow (`.github/workflows/checks.yml`) builds an experimental app ZIP
on every push, without publishing a release. The instructions below describe
Windows unless otherwise stated.

## Use

Open `dist/stream-dlp/stream-dlp.exe`. Paste a YouTube video URL and click **Find video** (or press Enter). Confirm the title and creator, select a resolution and container, then click **Choose folder & download**. The native Windows folder picker asks where to save the file. A prominent progress bar shows the current stream's percentage directly inside the bar. Click **Open folder** when the download finishes.

The format list shows every non-DRM video and audio stream returned by YouTube, including codec, frame rate, format ID, language, and stream size when available. Video-only choices automatically download matching audio and merge it without re-encoding, so they do not display a missing-audio warning. If matching audio cannot be obtained, the download reports an error instead of silently saving a video without sound. MP4 prefers M4A audio; WebM prefers WebM audio. FFmpeg may use MKV when the available streams cannot share the selected container. Stream size estimates exclude added audio. Progress describes the current stream; audio download and final merging follow. Audio choices save the original audio format without conversion.

Cancel stops the active process tree. Incomplete `.part` files are retained so a later attempt in the same folder can resume. Existing complete files are not overwritten. Closing the app signals cancellation. Regular videos and Shorts use the same downloader. Active live streams and DRM-protected formats are not supported. YouTube can change availability; run the tool setup again if extraction stops working.

Missing video details are shown explicitly: an absent title falls back to the
video ID or **Untitled video**, and missing creator, duration, resolution, bitrate,
container, and size use unknown labels. Unknown duration and resolution are not
shown as zero. Estimated stream sizes keep a `~` prefix; files smaller than 1 MB
show `<1 MB`.

## When YouTube asks you to verify

Click **Connect browser…** below the video link to expand the inline connection panel, or **Connect browser / retry help** in an error message. Choose **Firefox · automatic**, choose your profile (the default is selected for you), and click **Import from Firefox & retry**. No cookie exporter or file selection is needed. Successful import collapses the panel into **Session loaded · Manage**; connection errors stay inline. The app imports YouTube cookies in the background and retries video detection or the failed download once, keeping its quality and destination. **Auto mode** is on by default: after connecting, an authentication error refreshes only that same Firefox profile and retries once per attempt. Uncheck it to refresh only when you click **Refresh from Firefox & retry**. Browser/profile choices and cookies last for the current app session.

Firefox automatic import is experimental and supports normal, unpartitioned profile cookies on Windows, with database schemas through 17. Private windows and Firefox containers need manual export. The importer opens the selected database read-only through Windows' SQLite component, selects only `youtube.com` and its subdomains, and skips expired cookies. It does not copy the whole browser database or create an all-site cookie export. Import runs in the background with cancellation and a query time budget. If the profile is busy or inaccessible, close Firefox and retry. A successful import means cookies were read, not that YouTube has accepted the sign-in.

Chrome and Edge currently offer the guided export path. For those browsers, or if Firefox import fails, choose **Import a cookies file…** to import your own Netscape-format `cookies.txt` export from a signed-in YouTube session. The session applies to both video detection and downloading.

Need help making the file? Choose **Export guide**. The three-step guide offers Chrome/Edge and Firefox exporter links, explains the private-window YouTube export, and suggests saving `youtube-cookies.txt` in your Windows Downloads folder under `stream-dlp`. **Open export folder** creates and opens that folder; **Copy folder path** lets you paste its location into a Save As dialog. The folder is located using Windows' Downloads setting, including redirected Downloads folders.

The final guide step opens the file picker in the suggested folder (or Downloads if the subfolder has not been created). **Look in Downloads instead** handles browsers that save exports automatically. The guide also explains using the browser's Ctrl+J downloads list and Show in folder to locate exports such as `cookies (1).txt`. You can browse to any other folder. Subsequent imports start in the last chosen folder for the current app session, and a loaded session shows the selected file path. JSON, HTML, and exports without YouTube cookies receive an explanation rather than being silently accepted.

The inline connection panel links to the [official YouTube cookie export instructions](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies). Complete sign-in and any verification yourself in your browser, export only `youtube.com` cookies, and import the export. JSON exports are not supported. Cookie files act like a sign-in: keep them private. The upstream project also notes that account use can trigger restrictions, so use a session only when needed.

The imported session stays in memory until you click **Remove session** or close the app. stream-dlp keeps only YouTube-domain cookie entries, normalizes the file format, and supplies a separate temporary copy to yt-dlp for each operation. The copy is deleted when the operation finishes, fails, or is cancelled. The original export is not changed, and the session is not saved in the portable app folder or included in its ZIP.

Private, age-restricted, or members-only videos require an account with access, and even a signed-in session can be expired, rate-limited, or challenged by YouTube. Firefox cookies are read only after you explicitly connect a profile; automatic refresh is limited to that source. Complete any sign-in or verification yourself in the browser.

## First-time tool setup / updates

The Windows installer is `dist/stream-dlp-Setup-Windows-x64.exe`. It includes the download tools, installs for the current user without administrator access, adds a Start menu shortcut, and offers an optional desktop shortcut. Uninstall through Windows Settings. The app has an embedded blue download-arrow icon for Explorer, shortcuts, and the taskbar.

### App updates

The app checks GitHub for a newer stable release when it opens and every hour while it stays open; it never interrupts you with a popup. An installed copy then downloads the small update installer in the background (`stream-dlp-Update-Windows-x64.exe`: the app without the tools, about 4 MB) and the sidebar shows **Update to X**. It opens the release notes and the download progress. **Update & restart** installs silently and reopens the app; it is available once the download is verified and nothing else is running. **Later**, Escape, or clicking outside closes the dialog. **Settings → Updates** has the same button and **Check for updates**.

A portable copy downloads the full installer only when you choose **Download setup**; **Open setup** then runs the setup wizard, where you choose the portable folder (or download a fresh ZIP from Releases instead). Failed checks retry at the next hourly check and keep a previously found update; a failed download is retried then too, or with **Retry download**. Network errors never block downloading videos. Mac updates are manual: the Mac version shows that a release exists and links to its page.

An update must be a published release (not a draft or prerelease) whose version tag is newer than the running app, with its installer at this repository's release URL and GitHub's SHA-256 digest. The download must match that digest before anything runs. Updates never downgrade. Every release still publishes the full installer, `stream-dlp-Setup-Windows-x64.exe`, as versions up to 0.3.9 update only through it. Before 0.3.9, a copy set to the former Dev channel sees only Dev prereleases, and there will be no more of them: choose **Public** in its Settings once. Releases are built by CI and published by hand (see [Develop and release](#develop-and-release)).

### Download tools

The installer and the ready-to-run local portable folder include the tools. **Settings → Download tools → Update download tools** downloads only the tools with a new release, each checked against GitHub's SHA-256 digest; a new yt-dlp is the usual fix when a site stops working. The small distributable ZIP contains the app and setup script: extract it into a writable folder, then use **Install download tools** in Settings, or run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File ".\Setup tools.ps1"
```

Both download yt-dlp, FFmpeg/ffprobe, and Deno directly from their official GitHub releases and record them in `tools/versions.json`. Each binary archive is verified against its GitHub release SHA-256 digest before installation. Setup fails closed if the upstream digest is unavailable. It installs only into the app's `tools` directory, needs no administrator access, and does not modify PATH. Close the app before running the script; the Settings button waits until nothing is running. Internet access is needed for setup and YouTube access.

## Build from source

Requires Windows 10/11 x64, Rust 1.93 or newer, and the Visual Studio C++ build tools / Windows SDK.

```powershell
cargo xtask check
.\build.ps1
```

`cargo xtask check` runs formatting, Clippy, the Rust tests, and the Deno script tests (`scripts/*.test.js`, using `tools/deno` or `deno` on PATH), exactly as CI does. `build.ps1` runs the Rust tests, then creates a ready-to-run portable folder, an app/setup ZIP, and `SHA256SUMS.txt` in `dist`. `-SkipTools` builds the app without downloading helpers; `-SkipTests` skips the tests. The UI and orchestration are Rust (`eframe`/`egui`); extraction is delegated to yt-dlp, media merging to FFmpeg, and JavaScript challenges to Deno. No web server or Electron runtime is used.

To also build the Windows installers, install Inno Setup and run `./build.ps1 -Installer -IsccPath 'C:\path\to\ISCC.exe'`. This builds the full installer and, from the same `installer/stream-dlp.iss` with `/DUpdateOnly`, the update installer without the tools, which refuses to install where stream-dlp is not installed yet. Installer builds require all helpers in the portable folder; `-SkipTools -Installer` reuses helpers already present there. The installer version comes from `Cargo.toml`. `scripts/make-icon.ps1` regenerates the multi-size app icon and PNG preview. `build.rs` embeds that icon and the app version into the Windows executable.

If the existing portable executable is open, use `-PortableDirectory 'dist\stream-dlp-0.3.4'` to build a separate folder. Without `-SkipTools`, setup populates that folder's helpers as usual. The generated installer and ZIP use the selected portable folder.

## Develop and release

- Checks: `cargo xtask check`. The **Checks** workflow runs it (and the Mac build) on every pushed branch and pull request.
- Try a change on Windows without releasing: push a branch named `check/<anything>`. The **Package check** workflow builds both installers, checks that the packaged and the silently installed app can run their bundled tools (`--check-tools`), that the update installer refuses a first installation and then updates that installation in place, and attaches the installers, ZIP, and checksums to the run.
- Release: on a clean `main`, `cargo xtask release <patch|minor|major>` (or an explicit newer `X.Y.Z`) bumps `Cargo.toml` and `Cargo.lock`, commits `build: bump version to X.Y.Z`, and tags `vX.Y.Z`. Then `git push --follow-tags`. The **Release** workflow checks the tag matches `Cargo.toml`, runs the checks, builds and checks both installers, and uploads them, the ZIP, and `SHA256SUMS.txt` to a **draft** GitHub release with generated notes. Edit the notes (the app shows them in its update dialog) and press **Publish**; installed apps only see published releases. There are no prereleases: try changes through Package check instead.
- Changelog: the GitHub release notes.
- Restructure and update-rework plan: [docs/plans/2026-10-27-ffc-alignment.md](docs/plans/2026-10-27-ffc-alignment.md).

## GIF converter

Open **GIF converter** in the sidebar, or **Make GIF** after downloading a video.
Selecting a video shows a large frame preview. Move **Clip start** to preview your
chosen moment, or use **Play source video** to open it in your system player.
**My videos** shows thumbnails for videos in an added folder and a larger preview
for the selected video. Previews load locally in the background as you browse.
Choose a local video, set the start time and duration (0.1–60 seconds), then choose
Small file, Balanced, or High quality. Expand **Advanced settings** to adjust the maximum dimension (160–960 px),
frame rate (5–30 fps), palette colors, dithering, and looping before **Save GIF…**.
The converter preserves aspect ratio and does not upscale. GIFs have no audio.

Conversion runs locally using the bundled FFmpeg and FFprobe, with two-pass
palette optimization, bounded worker threads, progress, and cancellation. You can
switch pages while it works. Existing files are never replaced; choose a fresh
filename. Temporary output is cleaned up after cancellation or failure, and the
finished GIF offers **Preview GIF** (in your default viewer) and **Open output folder**.
Shorter clips, smaller dimensions, and lower frame rates usually save the most space.

## My videos and Streamable

Open the **My videos** page from the sidebar, then **Add video folder…** to list the videos
in a folder. Switch between **Download** and **My videos** without losing the
current download or folder selection. Select a video to check its container, duration, and size, then use
**Play video** or **Show file**. **Refresh** reloads the folder after files change.
The list includes up to 1,000 video filenames in that folder, without scanning
subfolders. The folder selection lasts for this app session; no library history
is saved. **Choose existing video…** also works without adding a folder.

After a download, **Upload on Streamable** selects the completed file in the same
interface. **Upload automatically (experimental)** opens a separate private
Edge/Chrome window, selects the file automatically, and returns a share link
after verifying playback. The app shows upload/processing status and offers
cancellation. Complete any sign-in or verification in that browser window.
Uploading is an explicit choice and accepts the linked Streamable upload terms.
Guest links may be publicly accessible and are temporary. **Use website manually**
remains available as a fallback. Neither flow changes the original file.

Automatic uploading requires installed Edge or Chrome and the bundled Deno tool.
It does not reuse your normal browser profile or YouTube session. Website changes
can break automation. Only Windows/Edge is locally exercised; see the
[automatic uploader implementation and tests](docs/automatic-streamable-upload.md)
for lifecycle details and validation limits.

## Verification

See the [v0.3.4 release notes](docs/release-0.3.4.md) for release validation and
known acceptance limits.

The [R-02 Streamable decision](docs/streamable-feasibility.md) selects the
browser-assisted sharing workflow. Authorized guest website uploads of synthetic
and existing downloaded clips processed and played successfully. Direct uploads
remain blocked by the official read-only API. Signed-in website acceptance
remains open.

`cargo test --locked local_library_probe -- --ignored --nocapture` checks a real
generated video and invalid files through FFprobe, including Unicode filenames
and cancellation. Set `STREAM_DLP_VIDEO_TEST_FOLDER` only to a folder you explicitly
want this test to read to additionally scan and probe its first two videos.

Browser-session research is recorded in [the R-01 spike](docs/browser-session-spike.md).
`python scripts/browser_session_spike.py --yt-dlp tools/yt-dlp.exe` probes only
disposable synthetic profiles without a media URL. It does not read your browser
profiles or establish automatic browser-import support. Repeat `--yt-dlp` with a
verified candidate executable to compare helper versions.

`cargo test --locked` checks URL validation (including mobile and shared Shorts links), format filtering and ordering, audio selection, progress parsing, session validation and domain filtering, temporary-file cleanup, and preservation of the original export. `cargo clippy --all-targets --locked -- -D warnings` checks code quality.

With the tools installed, `cargo test --locked local_download_pipeline -- --ignored --nocapture` generates a local DASH clip with separate audio/video, serves it on localhost, detects its formats, downloads and merges both streams through the production backend, and checks the saved MP4 with FFprobe. It supplies a synthetic YouTube session to both operations, checks those cookies are not sent to localhost, and verifies cancellation. Test artifacts remain in `test-output`.

The optional anonymous live YouTube test is `cargo test --locked youtube_download -- --ignored --nocapture`. Set `STREAM_DLP_TEST_URL` to override the short public test video. During development, YouTube required bot verification on this connection, so the anonymous live test could not finish. Authenticated YouTube access requires a real user-supplied session and has not been verified here. The local pipeline and synthetic session integration tests exercise the engine and session handling without reading any user's browser data.

## Source layout

- `src/main.rs`: arguments (`--check-tools`) and the window.
- `src/app/`: the UI only. `mod.rs` holds the state, sidebar and frame loop; `theme.rs` the colors and shared widgets; one module per page or panel (`download`, `session_panel`, `settings`, `gif`, `videos`) and the `preview` widget.
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

Downloads execute tools directly using argument arrays, without interpolating video links into a shell. User yt-dlp configs are ignored to keep behavior predictable. Cookies are read only from an export or Firefox profile the user explicitly chooses and passed to yt-dlp for its YouTube requests. No passwords, analytics, or download history are collected.

Download videos you own or have permission to save. See `THIRD-PARTY.md` for component licenses and upstream sources.
