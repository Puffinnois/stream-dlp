# R-03 / P-02 — X and Twitter feasibility

On November 2, 2026, the bundled Windows yt-dlp 2026.08.19 inspected a
public single-video post and a two-video post without cookies. P-02 can target
public native post videos once F-02 lets the user choose an entry. These
probes establish metadata extraction. Download, decode and app acceptance
are still open.

## Anonymous probes

Cases came from the [upstream Twitter extractor tests](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/twitter.py)
plus a synthetic unavailable status. Each run used `--ignore-config`,
`--no-cache-dir`, `--skip-download` and `--dump-single-json`, without a
playlist override or a session. The results below group retries and URL
variants by case.

- **Single video.** The same status on `x.com`, `twitter.com` and
  `mobile.twitter.com` returned one Twitter video with nine formats, a title
  and a duration. The X link with a share query (`?s=20`) did too. Two older
  single-video fixtures returned unavailable errors across all four variants.
- **Multiple videos.** A legacy-domain status returned a playlist of two
  Twitter videos. Each entry had five formats, a title and a duration.
- **Short shares.** One `t.co` link resolved to a two-entry Twitter playlist
  with five formats per entry. Another short link failed with an unavailable
  error. A short URL alone does not identify an eligible destination.
- **Restricted or missing.** Two upstream fixtures marked as requiring
  authentication and a synthetic unavailable status all failed with the
  same no-video diagnostic. Anonymous inspection could not establish the
  current visibility of either fixture or distinguish the reason for failure.

## First-release scope

Accept HTTPS individual post paths of the form
`/<account>/status/<numeric-id>` on the two desktop domains and the tested
legacy mobile host. Accept the copied-link query form. Defer short redirects
until a bounded resolver verifies an allowed final post URL. Exclude profiles,
feeds, Spaces, live streams and external embeds.

Inspect the whole post before choosing a download. For a playlist, show
F-02's entry chooser and download only the selected video. Treat title and
duration as optional even though the successful cases provided both.
Keep X requests anonymous. Reserve sign-in advice for explicit authentication
errors and report an ambiguous no-video response without diagnosing privacy
or deletion. X sessions wait for F-03.

## Maintainer samples, November 3

The bundled helper inspected one public post with eleven formats. A direct MP4
download passed full H.264 video and AAC audio decoding. A second post returned
the ambiguous no-video diagnostic in both default and syndication modes. Its
public embed metadata listed only a photo.

The production backend subsequently passed both cases. It saved and decoded
the video and rejected the photo-only post. On a separate public two-video
post it saved exactly one MP4 when entry two was selected. The saved media ID
matched that entry and FFmpeg decoded it. A historical animated-media fixture
was unavailable during this run.

Later on November 3, X's public embed metadata classified a separate public
post as `animated_gif`. The v0.5.0 production backend inspected the post and
saved exactly one MP4. FFprobe counted multiple frames in its sole video
stream and found no audio stream. FFmpeg decoded the entire file successfully.

A historical native-MP4 fixture also passed the live unavailable-media check.
X's public embed endpoint returned HTTP 404, and the bundled Twitter extractor
reported no video. Production inspection returned the generic unsupported-post
error before format selection or download. The response did not establish why
the post was unavailable.

On November 3, a separate post produced an explicit authentication error
with the bundled extractor while signed out. The production inspection
returned `SignInNeeded(Source::X)` before format selection. The error did
not trigger YouTube session recovery. No media was downloaded.
