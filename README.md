# stream-dlp

A small native YouTube downloader written in Rust. Windows is the established release; experimental Apple Silicon Mac support is being added. A light, minimal interface with blue accents, inspired by RustDesk.

## Public and Dev versions

Use **About & tools → Update channel** to choose **Public** (stable releases) or
**Dev** (experimental prereleases). Selection is remembered; installing a build
requires **Install selected build** and completing setup. The sidebar shows the
build currently running. Dev builds do not update Public users.

Public `0.3.5` and Dev `0.3.5-dev.1` both include the channel selector. Install
either build to start switching inside the app. The older Public v0.3.4 has no
selector, so switching from that version requires installing a newer build
first. Mac updates remain manual.

See [release channels and promotion](docs/release-channels.md). Windows Dev build
artifacts are written under `dist/dev/<version>/` by `build.ps1`.

## Experimental Mac version

The first Mac target is Apple Silicon (M1 or newer), macOS 15 or newer. The Mac
package includes the download tools, uses manual cookie-file import and manual
app updates, and requires no paid Apple developer account. It is ad-hoc signed
and not notarized, so a browser-downloaded copy may require **Open Anyway** in
macOS Privacy & Security. Initial real-device acceptance is still pending.

See [Mac build, installation, automated checks, and acceptance checklist](docs/macos.md).
The GitHub Actions workflow is prepared in `ci/checks.yml`; activation is pending
permission to add workflows. Once activated, it builds an experimental app ZIP
without publishing a release. The instructions below describe Windows unless
otherwise stated.

## Use

Open `dist/stream-dlp/stream-dlp.exe`. Paste a YouTube video URL and click **Find video** (or press Enter). Confirm the title and creator, select a resolution and container, then click **Choose folder & download**. The native Windows folder picker asks where to save the file. A prominent progress bar shows the current stream's percentage directly inside the bar. Click **Open folder** when the download finishes.

The format list shows every non-DRM video and audio stream returned by YouTube, including codec, frame rate, format ID, language, and stream size when available. Video-only choices automatically download matching audio and merge it without re-encoding, so they do not display a missing-audio warning. If matching audio cannot be obtained, the download reports an error instead of silently saving a video without sound. MP4 prefers M4A audio; WebM prefers WebM audio. FFmpeg may use MKV when the available streams cannot share the selected container. Stream size estimates exclude added audio. Progress describes the current stream; audio download and final merging follow. Audio choices save the original audio format without conversion.

Cancel stops the active process tree. Incomplete `.part` files are retained so a later attempt in the same folder can resume. Existing complete files are not overwritten. Closing the app signals cancellation. Regular videos and Shorts use the same downloader. Active live streams and DRM-protected formats are not supported. YouTube can change availability; run the tool setup again if extraction stops working.

## When YouTube asks you to verify

Click **Connect browser / YouTube session** below the video link, or **Connect browser / retry help** in an error message. Choose **Firefox · automatic**, choose your profile (the default is selected for you), and click **Import from Firefox & retry**. No cookie exporter or file selection is needed. The app imports YouTube cookies in the background and retries video detection or the failed download once, keeping its quality and destination. **Auto mode** is on by default: after connecting, an authentication error refreshes only that same Firefox profile and retries once per attempt. Uncheck it to refresh only when you click **Refresh from Firefox & retry**. Browser/profile choices and cookies last for the current app session.

Firefox automatic import is experimental and supports normal, unpartitioned profile cookies on Windows, with database schemas through 17. Private windows and Firefox containers need manual export. The importer opens the selected database read-only through Windows' SQLite component, selects only `youtube.com` and its subdomains, and skips expired cookies. It does not copy the whole browser database or create an all-site cookie export. Import runs in the background with cancellation and a query time budget. If the profile is busy or inaccessible, close Firefox and retry. A successful import means cookies were read, not that YouTube has accepted the sign-in.

Chrome and Edge currently offer the guided export path. For those browsers, or if Firefox import fails, choose **I have a cookies file** to import your own Netscape-format `cookies.txt` export from a signed-in YouTube session. The session applies to both video detection and downloading.

Need help making the file? Choose **Export guide**. The three-step guide offers Chrome/Edge and Firefox exporter links, explains the private-window YouTube export, and suggests saving `youtube-cookies.txt` in your Windows Downloads folder under `stream-dlp`. **Open export folder** creates and opens that folder; **Copy folder path** lets you paste its location into a Save As dialog. The folder is located using Windows' Downloads setting, including redirected Downloads folders.

The final guide step opens the file picker in the suggested folder (or Downloads if the subfolder has not been created). **Look in Downloads instead** handles browsers that save exports automatically. The guide also explains using the browser's Ctrl+J downloads list and Show in folder to locate exports such as `cookies (1).txt`. You can browse to any other folder. Subsequent imports start in the last chosen folder for the current app session, and a loaded session shows the selected file path. JSON, HTML, and exports without YouTube cookies receive an explanation rather than being silently accepted.

