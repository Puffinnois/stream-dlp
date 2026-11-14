# Session working files

A connected session (Firefox import or a cookies.txt export) lives in memory as `SessionCookies` (`src/session/mod.rs`). It reaches the disk only while a helper needs it.

## What is written

- `SessionCookies::temporary_file` writes the filtered cookies to `stream-dlp-session-<random>.txt` in the system temporary folder (`std::env::temp_dir()`), one file per yt-dlp run (inspection or download). It is a `tempfile::NamedTempFile`, removed when the run ends, fails or is cancelled.
- This is the only session file the app writes. The Firefox import (`src/session/firefox.rs`) opens the profile's `cookies.sqlite` read-only in place and makes no copy.
- The TikTok share-link resolver's temporary script (`src/engine/sources/tiktok.rs`) holds no session data and does not use the session prefix, so the sweep below does not touch it.

## After a crash

A crash or a killed process skips the drop, so the file stays. At every start, `session::remove_stale_session_files` (`src/session/stale.rs`) runs on a background thread next to the stale-installer sweep and removes entries of the temporary folder that:

- start with `stream-dlp-session-` (exact case),
- are a plain file or folder, never a symbolic link, junction or other reparse point (folders are removed with `remove_dir_all`, which does not follow links inside), and
- were last modified more than 24 hours ago.

The installed app and a development build can run at the same time, so the age rule is what keeps the sweep away from a file another instance is using. Inspection stops after 150 seconds, but a download has no time limit, so the threshold is a day, as for stale update installers. A file whose download outlasts a day could be removed by another instance's start; yt-dlp has already read its cookies by then.

The sweep reads only names and modification times, never opens a file, ignores every error (a locked or vanished entry is skipped), and logs nothing. Normal shutdown is unchanged: the drops remove the files.

Tests (`session::stale::tests`) use synthetic files in their own temporary folder, including a check that `temporary_file` creates its file where the sweep looks and with its prefix.
