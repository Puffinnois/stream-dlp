# stream-dlp v0.3.8

Recognize your videos at a glance and choose GIF moments visually.

- GIF converter shows a large source-frame preview. Move Clip start to preview the moment you want to convert.
- My videos shows representative thumbnails as you browse a folder, plus a larger preview for the selected video.
- Full video playback opens in your system video player.
- Previews load locally in the background, with a bounded texture cache, visible-row loading, and debounced scrubbing. Portrait and landscape videos retain their display aspect ratio.
- Missing tools or unreadable frames show a fallback without blocking video selection or conversion.

## Downloads

The Windows installer includes the download tools. The portable ZIP includes the app and helper setup script. Existing installations can discover this Public release through their update checker. No Mac package is included.

## Validation

35 enabled Rust tests pass; seven optional integration tests remain skipped. Real FFmpeg tests cover seeking, portrait framing, short clips, invalid files, and cancellation. Cache tests cover stale results and memory bounds. Formatting and Clippy checks pass. Interactive desktop UI verification was not performed for this release.