The dialog links to the [official YouTube cookie export instructions](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies). Complete sign-in and any verification yourself in your browser, export only `youtube.com` cookies, and import the export. JSON exports are not supported. Cookie files act like a sign-in: keep them private. The upstream project also notes that account use can trigger restrictions, so use a session only when needed.

The imported session stays in memory until you click **Remove session** or close the app. stream-dlp keeps only YouTube-domain cookie entries, normalizes the file format, and supplies a separate temporary copy to yt-dlp for each operation. The copy is deleted when the operation finishes, fails, or is cancelled. The original export is not changed, and the session is not saved in the portable app folder or included in its ZIP.

Private, age-restricted, or members-only videos require an account with access, and even a signed-in session can be expired, rate-limited, or challenged by YouTube. Firefox cookies are read only after you explicitly connect a profile; automatic refresh is limited to that source. Complete any sign-in or verification yourself in the browser.

## First-time tool setup / updates

The Windows installer is `dist/stream-dlp-Setup-Windows-x64.exe`. It includes the download tools, installs for the current user without administrator access, adds a Start menu shortcut, and offers an optional desktop shortcut. Uninstall through Windows Settings. The app has an embedded blue download-arrow icon for Explorer, shortcuts, and the taskbar.

The app checks your selected channel in the background each time it opens. An available build offers **Install selected build** and **Later**. Use **About & tools → Update channel** to change channels or **Check for updates** to check again. Nothing downloads until you click Install selected build. Finish or cancel an active operation first. The Windows installer is verified against GitHub's SHA-256 digest, then setup opens for you to review. You may cancel setup. Network errors do not block downloading videos. Portable users can select their existing app folder in setup, or download a fresh ZIP from Releases.

Updates require a matching semantic version tag and an asset named `stream-dlp-Setup-Windows-x64.exe` with GitHub's SHA-256 digest. Public uses stable releases; Dev uses `vX.Y.Z-dev.N` prereleases. Drafts and unverified installers are rejected. Same-channel checks never downgrade, while an explicit channel switch can offer an older target-channel build. Publishing is a separate step from building locally. See [release channels](docs/release-channels.md).

The ready-to-run local portable folder includes tools. The small distributable ZIP contains the app and setup script; extract the ZIP into a writable folder, then run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File ".\Setup tools.ps1"
```

Setup downloads yt-dlp, FFmpeg/ffprobe, and Deno directly from their official GitHub releases. Each binary archive is verified against its GitHub release SHA-256 digest before installation. Setup fails closed if the upstream digest is unavailable. It installs only into the app's `tools` directory, needs no administrator access, and does not modify PATH. Close the app before updating. Internet access is needed for setup and YouTube access.

## Build from source

Requires Windows 10/11 x64, Rust 1.93 or newer, and the Visual Studio C++ build tools / Windows SDK.

```powershell
cargo test --locked
cargo build --release --locked
.\build.ps1
```

`build.ps1` creates a ready-to-run portable folder and an app/setup ZIP in `dist`. `-SkipTools` builds the app without downloading helpers. The UI and orchestration are Rust (`eframe`/`egui`); extraction is delegated to yt-dlp, media merging to FFmpeg, and JavaScript challenges to Deno. No web server or Electron runtime is used.

To also build the Windows installer, install Inno Setup and run `./build.ps1 -Installer -IsccPath 'C:\path\to\ISCC.exe'`. Installer builds require all helpers in the portable folder; `-SkipTools -Installer` reuses helpers already present there. The installer version comes from `Cargo.toml`. `scripts/make-icon.ps1` regenerates the multi-size app icon and PNG preview. `build.rs` embeds that icon and the app version into the Windows executable.

If the existing portable executable is open, use `-PortableDirectory 'dist\stream-dlp-0.3.4'` to build a separate folder. Without `-SkipTools`, setup populates that folder's helpers as usual. The generated installer and ZIP use the selected portable folder.

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

- `src/main.rs`: native UI, worker coordination, Windows folder picker.
- `src/backend.rs`: URL normalization, metadata and format parsing, yt-dlp invocation, progress, cancellation.
- `src/session.rs`: explicit session import, YouTube-domain filtering, disposable cookie working files.
- `src/browser_session.rs`: Firefox profile discovery and scoped native cookie import, cancellation and query deadline.
- `src/export_help.rs`: Windows Downloads location and file-picker folder selection.
- `Setup tools.ps1`: verified installation and update of external tools.
- `build.ps1`: tests, release build, portable packaging.

Downloads execute tools directly using argument arrays, without interpolating video links into a shell. User yt-dlp configs are ignored to keep behavior predictable. Cookies are read only from an export or Firefox profile the user explicitly chooses and passed to yt-dlp for its YouTube requests. No passwords, analytics, or download history are collected.

Download videos you own or have permission to save. See `THIRD-PARTY.md` for component licenses and upstream sources.
