# stream-dlp v0.3.6

Create GIFs locally in a dedicated GIF converter page, or choose Make GIF after
finishing a video download.

- Trim clips from 0.1 to 60 seconds, with Small file, Balanced, and High quality presets.
- Adjust dimensions, frame rate, palette colors, dithering, and looping.
- Preserve aspect ratio without upscaling, using optimized two-pass FFmpeg palette encoding.
- Convert in the background with progress, cancellation, bounded worker threads,
  temporary-file cleanup, and protection against overwriting existing files.
- Preview the finished GIF in your default viewer or open its output folder.

The Windows installer includes the required helpers. The portable ZIP includes
an app and helper setup script. Existing Public installations can find this
release through About & tools. GIFs have no audio.

Validation: 33 enabled Rust tests pass, including actual FFmpeg conversion,
clip validation, cancellation, cleanup, and existing-file protection. Formatting
and Clippy checks pass. Native UI visual acceptance has not been performed for
this release. Seven tests requiring external services or explicit access remain
ignored. No Mac package is included.
