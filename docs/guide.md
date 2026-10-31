# stream-dlp user guide

stream-dlp downloads YouTube videos, with experimental TikTok support, makes GIFs from local videos, and hands videos to Streamable. Windows is the established release; the Apple Silicon Mac version is experimental (see [Mac](#mac)). This guide describes Windows unless otherwise stated.

## Install

Download `stream-dlp-Setup-Windows-x64.exe` from [Releases](https://github.com/Puffinnois/stream-dlp/releases). It includes the download tools, installs for the current user without administrator access, adds a Start menu shortcut, and offers an optional desktop shortcut. Uninstall through Windows Settings. The portable ZIP is the alternative: extract it into a writable folder and install the download tools from Settings (see [Download tools](#download-tools)).

## Download a video

Open stream-dlp. Paste a YouTube video URL and click **Find video** (or press Enter). Confirm the title and creator, select a resolution and container, then click **Choose folder & download**. The native Windows folder picker asks where to save the file. A prominent progress bar shows the current stream's percentage directly inside the bar. Click **Open folder** when the download finishes.

The format list shows every non-DRM video and audio stream returned by YouTube, including codec, frame rate, format ID, language, and stream size when available. Video-only choices automatically download matching audio and merge it without re-encoding, so they do not display a missing-audio warning. If matching audio cannot be obtained, the download reports an error instead of silently saving a video without sound. MP4 prefers M4A audio; WebM prefers WebM audio. FFmpeg may use MKV when the available streams cannot share the selected container. Stream size estimates exclude added audio. Progress describes the current stream; audio download and final merging follow. Audio choices save the original audio format without conversion.

Cancel stops the active process tree. Incomplete `.part` files are retained so a later attempt in the same folder can resume. Existing complete files are not overwritten. Closing the app signals cancellation. Regular videos and Shorts use the same downloader. Active live streams and DRM-protected formats are not supported. YouTube can change availability; update the download tools if extraction stops working.

Missing video details are shown explicitly: an absent title falls back to the
video ID or **Untitled video**, and missing creator, duration, resolution, bitrate,
container, and size use unknown labels. Unknown duration and resolution are not
shown as zero. Estimated stream sizes keep a `~` prefix; files smaller than 1 MB
show `<1 MB`.

## TikTok links (experimental)

Paste a full public `www.tiktok.com/@creator/video/<id>` or
`www.tiktok.com/share/video/<id>` link, then use **Find video** and the existing
download workflow. TikTok currently offers H.264/H.265 video formats containing
audio. Portrait dimensions appear directly in the quality list. TikTok filenames
have a source prefix, and YouTube sessions are never attached to TikTok requests.

Short/mobile links, photo posts, profiles, collections, live streams, authenticated
access, silent clips, and video-only formats are not supported in this first
implementation. For a short link, open it in your browser and copy the full video
URL. Website restrictions can still prevent a public video from downloading.
See [TikTok validation and remaining work](research/tiktok-feasibility.md).

## When YouTube asks you to verify

Click **Connect browser…** below the video link to expand the inline connection panel, or **Connect browser / retry help** in an error message. Choose **Firefox · automatic**, choose your profile (the default is selected for you), and click **Import from Firefox & retry**. No cookie exporter or file selection is needed. Successful import collapses the panel into **Session loaded · Manage**; connection errors stay inline. The app imports YouTube cookies in the background and retries video detection or the failed download once, keeping its quality and destination. **Auto mode** is on by default: after connecting, an authentication error refreshes only that same Firefox profile and retries once per attempt. Uncheck it to refresh only when you click **Refresh from Firefox & retry**. Browser/profile choices and cookies last for the current app session.

Firefox automatic import is experimental and supports normal, unpartitioned profile cookies on Windows, with database schemas through 17. Private windows and Firefox containers need manual export. The importer opens the selected database read-only through Windows' SQLite component, selects only `youtube.com` and its subdomains, and skips expired cookies. It does not copy the whole browser database or create an all-site cookie export. Import runs in the background with cancellation and a query time budget. If the profile is busy or inaccessible, close Firefox and retry. A successful import means cookies were read, not that YouTube has accepted the sign-in.

Chrome and Edge currently offer the guided export path. For those browsers, or if Firefox import fails, choose **Import a cookies file…** to import your own Netscape-format `cookies.txt` export from a signed-in YouTube session. The session applies to both video detection and downloading.

Need help making the file? Choose **Export guide**. The three-step guide offers Chrome/Edge and Firefox exporter links, explains the private-window YouTube export, and suggests saving `youtube-cookies.txt` in your Windows Downloads folder under `stream-dlp`. **Open export folder** creates and opens that folder; **Copy folder path** lets you paste its location into a Save As dialog. The folder is located using Windows' Downloads setting, including redirected Downloads folders.

The final guide step opens the file picker in the suggested folder (or Downloads if the subfolder has not been created). **Look in Downloads instead** handles browsers that save exports automatically. The guide also explains using the browser's Ctrl+J downloads list and Show in folder to locate exports such as `cookies (1).txt`. You can browse to any other folder. Subsequent imports start in the last chosen folder for the current app session, and a loaded session shows the selected file path. JSON, HTML, and exports without YouTube cookies receive an explanation rather than being silently accepted.

The inline connection panel links to the [official YouTube cookie export instructions](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies). Complete sign-in and any verification yourself in your browser, export only `youtube.com` cookies, and import the export. JSON exports are not supported. Cookie files act like a sign-in: keep them private. The upstream project also notes that account use can trigger restrictions, so use a session only when needed.

The imported session stays in memory until you click **Remove session** or close the app. stream-dlp keeps only YouTube-domain cookie entries, normalizes the file format, and supplies a separate temporary copy to yt-dlp for each operation. The copy is deleted when the operation finishes, fails, or is cancelled. The original export is not changed, and the session is not saved in the portable app folder or included in its ZIP.

Private, age-restricted, or members-only videos require an account with access, and even a signed-in session can be expired, rate-limited, or challenged by YouTube. Firefox cookies are read only after you explicitly connect a profile; automatic refresh is limited to that source. Complete any sign-in or verification yourself in the browser.

## App updates

The app checks GitHub for a newer stable release when it opens and every hour while it stays open; it never interrupts you with a popup. An installed copy then downloads the small update installer in the background (`stream-dlp-Update-Windows-x64.exe`: the app without the tools, about 4 MB) and the sidebar shows **Update to X**. It opens the release notes and the download progress. **Update & restart** installs silently and reopens the app; it is available once the download is verified and nothing else is running. **Later**, Escape, or clicking outside closes the dialog. **Settings → Updates** has the same button and **Check for updates**.

A portable copy downloads the full installer only when you choose **Download setup**; **Open setup** then runs the setup wizard, where you choose the portable folder (or download a fresh ZIP from Releases instead). Failed checks retry at the next hourly check and keep a previously found update; a failed download is retried then too, or with **Retry download**. Network errors never block downloading videos. Mac updates are manual: the Mac version shows that a release exists and links to its page.

An update must be a published release (not a draft or prerelease) whose version tag is newer than the running app, with its installer at this repository's release URL and GitHub's SHA-256 digest. The download must match that digest before anything runs. Updates never downgrade. Every release still publishes the full installer, `stream-dlp-Setup-Windows-x64.exe`, as versions up to 0.3.9 update only through it. Before 0.3.9, a copy set to the former Dev channel sees only Dev prereleases, and there will be no more of them: choose **Public** in its Settings once.

## Download tools

The installer includes the tools. **Settings → Download tools → Update download tools** downloads only the tools with a new release, each checked against GitHub's SHA-256 digest; a new yt-dlp is the usual fix when a site stops working. The small distributable ZIP contains the app and setup script: extract it into a writable folder, then use **Install download tools** in Settings, or run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File ".\Setup tools.ps1"
```

Both download yt-dlp, FFmpeg/ffprobe, and Deno directly from their official GitHub releases and record them in `tools/versions.json`. Each binary archive is verified against its GitHub release SHA-256 digest before installation. Setup fails closed if the upstream digest is unavailable. It installs only into the app's `tools` directory, needs no administrator access, and does not modify PATH. Close the app before running the script; the Settings button waits until nothing is running. Internet access is needed for setup and YouTube access.

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
[automatic uploader implementation and tests](reference/automatic-streamable-upload.md)
for lifecycle details and validation limits.

## Mac

The first Mac target is Apple Silicon (M1 or newer), macOS 15 or newer. The Mac
package includes the download tools, uses manual cookie-file import and manual
app updates, and requires no paid Apple developer account. It is ad-hoc signed
and not notarized, so a browser-downloaded copy may require **Open Anyway** in
macOS Privacy & Security. Initial real-device acceptance is still pending.
See [Mac build, installation, automated checks, and acceptance checklist](macos.md).

## Privacy

Downloads execute tools directly using argument arrays, without interpolating video links into a shell. User yt-dlp configs are ignored to keep behavior predictable. Cookies are read only from an export or Firefox profile the user explicitly chooses and passed to yt-dlp for its YouTube requests. No passwords, analytics, or download history are collected.

Download videos you own or have permission to save. See `THIRD-PARTY.md` for component licenses and upstream sources.
