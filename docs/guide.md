# stream-dlp user guide

stream-dlp downloads YouTube videos, with experimental TikTok, X/Twitter, Facebook and Instagram support and an experimental mode for other sites, makes GIFs from local videos, and hands videos to Streamable. Windows is the established release; the Apple Silicon Mac version is experimental (see [Mac](#mac)). This guide describes Windows unless otherwise stated.

## Install

Download `stream-dlp-Setup-Windows-x64.exe` from [Releases](https://github.com/Puffinnois/stream-dlp/releases). It includes the download tools, installs for the current user without administrator access, adds a Start menu shortcut, and offers an optional desktop shortcut. Uninstall through Windows Settings. The portable ZIP is the alternative: extract it into a writable folder and install the download tools from Settings (see [Download tools](#download-tools)).

## Download a video

Open stream-dlp. Paste a YouTube video URL and click **Find video** (or press Enter). Confirm the title and creator, select a resolution and container, then click **Choose folder & download**. The native Windows folder picker asks where to save the file. A progress bar shows the current stream's percentage beside it. Click **Open folder** when the download finishes.

The format list shows every non-DRM video and audio stream returned by YouTube, including codec, frame rate, format ID, language, and stream size when available. Video-only choices automatically download matching audio and merge it without re-encoding, so they do not display a missing-audio warning. If matching audio cannot be obtained or combined, or a format that lists sound is saved without it, the download fails with a message saying the sound was lost instead of silently saving a video without sound; if a copy without sound was left in the folder, delete it before retrying. A video whose source has no sound at all (every stream reports no audio, on any site) is labelled **This video has no sound** before you download, offers no audio choices, and is saved as video only. A direct file whose site does not report its streams is saved as it is. MP4 prefers M4A audio; WebM prefers WebM audio. FFmpeg may use MKV when the available streams cannot share the selected container. Stream size estimates exclude added audio. Progress describes the current stream; audio download and final merging follow. Audio choices save the original audio format without conversion.

Cancel stops the active process tree. Incomplete `.part` files are retained so a later attempt in the same folder can resume. Existing complete files are not overwritten. Closing the app signals cancellation. Regular videos and Shorts use the same downloader. Active live streams and DRM-protected formats are not supported. YouTube can change availability; update the download tools if extraction stops working.

Missing video details are shown explicitly: an absent title falls back to the
video ID or **Untitled video**, and missing creator, duration, resolution, bitrate,
container, and size use unknown labels. Unknown duration and resolution are not
shown as zero. Estimated stream sizes keep a `~` prefix; files smaller than 1 MB
show `<1 MB`.

### Posts with several videos

When a post holds several videos (an X post, an Instagram carousel, a Facebook post or a page under other sites), stream-dlp lists them. Choose one to pick its quality as usual, or save them all: choose a quality under the list (**Best**, **1080p**, **720p** or **480p**) and click **Download all N videos**, then pick the folder. Nothing is downloaded until you click it. When a site answers that it is limiting requests, that video is tried once more after 15 seconds and the following videos each start after a 5-second pause; Cancel also stops the waiting.

The videos are saved one after another, never at the same time, each exactly as if you had chosen it on its own: the same session and site rules, sound check and file names, and existing files are never replaced. **Best** saves each video in the best quality it offers; a height saves each video in that quality or the closest one below it, or in its smallest quality when it offers none that small. A portrait video counts by its shorter side, so 1080 × 1920 is 1080p.

While it runs, the page shows which video is downloading (**Video 2 of 4**) with its progress. **Cancel** stops that video and the ones after it; the videos already saved are kept. A video that fails does not stop the others. At the end the page lists each saved file, with **Make GIF** and **Upload on Streamable** for it, and each video that failed with its reason; **Open folder** opens the folder. When none of the videos could be saved, the page shows an error instead. With **Recent downloads** turned on, each saved video is added to the list. Only the videos in the list can be saved together: never more than 25, and never a playlist, profile or other collection.

## X / Twitter posts (experimental)

Paste a public HTTPS post link from `x.com`, `twitter.com`, or
`mobile.twitter.com` and choose **Find video**. A copied post link can include
a share query. For a shortened link, open it in your browser first and copy
the full post address.

For a post with multiple videos, pick one video before choosing its MP4
quality and destination, or save them all (see
[Posts with several videos](#posts-with-several-videos)). **Choose another
video from this post** returns to the list. MP4 files retain their original audio when present. Downloads use
anonymous access by default. Photo-only posts, external embeds, live streams, and posts
that require a signed-in account are outside the supported scope.

## TikTok links (experimental)

Paste a full public `www.tiktok.com/@creator/video/<id>` or
`www.tiktok.com/share/video/<id>` link, then use **Find video** and the existing
download workflow. TikTok currently offers H.264/H.265 video formats containing
audio. Portrait dimensions appear directly in the quality list. TikTok filenames
have a source prefix, and YouTube sessions are never attached to TikTok requests.

For a short share link, the app tries to find the full post URL. If that fails,
open the link in your browser and copy the full video URL. Supported share-link
forms include `vm.tiktok.com`, `vt.tiktok.com`, and `tiktok.com/t/…`.

Photo slideshows and live links get separate messages. For a private post or
one that requires sign-in, choose a public video that plays while signed out.
If TikTok reports a video as unavailable or removed, check the link in your
browser. Profiles, collections, silent clips, and video-only formats are outside
the current scope.
See [TikTok validation and remaining work](research/tiktok-feasibility.md).

## Facebook (experimental)

Paste an individual public Reel, watch link, account video or video share link (`/share/r/` or `/share/v/`). Mobile watch links work too. If a share link fails, open it in your browser and paste the individual video link. A format with unknown resolution can still be selected. Feeds, groups, collections and live broadcasts are outside the supported scope.

## Instagram (experimental)

Paste a direct Reel (`/reel/…`) or feed post (`/p/…`). For a carousel, choose one video before choosing its quality, or save every video in it (see [Posts with several videos](#posts-with-several-videos)). Silent videos keep their original video stream. Photo-only posts, stories, profiles and share redirects are outside this workflow. If Instagram asks for sign-in, select **Connect Instagram session…**, import an Instagram cookie export, and retry. Anonymous access can vary between requests.

## Other sites (experimental)

Paste a public `http` or `https` link from any other site, either a page with a video or a video file, and choose **Find video**. The page shows **Other sites (experimental)** under the link. stream-dlp then tries the download tools' readers for hundreds of sites and their fallback page reader, which finds plain video files, videos embedded in a page, and HLS or DASH streams. This is an attempt, not a promise: many sites do not work, and a site that works today can stop working.

When a page holds several videos, stream-dlp lists the first 25: you choose one, or save the listed ones one after another (see [Posts with several videos](#posts-with-several-videos)). It never downloads a whole page or playlist. Live streams, audio-only results, DRM-protected videos and videos that need an account are refused with a message saying so.

Signing in is not available for other sites: no session, cookie export or Firefox import is used, even when you have connected one for another platform. Links to local or private network addresses (such as `localhost`, a router or a NAS) are refused, and so are links that redirect to one. The same check covers everything the download tools fetch for the page afterwards: a page or stream that loads its video, or part of it, from a local or private address is refused with a message saying so. A few unusual encrypted streams need a download method stream-dlp does not allow for other sites and are refused too. When your computer or its environment is set to use a proxy, the check covers only the pasted link and its redirects. A link that redirects to YouTube, TikTok, X, Facebook or Instagram is refused too; paste that platform's link itself. File names start with the site's name and address, followed by the title.

## Sessions for TikTok, X, Facebook and Instagram

Paste the post link, then choose **Connect TikTok session…**, **Connect X / Twitter session…**, **Connect Facebook session…** or **Connect Instagram session…**. The session link names the site of the pasted address, even a link that site's rules refuse (a TikTok profile still shows TikTok); with the field empty it offers YouTube, and it is hidden while the text is not a link or comes from another site. Import a Netscape-format cookie export for that platform. TikTok accepts cookies under `tiktok.com`. X accepts `x.com` and `twitter.com`. Facebook accepts `facebook.com` and its subdomains. Instagram accepts `instagram.com` and its subdomains. Each platform has its own in-memory session for inspection and downloading. TikTok and Facebook share links resolve anonymously. To disconnect one platform, paste one of its links, open **Manage**, and choose **Remove session**. Signed-in access to restricted posts is unverified. Links to [other sites](#other-sites-experimental) never use a session.

## When YouTube asks you to verify

Click **Connect YouTube session…** below the video link to expand the inline connection panel, or **Connect session / retry help** in an error message. Choose **Firefox · automatic**, choose your profile (the default is selected for you), and click **Import from Firefox & retry**. No cookie exporter or file selection is needed. Successful import collapses the panel into **Session loaded · Manage**; connection errors stay inline. The app imports YouTube cookies in the background and retries video detection or the failed download once, keeping its quality and destination. **Auto mode** is on by default: after connecting, an authentication error refreshes only that same Firefox profile and retries once per attempt. Uncheck it to refresh only when you click **Refresh from Firefox & retry**. Browser/profile choices and cookies last for the current app session.

Firefox automatic import is experimental and supports normal, unpartitioned profile cookies on Windows, with database schemas through 17. Private windows and Firefox containers need manual export. The importer opens the selected database read-only through Windows' SQLite component, selects only `youtube.com` and its subdomains, and skips expired cookies. It does not copy the whole browser database or create an all-site cookie export. Import runs in the background with cancellation and a query time budget. If the profile is busy or inaccessible, close Firefox and retry. A profile that is not signed in to YouTube is refused with a message asking you to sign in there first. A successful import means cookies were read, not that YouTube has accepted the sign-in.

Chrome and Edge currently offer the guided export path. For those browsers, or if Firefox import fails, choose **Import a cookies file…** to import your own Netscape-format `cookies.txt` export from a signed-in YouTube session. The session applies to both video detection and downloading.

Need help making the file? Choose **Export guide**. The three-step guide offers Chrome/Edge and Firefox exporter links, explains the private-window YouTube export, and suggests saving `youtube-cookies.txt` in your Windows Downloads folder under `stream-dlp`. **Open export folder** creates and opens that folder; **Copy folder path** lets you paste its location into a Save As dialog. The folder is located using Windows' Downloads setting, including redirected Downloads folders.

The final guide step opens the file picker in the suggested folder (or Downloads if the subfolder has not been created). **Look in Downloads instead** handles browsers that save exports automatically. The guide also explains using the browser's Ctrl+J downloads list and Show in folder to locate exports such as `cookies (1).txt`. You can browse to any other folder. Subsequent imports start in the last chosen folder for the current app session, and a loaded session shows the selected file path. JSON, HTML, and exports without YouTube cookies receive an explanation rather than being silently accepted.

The inline connection panel links to the [official YouTube cookie export instructions](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies). Complete sign-in and any verification yourself in your browser, export only `youtube.com` cookies, and import the export. JSON exports are not supported. Cookie files act like a sign-in: keep them private. The upstream project also notes that account use can trigger restrictions, so use a session only when needed.

The imported session stays in memory until you click **Remove session** or close the app. stream-dlp keeps only YouTube-domain cookie entries, normalizes the file format, and supplies a separate temporary copy to yt-dlp for each operation. The copy is deleted when the operation finishes, fails, or is cancelled. If the app crashes or is ended forcibly, the next start removes copies left behind once they are a day old. The original export is not changed, and the session is not saved in the portable app folder or included in its ZIP.

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
Previews load locally in the background.
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
current download or folder selection. Each video has a thumbnail: click it to play
the video right there, or click its name to select it and check its container, duration,
and size. **Open in player** opens it in your system player and **Show file** shows it
in its folder. **Refresh** reloads the folder after files change.
The list includes up to 1,000 video filenames in that folder, without scanning
subfolders. The folder selection lasts for this app session. **Choose existing
video…** also works without adding a folder.

### Play a video in the app

A finished download shows its picture on the **Download** page, and every video on
**My videos** has a thumbnail. Click one and it plays in place:

- Click the picture, press **Space** or use the button on the left to pause and play.
- Click or drag the position bar to move through the video; the picture follows
  while you drag. The **←** and **→** keys move 5 seconds.
- Drag the level next to the speaker for the volume, or click the speaker (or press
  **M**) to mute. The keys work while the pointer is over the player.
- The button at the far right, or **F**, or a double click on the picture, plays the
  video full screen: the window fills the screen with the picture on black. The
  controls lie over the bottom of the picture and hide, with the pointer, after a few
  seconds without movement; move the mouse to bring them back. They stay while the
  video is paused. All the buttons and keys keep working. **Esc**, **F**, a double
  click or the same button leave full screen, and the window and the page come back
  as they were. At the end of the video the last picture stays on screen.
- The button next to it opens the video in the app your computer plays videos with,
  for anything the app's player does not do. The app's player pauses and leaves full
  screen. A video that was moved or deleted says so instead.

After **Download all**, each saved video has its own thumbnail to play it. Only one
video plays at a time, and it pauses when you switch to another page.

Playing happens on your computer with the bundled FFmpeg; nothing is uploaded and
the file is never changed. The picture is shown at up to 960 pixels on its longer
side, whatever the file's resolution, so full screen enlarges it and a sharp video
looks softer there than in your system player. On a Mac the player has no sound yet: use the
system-player button to hear a video in your system player. A very large video
(4K at a high frame rate) may skip frames on a slow computer; its sound keeps playing.

### Recent downloads (optional)

The app can keep a short list of your recent downloads. It is off until you turn
on **Settings → Recent downloads → Keep a list of recent downloads**. From then on,
**My videos** shows a **Recent downloads** section with your last 50 downloads:
each one's title, platform, when it finished and its folder. Select one to work
with it just like a video you chose yourself: details, play, show file, GIF and
Streamable. A file you have moved or deleted stays listed, greyed out and marked
**File moved or deleted**, until you remove it.

The list holds only where each file was saved, its title, the platform and the
time. It never holds links, sign-in details or anything about the video's page,
and it is never sent anywhere: not with feedback, usage reports or anything else.
**Remove** takes one entry off the list and **Clear all** erases the whole list;
neither deletes a video. Turning the setting off erases the list too, after asking.
Deleting the app's data folder (see Privacy) also removes it.

After a download, **Upload on Streamable** selects the completed file in the same
interface. **Upload automatically (experimental)** opens a separate private
Edge/Chrome window, selects the file automatically, and returns a share link
after verifying playback. The app shows upload/processing status and offers
cancellation. Complete any sign-in or verification in that browser window.
Uploading is an explicit choice and accepts the linked Streamable upload terms.
Guest links may be publicly accessible and are temporary. **Use website manually**
remains available as a fallback. Neither flow changes the original file.

**Use website manually** opens Streamable in your browser and shows the file so
you can drag it into the page. If that fails or you cancel on the website, the
file and your download stay as they were. Afterwards, you can paste the link
Streamable gives you into **Paste the Streamable link here (optional)** and choose
**Use link**. The app accepts only links like `https://streamable.com/abc123`
(also without `https://` or with `www.`), then offers **Open** and **Copy**. It
cannot see the upload, so it shows this as the link you pasted, not as a finished
or checked upload. The link is kept only while that video stays selected in this
session: choosing another video forgets it, and it is never saved or sent anywhere.

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

Downloads execute tools directly using argument arrays, without interpolating video links into a shell. User yt-dlp configs are ignored to keep behavior predictable. Cookies are read only from an export or Firefox profile the user explicitly chooses and passed to yt-dlp only for requests to the platform they belong to. No passwords or download history are collected. The optional recent-downloads list stays on your computer, in `history.json` in the app's data folder (Windows: `%LOCALAPPDATA%\stream-dlp`; Mac: `~/Library/Application Support/stream-dlp`); deleting that folder removes it with the app's other settings.

On Windows, **Send feedback** is the one thing the app sends about itself. On Mac there is one more, only if you agree: **usage reports**. The first time the Mac version opens, it asks whether to send the developers short reports on how it is working. They carry the app and macOS versions, the chip generation (such as M1), whether the app and its tools started properly, the site and result or kind of error of each video lookup and download, which pages you open, whether the folder picker and cookie import worked, and where in the app's code a crash happened. They never carry links, video titles, file names, folders, cookies or error messages. Your IP address is only used in scrambled form, to limit abuse and roughly count how many people report each day. The developers see totals per day, kept for a year, and no report is stored on its own. **Settings → Usage reports** turns this off, deletes what is waiting, and shows what is sent.

**Send feedback** sends only when you press Send: your text, the tag, and the app version, operating system, page, site and kind of error, all of which **What else is sent** shows first. It sends no links, file names or screenshots, so leave out anything private when you write. Your IP address is only used in scrambled form, to limit abuse. The developers read each note; notes nobody handles are deleted after 90 days.

Download videos you own or have permission to save. See `THIRD-PARTY.md` for component licenses and upstream sources.
