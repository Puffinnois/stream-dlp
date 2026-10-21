# stream-dlp v0.3.4

This release adds experimental Firefox session import, a per-user Windows
installer, and opt-in installation of app updates discovered through GitHub.

## Changes

- Connect a selected Firefox profile, import only current YouTube cookies in the
  background, and retry the pending operation once. Optional automatic refresh
  stays limited to that connected profile and one retry per attempt.
- Keep Chrome/Edge and private/container sessions on the guided manual-export
  path. Imported cookies and browser choices remain in memory for this app run.
- Install with a bundled-tools Windows x64 installer, optional desktop shortcut,
  and embedded app icon. The portable ZIP remains available with verified helper
  setup for users who prefer it.
- Check stable GitHub releases in the background and from About & tools.
  Download an installer only after clicking Update; verify GitHub's SHA-256
  digest before opening setup. Network failures leave video downloading usable.
- Record the R-02 Streamable feasibility decision: browser-assisted sharing is
  the planned integration. Authorized guest uploads processed and played.
  Streamable sharing is not yet an app feature; its documented API excludes
  direct upload.

## Validation on October 21, 2026

- `cargo test --locked`: 19 passed, four opt-in tests skipped.
- `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings`:
  passed.
- Local production download/merge/cancel test: passed with localhost access.
- Updater transfer test: passed for valid bytes, checksum mismatch rejection,
  and temporary-file cleanup, with localhost access.
- Windows release build, portable ZIP, and Inno Setup installer compilation:
  passed. Executable file/product version is 0.3.4.

## Known validation limits

Firefox automatic import is experimental. Scoped extraction and the connection
dialog were previously tested locally, but a full authenticated YouTube download,
clean-machine acceptance, and the Windows 10/11 browser matrix remain open.
Anonymous live YouTube extraction can require verification. Private windows,
Firefox containers, and Chrome/Edge automatic extraction are not supported.
Installer upgrade/uninstall and an actual published-release update round trip
still require acceptance testing. Checksums establish release-asset integrity;
the installer is not Authenticode-signed.

See the linked research records for detailed evidence and remaining cases.
