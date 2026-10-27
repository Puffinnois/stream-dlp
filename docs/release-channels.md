# Public and Dev builds

On Windows, open **Settings → Updates**, choose **Update channel**, and select Public
or Dev. The app checks that channel immediately; it never installs a build until
you choose **Install selected build…** and confirm **Download & open setup**.
Available builds appear in the sidebar without interrupting startup. Setup then
asks you to complete installation.
The sidebar identifies the build currently running, while the selector identifies
the channel you want to receive. Selecting a channel alone does not change the
running executable.

Public receives stable GitHub releases. Dev receives GitHub prereleases whose
version is `vX.Y.Z-dev.N` and every newer stable release, so a Dev install never
falls behind Public (from 0.3.9; earlier Dev builds only see prereleases). Dev
builds can contain unfinished features and are publicly downloadable in this
public repository. They are excluded from Public update checks and must never be
marked as GitHub's latest stable release.

Switching from Dev to Public offers the latest stable version even when its
version number is lower. Switching from Public to Dev offers only newer builds. Both use the same per-user installation and shortcuts;
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

Releases are built by the Release workflow, never uploaded by hand. On a clean
`main`, `cargo xtask release dev` (or an explicit `X.Y.Z-dev.N`) sets Cargo.toml
and Cargo.lock to the Dev version, commits, and tags it; `git push --follow-tags`
starts the build. The workflow uploads the installer, ZIP, and SHA256SUMS.txt to a
draft prerelease. Edit its notes and publish it; GitHub never marks a prerelease
as latest. Locally, `build.ps1` places a Dev build's installer, ZIP, and portable
staging folder under `dist/dev/X.Y.Z-dev.N/`, leaving stable artifacts intact.
Installer branding says stream-dlp Dev, but its installation identity and
destination match Public so switching replaces the app. Asset filenames stay
constant within each release.

When testing is approved, `cargo xtask release patch` (or `minor`) removes the
version suffix and tags the stable version, and the workflow makes a fresh
stable build. Promotion never relabels a Dev binary.
