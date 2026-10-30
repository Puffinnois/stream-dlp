# stream-dlp v0.3.5-dev.1

Experimental Windows build for testing before the next Public release.

- Add Public / Dev selection in About & tools, a visible installed-build label,
  persistent preference, isolated release checks, and explicit installation of
  the selected channel, including rollback from Dev to Public.
- Add the My videos page for browsing a selected local video folder, inspecting
  video metadata, and choosing an existing video to share.
- Add experimental automatic Streamable uploads through an isolated Edge/Chrome
  window, with progress, cancellation, manual fallback, and playback verification
  before reporting success. Uploads require an explicit click and can be public.
- Keep experimental Apple Silicon Mac support on manual app updates; Mac device
  acceptance remains pending.

Public v0.3.4 remains the stable release. It predates the channel selector;
returning from it to Dev requires reopening this Dev installer until the next
Public release includes the selector. See release-channels.md for details.

The automatic uploader previously passed the authorized synthetic-video live
test, returning a playable Streamable link. Browser automation depends on the
current Streamable website and remains experimental.

## Validation on October 21, 2026

- Rust suite: 31 passed, seven opt-in tests skipped; formatting and Clippy passed.
- Live GitHub metadata checks passed for both Public and Dev using the production checker.
- Installer transfer fixtures passed for verified bytes and checksum mismatch rejection.
- Native Windows UI showed the installed Dev label, both channel options, the Public
  v0.3.4 rollback offer, and explicit installation control. Selection saved successfully.
- Windows release executable, portable ZIP, and bundled-tools Inno installer built successfully.
- Packaged executable passed `--check-tools`. No existing Public installation was replaced
  during validation; completing installer-based switching remains a user acceptance step.
- Mac build/device validation remains pending; its updates are manual.
