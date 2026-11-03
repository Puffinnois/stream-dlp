# R-03 platform feasibility decision, November 3, 2026

## Method

On Windows, the bundled yt-dlp 2026.08.19 and candidate 2026.10.26 each
inspected the same 13 inputs. The candidate executable matched the SHA-256
in the [upstream release](https://github.com/yt-dlp/yt-dlp/releases/tag/2026.10.26).
Each invocation used `--ignore-config --no-plugin-dirs --no-cache-dir
--skip-download --socket-timeout 10 --retries 0 --extractor-retries 0 -J`
without cookies or a playlist override. Counts below are per helper. Both
versions produced the same outcome for each input. These probes inspected
metadata only.

## Instagram (P-03)

Defer Instagram support. A video post (`/p/<code>/`), a Reel
(`/reel/<code>/`) and a post whose upstream test expects multiple videos
all redirected to login. The helper attributed the redirect to anonymous
rate limiting. None returned entries, formats or metadata. The responses
leave both post visibility and the effectiveness of cookies unknown.
[Upstream Instagram fixtures](https://github.com/yt-dlp/yt-dlp/blob/2026.10.26/yt_dlp/extractor/instagram.py)

Reopen with a fresh permitted Reel and video post. Compare anonymous
inspection with a separately authorized, Instagram-only session if needed.
Before accepting carousels, verify a mixed post yields selectable video
entries and a photo-only post yields a clear outcome. Share redirects,
stories, profiles and feeds are outside the initial scope.

## Facebook (P-04)

Target individual public Reels first. The `/reel/<id>` fixture and its
`/watch/?v=<id>` equivalents on the desktop and mobile hosts returned the
same media ID, five formats, a title, uploader and duration without cookies.
Three formats omitted height. Preserve the watch query when routing.
[Upstream Facebook fixtures](https://github.com/yt-dlp/yt-dlp/blob/2026.10.26/yt_dlp/extractor/facebook.py)

A different watch fixture and a desktop `/<account>/videos/<id>/` fixture
both failed to parse. A mobile `/story.php` fixture produced an explicit
registered-user error. A synthetic invalid watch ID returned HTTP 404.
Keep these failure reasons distinct. Authenticated recovery is untested.

Gate implementation on download and decode of a permitted Reel and test a
second playable watch video. Defer share redirects until a fresh share
resolves to a verified individual video. Collections, feeds, groups and live
streams are outside the initial scope. No Facebook probe returned a collection.

## TikTok (P-01) and X (P-02)

Retain the P-01 scope of individual videos and verified short links. On this
network the historical direct video fixture produced an explicit IP block and
the short-link fixture ended at an unsupported URL with both helpers. Fresh
short links previously passed production validation as recorded under P-01.
[TikTok fixtures](https://github.com/yt-dlp/yt-dlp/blob/2026.10.26/yt_dlp/extractor/tiktok.py)
and [earlier probe results](tiktok-feasibility.md).

Retain the P-02 scope of public native X post videos. One direct status
returned a single video with nine formats, title, uploader and duration
without cookies on both versions. The existing v0.6.0 acceptance record
covers selected playlist entries, silent animation and photo-only,
unavailable and login-required posts. Restricted downloads with an imported
session remain unverified. [X fixtures](https://github.com/yt-dlp/yt-dlp/blob/2026.10.26/yt_dlp/extractor/twitter.py)
and [release verification](x-feasibility.md).
