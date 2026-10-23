# Public and Dev builds

On Windows, open **Settings → Updates**, choose **Update channel**, and select Public
or Dev. The app checks that channel immediately; it never installs a build until
you choose **Install selected build…** and confirm **Download & open setup**.
Available builds appear in the sidebar without interrupting startup. Setup then
asks you to complete installation.
The sidebar identifies the build currently running, while the selector identifies
the channel you want to receive. Selecting a channel alone does not change the
running executable.

Public receives stable GitHub releases. Dev receives only GitHub prereleases
whose version is `vX.Y.Z-dev.N`. Dev builds can contain unfinished features and
are publicly downloadable in this public repository. They are excluded from
Public update checks and must never be marked as GitHub's latest stable release.

Switching from Dev to Public offers the latest stable version even when its
version number is lower. Both use the same per-user installation and shortcuts;
install into your existing app folder to replace that copy. Videos are separate
files and are not moved. The selection is saved in the user's local app-data
directory as `stream-dlp/update-channel.json`, shared by installed and portable
copies. If missing or unreadable, a Dev build defaults to Dev and a stable build
defaults to Public. A failed preference write leaves the current selection intact.

The Windows updater accepts only this repository's exact installer asset URL
and requires GitHub's SHA-256 digest. Background checks from a previous selection
cannot offer the wrong channel. Switching is disabled during active app operations.
Dev discovery examines up to the most recent 500 releases and selects the highest
eligible semantic version; same-channel checks never downgrade.

## First installation

Public v0.3.5 and Dev v0.3.5-dev.1 include the channel selector. Install either
to start switching inside the app. Public v0.3.4 has no selector because it was
built before this feature. If you return to v0.3.4, reopen a newer installer
to regain the selector.

Mac builds retain manual installation: use the selected channel's package on
GitHub Releases. An automatic Mac installer is not implemented.

## Build and promotion

Set Cargo.toml and Cargo.lock to `X.Y.Z-dev.N` for a Dev build. `build.ps1`
places its Windows installer, ZIP, and portable staging folder under
`dist/dev/X.Y.Z-dev.N/`, leaving stable artifacts intact. Installer branding says
stream-dlp Dev, but its installation identity and destination match Public so
switching replaces the app. Asset filenames stay constant within each release.

After tests, upload the installer, ZIP, and SHA256SUMS.txt to a draft GitHub
prerelease with matching `vX.Y.Z-dev.N` tag. Verify asset sizes and server digests
before publishing it as a prerelease with latest disabled. When testing is
approved, remove the version suffix, rebuild and validate, then create a stable
release. Promotion requires a fresh stable build; do not relabel a Dev binary.
