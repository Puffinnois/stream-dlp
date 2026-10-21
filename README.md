# stream-dlp

A small native Windows YouTube downloader written in Rust. A light, minimal interface with blue accents, inspired by RustDesk.

## Use

Open `dist/stream-dlp/stream-dlp.exe`. Paste a YouTube video URL and click **Find video** (or press Enter). Confirm the title and creator, select a resolution and container, then click **Choose folder & download**. The native Windows folder picker asks where to save the file. A prominent progress bar shows the current stream's percentage directly inside the bar. Click **Open folder** when the download finishes.

The format list shows every non-DRM video and audio stream returned by YouTube, including codec, frame rate, format ID, language, and stream size when available. Video-only choices automatically download matching audio and merge it without re-encoding, so they do not display a missing-audio warning. If matching audio cannot be obtained, the download reports an error instead of silently saving a video without sound. MP4 prefers M4A audio; WebM prefers WebM audio. FFmpeg may use MKV when the available streams cannot share the selected container. Stream size estimates exclude added audio. Progress describes the current stream; audio download and final merging follow. Audio choices save the original audio format without conversion.

Cancel stops the active process tree. Incomplete `.part` files are retained so a later attempt in the same folder can resume. Existing complete files are not overwritten. Closing the app signals cancellation. Regular videos and Shorts use the same downloader. Active live streams and DRM-protected formats are not supported. YouTube can change availability; run the tool setup again if extraction stops working.

## When YouTube asks you to verify

Click **YouTube session** below the video link, or **YouTube session / retry help** in an error message. Import your own Netscape-format `cookies.txt` export from a signed-in YouTube browser session. stream-dlp retries the current video or Short automatically after a successful import. The session applies to both video detection and downloading.

Need help making the file? Choose **Help me export cookies**. The three-step guide offers Chrome/Edge and Firefox exporter links, explains the private-window YouTube export, and suggests saving `youtube-cookies.txt` in your Windows Downloads folder under `stream-dlp`. **Open export folder** creates and opens that folder; **Copy folder path** lets you paste its location into a Save As dialog. The folder is located using Windows' Downloads setting, including redirected Downloads folders.

The final guide step opens the file picker in the suggested folder (or Downloads if the subfolder has not been created). **Look in Downloads instead** handles browsers that save exports automatically. The guide also explains using the browser's Ctrl+J downloads list and Show in folder to locate exports such as `cookies (1).txt`. You can browse to any other folder. Subsequent imports start in the last chosen folder for the current app session, and a loaded session shows the selected file path. JSON, HTML, and exports without YouTube cookies receive an explanation rather than being silently accepted.

The dialog links to the [official YouTube cookie export instructions](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies). Complete sign-in and any verification yourself in your browser, export only `youtube.com` cookies, and import the export. JSON exports are not supported. Cookie files act like a sign-in: keep them private. The upstream project also notes that account use can trigger restrictions, so use a session only when needed.

The imported session stays in memory until you click **Remove session** or close the app. stream-dlp keeps only YouTube-domain cookie entries, normalizes the file format, and supplies a separate temporary copy to yt-dlp for each operation. The copy is deleted when the operation finishes, fails, or is cancelled. The original export is not changed, and the session is not saved in the portable app folder or included in its ZIP.

This is authenticated downloading, not a universal verification bypass. Private, age-restricted, or members-only videos require an account with access, and even a signed-in session can be expired, rate-limited, or challenged by YouTube. The app does not automatically read browser profiles, solve CAPTCHAs, bypass DRM, or grant access the account does not have.

## First-time tool setup / updates

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

## Verification

`cargo test --locked` checks URL validation (including mobile and shared Shorts links), format filtering and ordering, audio selection, progress parsing, session validation and domain filtering, temporary-file cleanup, and preservation of the original export. `cargo clippy --all-targets --locked -- -D warnings` checks code quality.

With the tools installed, `cargo test --locked local_download_pipeline -- --ignored --nocapture` generates a local DASH clip with separate audio/video, serves it on localhost, detects its formats, downloads and merges both streams through the production backend, and checks the saved MP4 with FFprobe. It supplies a synthetic YouTube session to both operations, checks those cookies are not sent to localhost, and verifies cancellation. Test artifacts remain in `test-output`.

The optional anonymous live YouTube test is `cargo test --locked youtube_download -- --ignored --nocapture`. Set `STREAM_DLP_TEST_URL` to override the short public test video. During development, YouTube required bot verification on this connection, so the anonymous live test could not finish. Authenticated YouTube access requires a real user-supplied session and has not been verified here. The local pipeline and synthetic session integration tests exercise the engine and session handling without reading any user's browser data.

## Source layout

- `src/main.rs`: native UI, worker coordination, Windows folder picker.
- `src/backend.rs`: URL normalization, metadata and format parsing, yt-dlp invocation, progress, cancellation.
- `src/session.rs`: explicit session import, YouTube-domain filtering, disposable cookie working files.
- `src/export_help.rs`: Windows Downloads location and file-picker folder selection.
- `Setup tools.ps1`: verified installation and update of external tools.
- `build.ps1`: tests, release build, portable packaging.

Downloads execute tools directly using argument arrays, without interpolating video links into a shell. User yt-dlp configs are ignored to keep behavior predictable. Cookies are read only from an export the user explicitly chooses and passed to yt-dlp for its YouTube requests. No passwords, analytics, or download history are collected.

Download videos you own or have permission to save. See `THIRD-PARTY.md` for component licenses and upstream sources.
