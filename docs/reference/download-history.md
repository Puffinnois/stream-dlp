# Recent downloads — reference

An optional list of finished downloads on the **My videos** page (S-01). The code is `src/history.rs` (file and rules, no egui) and `src/app/recent.rs` (the section and the setting).

## The setting

`download_history` in `settings.json`, off unless the person turns on **Settings → Recent downloads → Keep a list of recent downloads**. While it is off nothing is recorded and no list exists: one found at launch with the setting off is deleted. Turning it off asks first, then deletes `history.json`; if that fails the setting stays on and the error is shown, so it can be tried again. The videos are never touched.

## The file

`history.json` in the app data folder (`%LOCALAPPDATA%\stream-dlp`, Mac `~/Library/Application Support/stream-dlp`):

```json
{
  "entries": [
    {
      "path": "C:\\Videos\\Example.mp4",
      "title": "Example title",
      "platform": "youtube",
      "completed": 1794614400
    }
  ]
}
```

- Exactly four fields per entry: the final file path, the title yt-dlp reported (the file name is shown when it is empty), the platform as the same fixed word public feedback uses (`youtube`, `tiktok`, `x`, `facebook`, `instagram`, `other`), and the completion time in seconds since the Unix epoch. No link, media or extractor ID, host, format, cookie or credential.
- Newest first, at most 50 entries (the oldest go), one per path: downloading to the same path again moves it to the top.
- Written whole through a temporary file in the same folder and a rename (`dev::write_atomically`).
- Read tolerantly: a missing, unreadable, larger than 1 MB or malformed file is an empty list; unknown fields are ignored; an entry that does not read, a repeated path or anything past 50 is left out. The next download replaces a damaged file.

## Recording

A download is recorded when it completes (`Event::Complete`), from the completed path, the selected video's title and the link's platform. A failed write shows a short message on the Download and My videos pages and in Settings, without the file's path; the download stays successful.

## The section

Shown on My videos only while the setting is on, between the folder card and the folder's videos. Each entry shows its title, platform, a relative or short local time and its folder, with **Remove**; the section has **Clear all**, which asks first. Selecting an entry selects the file exactly as **Choose existing video…** does. Whether each file still exists is checked when the page is opened, on **Refresh**, and after the list changes, never every frame and never on the UI thread: the list shows at once from the saved entries, each as present, and one background worker checks the files and wakes the page when it is done. While it runs, a further change waits for it, and a result for a list that has changed since is dropped, so a slow or unreachable drive delays only the marks and never freezes the page; a missing file stays listed, muted and marked "File moved or deleted", with only **Remove**.

## Privacy

The list never leaves the computer. Dev notes, public feedback, usage reports and the recent-errors log do not read it, and history errors carry no path or title; a test (`a_recorded_download_stays_out_of_feedback_usage_and_notes`) checks the feedback context, the usage counts and the dev-note context after a recorded download. Deleting the app data folder removes the list with the other settings.
