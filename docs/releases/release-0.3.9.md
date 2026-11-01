# stream-dlp v0.3.9

TikTok videos, update checks while the app is open, and previews of your videos. This release also contains 0.3.8, which was never published.

## TikTok (experimental)

- Paste a public TikTok video link and use **Find video** and the download workflow as for YouTube.
- Full links work (`tiktok.com/@creator/video/…`, `tiktok.com/share/video/…`, `m.tiktok.com`), and short share links (`vm.tiktok.com`, `vt.tiktok.com`, `tiktok.com/t/…`) are resolved to the video first.
- TikTok offers H.264/H.265 video with audio; portrait sizes show in the quality list. A missing title, creator, duration or size is labelled instead of left blank.
- Filenames start with the source, and your YouTube session is never sent to TikTok.
- Not supported: photo posts, profiles, collections, live streams and private videos.

## Update checks

- The app checks for updates when it opens and every four hours while it stays open.
- A new version shows as a small dot and **Update available** in the sidebar, never as a popup at startup. A failed check is retried at the next interval and keeps an update it already found visible.

## Previews of your videos (from 0.3.8)

- The GIF converter shows a large frame of the source; move **Clip start** to see the moment you are converting.
- My videos shows thumbnails as you browse a folder, and a larger preview of the selected video. Full playback opens in your system video player.
- Previews load in the background and keep portrait and landscape shapes. If a frame cannot be read, a placeholder shows and you can still select and convert the video.

## Fixes

- The Dev channel also receives newer stable releases, and switching to Dev never offers an older build.
- A release tag that is not a version no longer breaks the update check.
- Installer folders left in your temp folder are removed after a day.
- The Firefox sign-in retry no longer depends on the wording of the message, and the message names the real button.
- Switching between Audio and Video moves a hidden selection to a format that is shown.
- A new video no longer shows the previous video's status line, and a cancelled lookup no longer mentions partial downloads.
- The retry help follows the link that failed, not what is in the link box.

## Downloads

The Windows installer includes the download tools. The portable ZIP includes the app and the tool setup script. `SHA256SUMS.txt` lists their checksums. Installed copies on the Public channel find this release through their update checker. A copy set to the **Dev** channel looks only for Dev prereleases until it runs 0.3.9: switch it to **Public** once in **Settings → Updates**. No Mac package is included.

## Validation

52 Rust tests and 6 Deno tests pass in the release build; 8 optional integration tests remain skipped. Formatting and Clippy pass, and the built app's tool check passes. A real public TikTok video passed download, full decode, no-overwrite and startup-cancellation checks. Short-link resolution is covered by automated tests, not yet by fresh real share links. 0.3.8's previews were covered by real FFmpeg tests (seeking, portrait framing, short clips, invalid files, cancellation).

**Full Changelog**: https://github.com/Puffinnois/stream-dlp/compare/v0.3.7...v0.3.9
