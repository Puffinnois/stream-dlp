# stream-dlp 0.10.0

- **Play videos in the app.** A finished download now shows its picture on the Download page: click it and it plays right there. On **My videos**, every video in your folder has a thumbnail that plays it at once, and after **Download all** each saved video has one too. The player has play and pause, a position bar you can click or drag, volume and mute, and the keyboard (Space, ← and →, M), plus a button to open the video in your system player. Playing happens on your computer; nothing is uploaded and the file is never changed. Tested on Windows with generated test videos (H.264, VP9 and AV1, with and without sound); real downloads have not been tried yet.
- On a Mac the player shows the picture without sound for now, and AV1 videos do not play in the app. Use the player's button to open the video in your system player.
- **My videos** loads its thumbnails faster, three at a time. **Play video** there is now **Open in player**, since a click on the picture plays the video in the app.
- **Download all**: a video the site refuses for too many requests is tried once more after a short wait, and the videos after it start a few seconds apart.
- A browser profile that is not signed in to YouTube is no longer accepted as a YouTube session; the app says to sign in first.
- Videos from sites that leave out whether a video is live (Dailymotion, for one) no longer fail with "Could not read the video details."

Tested with yt-dlp 2026.08.19.
