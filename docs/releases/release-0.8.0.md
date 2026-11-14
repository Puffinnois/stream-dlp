# stream-dlp 0.8.0

- **Other sites (experimental).** Paste a public link from a site other than YouTube, TikTok, X, Facebook or Instagram and stream-dlp tries the download tools' other site readers and their fallback page reader. Compatibility varies by site. Signing in is not available for other sites, links to local or private network addresses are refused, and a page with several videos lists the first 25 for you to choose one.
- **Recent downloads (optional).** Turn on "Keep a list of recent downloads" in Settings to see your last 50 downloads on My videos. It is off by default, stays on your computer, stores only the file, title, platform and time, and is erased when you turn it off.
- **Streamable link.** After handing a video to Streamable in your browser, you can paste the link back to open or copy it. The app does not check whether the upload finished.
- **Videos without sound.** A source that has no audio is now labelled and saved as video only instead of failing. A download that should have had sound but ended without it is reported as a failure.
- **Session link.** The link to connect a session names the platform of the pasted link, and is hidden when no supported platform matches.
- **Mac: optional usage reports.** On first launch the Mac version asks whether to share counts of how the app is working. Nothing is sent before a yes, reports contain fixed words and counts, never links, titles or file names, and Settings turns them off. Windows never asks and never sends.
- Session files left in the temporary folder by a crash are removed at the next start once they are a day old.
- Inspection output is capped for every site, and the recent-downloads list checks its files in the background.
