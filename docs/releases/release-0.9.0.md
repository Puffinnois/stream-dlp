# stream-dlp 0.9.0

- **Download all videos of a post.** When a post or page holds several videos, the list now also offers "Download all N videos" (2 to 25 videos). They are saved one after another in one quality you choose (Best, 1080p, 720p or 480p). A video that fails does not stop the others, Cancel keeps what is already saved, and the result lists each saved file and each failure. Choosing a single video works as before. Verified with test pages; real multi-video posts on each platform have not been tried yet.
- **Other sites: safer downloads.** Everything the download tools fetch for a page from another site, not only the pasted link, is now checked: a page or stream that loads its video from a local or private network address (such as a router or a NAS) is refused with a message saying so. Streams in the DASH format were tested and work. A few unusual encrypted streams that need a download method stream-dlp does not allow for other sites are refused. When your computer is set to use a proxy, only the pasted link and its redirects are checked.
- Dev mode shows an early prototype for importing a YouTube session from Chrome or Edge through a companion extension. It is not ready for general use and has not been tested in a real browser yet; the guided export for Chrome and Edge is unchanged.
- The Mac disk image step of the build retries when the system is busy.

Tested with yt-dlp 2026.08.19.
