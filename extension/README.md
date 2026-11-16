# stream-dlp companion extension (experimental)

A Manifest V3 extension for Chrome and Edge that sends the YouTube cookies of the profile it is
loaded in to the stream-dlp app on the same computer, only when you press **Connect** in its
popup. It is not in a store; load it unpacked from this folder in a dedicated test profile.
The app side is visible only in Dev mode.

How it works, the trust boundary and the acceptance checklist:
[docs/reference/chrome-extension.md](../docs/reference/chrome-extension.md).

Tests: `deno test --no-config --no-remote --no-prompt extension/` (part of `cargo xtask check`).
