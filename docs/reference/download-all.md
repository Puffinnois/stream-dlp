# Download all — reference

Saving every video of a multi-video post, only as an explicit choice beside choosing one (F-04). The code is `src/app/batch.rs`: the `Batch` state machine and its part of the Download page. The engine is unchanged: each video goes through `engine::download`, as a single download does.

## When it is offered

Inspection returns `Event::Entries` for a post or page with several videos (X, Instagram carousels, Facebook, Other sites). `Batch::new` takes exactly those entries, the ones the chooser lists, and is `None` for fewer than two or more than 25. Inspection already caps Other sites at 25 and skips entries that are not a downloadable video; a named platform's post with more than 25 entries is offered no batch. Nested collections are never opened, and yt-dlp is never asked for a whole collection in one run.

The offer sits under the chooser's list: a quality row and **Download all N videos**. Pressing it opens the folder picker (`State::Picking`); cancelling the picker returns to `Ready`. Nothing starts before a folder is chosen.

## Running

- One entry at a time: the next starts only after the previous one's `Complete`, `Failed` or `Cancelled` arrives (`Batch::ended`). Results keep the entries' order.
- Each entry is a normal single download: its `MediaIdentity` (playlist position and ID), the format `Quality::pick` chose, the batch folder, and the session for that entry's own site from the app's session map, looked up when it starts. Every per-download rule applies unchanged: `--playlist-items` and `--match-filter`, Other sites' destination recheck and guard, the sound decision and lost-sound check, `--no-overwrites`, the file name template.
- Quality: `Best` is the format a single download selects first (the formats are sorted best video first). `AtMost(n)` is the best video format whose shorter side is at most `n` lines; without one, the entry's smallest video format; when no video format reports a resolution, its first format. The choices are Best, 1080p, 720p and 480p, made once before the batch starts.
- Cancel and closing the app set the batch's cancel flag (the app's `download_cancel` is that flag while a batch runs): the running download stops, the files already saved stay, and no later entry starts, also when the flag is set just as an entry completes.
- A failed entry is recorded with `EngineError::user_message` and the batch goes on. An entry whose file is the same file an earlier entry of the batch saved (two entries with the same file name; yt-dlp reports the existing file instead of replacing it) is a failure, not a second save.
- Automatic session refresh after a sign-in error does not run during a batch; the entry fails with its usual message, and **Connect session / retry help** is offered when every entry failed.

## The end

`State::Done` when every entry ended, `State::Cancelled` after a cancel. The card lists each entry as saved (path, **Make GIF**, **Upload on Streamable**), failed (its message) or cancelled, says how many were not started after a cancel, and offers **Open folder** and **Back to the videos in this post** (a new `Ready` batch with the same quality). A batch that ended with nothing saved and was not cancelled is a failure: the page shows "None of the N videos could be saved." with the first failure's kind for feedback.

## Privacy

- Recent downloads records each saved entry as it is saved, with the entry's title, as for a single download ([recent downloads](download-history.md)).
- Usage reports count each entry's end as the `download` outcome it is (`ok`, `failed` with its kind, `cancelled`), with the site's fixed word. Nothing identifies the post, and no event or count was added.

## Tests

Deterministic tests in `src/app/batch.rs` use a stand-in download: order, a failure in the middle, every entry failed, cancel during an entry and as one completes, per-entry quality fallback, a second entry reporting the first one's file, the 25-entry limit, a silent entry beside one with sound, per-site sessions, recent-download records and drawing. The local fixture pipeline (`other_sites_local_fixtures_inspect_and_download`, in `cargo xtask pipeline`) saves a page with two videos through the real helpers and the destination guard, then saves it again into the same folder and checks that neither file was replaced. Live multi-video posts on X, Instagram and Facebook are not verified.
