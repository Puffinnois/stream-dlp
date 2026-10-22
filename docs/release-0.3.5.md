# stream-dlp v0.3.5

This Windows release adds a My videos page, experimental automatic Streamable
uploads, and Public / Dev update channels.

- Browse a selected local video folder in My videos, inspect metadata, play
  videos, reveal files, and refresh the list. Folder selection lasts for the
  current app session; subfolders are not scanned.
- Upload an existing or freshly downloaded video to Streamable after an
  explicit click. A dedicated private Edge/Chrome window selects the file,
  follows upload and processing, and verifies playback before reporting success.
  Includes cancellation, copyable share links, and a manual fallback. Uploads
  can be public; website automation remains experimental.
- Choose Public or Dev in About & tools. The preference survives restarts and
  the sidebar identifies the installed build. Public receives stable releases;
  Dev receives only explicitly tagged Dev prereleases. Installation requires a
  click, validates the official installer URL and GitHub SHA-256 digest, and
  supports switching back to an older Public build.
- Preserve the existing YouTube downloader and browser-session import flow.

## Installation and channels

The Windows installer includes download helpers. The portable ZIP includes
the app and helper setup script. Both channels replace the same installation;
videos remain in their existing folders. Switching to Public v0.3.4 removes
the selector because that older build predates it. Both directions are available
inside v0.3.5 and newer builds that include the selector.

Dev v0.3.5-dev.1 remains available as a prerelease. This stable release is a
fresh build with the Dev suffix removed, rather than a relabeled Dev binary.

## Validation

- Windows user acceptance confirmed on October 22, 2026, including installing
  between Public and Dev and uploading representative videos without blocking issues.
- Dev validation passed 31 regular Rust tests, formatting, Clippy, live GitHub
  metadata checks, verified-transfer and bad-checksum fixtures, native UI checks,
  package compilation, and bundled-helper checks.
- The experimental uploader also passed a synthetic-video live upload and
  local browser fixtures. Large-file, account, and challenge handling should
  continue to be treated as experimental rather than universal compatibility.
- Stable binaries are rebuilt and checked before publishing their installer,
  ZIP, and checksums. No Mac asset is included; Mac real-device acceptance
  remains pending and Mac updates remain manual.
