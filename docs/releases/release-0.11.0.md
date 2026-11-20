# stream-dlp 0.11.0

- **Fullscreen.** The player has a fullscreen button; F or a double click on the picture does the same. The video fills the screen on black, and the controls and the pointer hide after a moment without movement. Escape, F or the button brings the window back as it was. Tried on Windows on **My videos** with generated test videos; the Download page's player uses the same code but was not tried fullscreen.
- **The system-player button now always opens the file.** It opens the video with the app your computer uses for that kind of file, as a double click in Explorer does. Before, a video whose name had letters outside the system's language (Japanese, for example) opened nothing, without a message. A file that was moved or deleted, or a kind of file no app opens, now says so. The app's player pauses and leaves fullscreen when the other player opens. **Open in player** on My videos and the two buttons on the GIF page work the same way.
- **Sound on a Mac.** The player now plays sound on a Mac. This is new and has not been heard on a real Mac yet: if a video plays without sound there, use the player's button to hear it in your system player.
- Uninstalling stream-dlp on Windows now removes the registration of the experimental Chrome and Edge extension (Dev mode), unless it belongs to another copy of the app.

Tested with yt-dlp 2026.08.19.
