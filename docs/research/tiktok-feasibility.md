# R-03 / P-01: TikTok feasibility

Investigated October 24, 2026, on Windows. Scope: TikTok only. Status: anonymous
metadata feasibility established for three individual video URLs. A user-supplied
clip also passed helper-level download, full decode, and no-overwrite checks.
Direct-link application integration is now implemented experimentally. Interactive
player acceptance and broader compatibility checks remain open.

## Implementation follow-up

The development build accepts full individual TikTok video/share URLs and rejects
short/mobile links and unsupported content. The backend restricts extraction to
TikTok, rejects collections and unexpected extractor identities, filters the first
version to muxed H.264/H.265 video with audio, supplies missing-title fallbacks,
and displays portrait dimensions and unknown resolutions clearly. Output names
include a TikTok prefix. TikTok errors are source-specific and do not expose raw
signed URLs in the UI. YouTube cookies are excluded at the helper-command boundary;
TikTok failures cannot trigger YouTube automatic session refresh.

An opt-in production-backend test passed using the authorized sample: inspection,
download with a synthetic YouTube session present, full audio/video decoding,
byte-for-byte and modification-time no-overwrite checks, and cancellation during
helper startup. The local DASH download/merge/cancel regression also passed.
Cancellation during a long active transfer and close-window cleanup still merit
manual acceptance; startup cancellation does not prove every transfer phase.

```powershell
$env:STREAM_DLP_TIKTOK_TEST_URL = 'https://www.tiktok.com/@thegolivebutton/video/7622412096440929566'
cargo test --locked tiktok_authorized_production_pipeline -- --ignored --nocapture
```

This is a local development change, not a published release. The original spike
and its proposed broader implementation requirements are preserved below.

Windows UI verification loaded the user-supplied URL in the development build
and confirmed the TikTok source label, creator, duration, portrait format choices,
and updated input help. Download/decoding and cancellation were exercised through
the production backend test, not through the GUI's folder picker or player.

## Decision

Proceed with a narrow first implementation: public individual TikTok videos at
`https://www.tiktok.com/@creator/video/<numeric-id>` and
`https://www.tiktok.com/share/video/<numeric-id>`.
Do not advertise short links, mobile variants, authenticated videos, slideshows,
profiles, collections, or live streams until their corresponding acceptance checks
pass. The bundled helper is sufficient for the metadata cases tested; this spike
does not justify updating the production helper.

## Reproducible evidence

Added `scripts/tiktok-spike.ps1` (PowerShell 7). It runs helpers without user
configuration or browser cookies, requests metadata without media downloads,
limits each operation to 40 seconds, kills the process tree on timeout, and saves
only selected metadata and errors with URLs redacted. It does not retain media
URLs, headers, full descriptions, or cookie jars.

Compared the bundled yt-dlp **2026.08.19** with **2026.10.21**, the latest stable
release returned by GitHub during this investigation. The candidate was downloaded
separately under ignored `test-output/tiktok-spike/` and verified against the
release asset's SHA-256 digest:
`33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555`.
Production `tools/yt-dlp.exe` was not replaced.

Run from the repository root:

```powershell
./scripts/tiktok-spike.ps1 -YtDlp './tools/yt-dlp.exe','./test-output/tiktok-spike/yt-dlp-candidate.exe'
```

The candidate must already exist; the script intentionally does not download or
trust an arbitrary executable automatically. `-CaseName` selects named cases and
`-OutputDirectory` preserves separate runs. Each run replaces its own summary.

The initial six cases were run against both helpers, followed by four cases
(including a repeat of desktop) against both: **20 metadata probes total**.
Eight succeeded; twelve failed; none hit the process deadline. These are observed
results from this connection, not reliability rates. Compact evidence is retained
in [tiktok-probe-results.json](tiktok-probe-results.json).

### Successful cases on both versions

- `@patroxofficial/video/6742501081818877190?langCountry=en`: title and uploader
  present, 27-second duration, eight formats, portrait 576 × 1024 video, H.264/AAC
  MP4 and a separate M4A option. One video format lacked dimensions.
- `@hankgreen1/video/7047596209028074758`: title and uploader present, 21-second
  duration, ten formats including H.264/H.265 video and MP3 audio.
- `/share/video/7668090902816017671/`: title and uploader present, 30-second
  duration, nine formats including portrait 576 × 1024 and 720 × 1280 video.

The repeated desktop probe confirmed `is_live` is absent rather than explicitly
null; the application's existing serde default handles that field correctly.
Missing title/uploader behavior has not been established by these live samples.
Format listings establish availability of metadata, not successful media fetching
or playback. No media was downloaded in the initial metadata probes.

### Authorized download follow-up

The user supplied `https://www.tiktok.com/@thegolivebutton/video/7622412096440929566`
in response to the request for an owned/permitted sample. The bundled helper
2026.08.19 downloaded it anonymously without browser cookies or user configuration.
The selection preferred a muxed H.264 format with audio; the reported format was
`h264_540p_376470-1`. yt-dlp handled its website JavaScript challenge automatically.

- Saved under ignored `test-output/tiktok-spike/authorized-sample/7622412096440929566.mp4`.
- FFprobe: H.264 video at 540 × 958; AAC stereo audio at 44,100 Hz;
  duration 34.549002 seconds; size 1,625,882 bytes.
- FFmpeg decoded the entire audio and video streams with `-v error -xerror`
  to the null output, exiting successfully with no reported decoding errors.
- Repeating the download with `--no-overwrites` reported the file already
  downloaded; SHA-256 and modification time both remained unchanged.
- SHA-256: `44BF3C147F0DFB16FA542246812ECA4BFDEA2A048FC7060466A520C19B7422DC`.

This proves helper-level retrieval and decoding for this sample. It does not
establish interactive player behavior, app integration, cancellation, separate
audio merging, or compatibility across TikTok videos. No candidate-version media
download was needed because the bundled helper succeeded.

### Unsuccessful and inconclusive cases

- `/t/ZTRC5xgJp`: bundled helper encountered DNS failure; candidate followed a
  path that ended in an unsupported URL. Do not conclude all `/t/` links fail.
- `vm.tiktok.com/ZTR45GpSF/`: DNS resolution failed on both versions.
- `vt.tiktok.com/ZSe4FqkKd`: unsupported URL on both versions.
- Synthetic mobile variant `m.tiktok.com/v/6742501081818877190.html`: HTTP 403
  through the generic extractor on both versions. This is not a verified current
  mobile share link.
- Synthetic `/@patroxofficial/photo/6742501081818877190`: unsupported URL on both.
  This deliberately altered URL is not evidence about a real photo post.
- Synthetic `/@_/video/0`: bundled helper reported unavailable status 100002;
  candidate hit DNS failure. It does not prove the candidate regressed.

No browser profile was inspected. Login-required/private posts, signed-in access,
real photo posts, fresh short links, and real mobile shares remain untested.

## Upstream source findings

The [TikTok extractor](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/tiktok.py)
was inspected on the investigation date. It has separate video, short-link,
profile, collection, and live extractors. Short links use a HEAD request and then
delegate the destination URL. Therefore `--no-playlist` alone is not a suitable
application boundary. It also identifies watermarked formats, unplayable bytevc2
formats, and some video-only H.265 variants. Private-content and IP-blocked errors
are distinct; not every failure should suggest cookie import.

Release source: [yt-dlp 2026.10.21](https://github.com/yt-dlp/yt-dlp/releases/tag/2026.10.21).
Master source is mutable; the tested executable versions and observed results
above are the reproducible baseline.

## Required application changes

1. Introduce source identity for YouTube and TikTok. Keep existing YouTube URL
   regression behavior. Accept exact approved TikTok hosts and individual-video
   paths; reject credentials, custom ports, deceptive domains, profiles,
   collections, photo paths, and live paths with specific messages. Preserve
   query parameters unless proven unnecessary. Short-link resolution needs a
   bounded redirect policy and validation of the final video destination before
   enabling it; prevent fallback to unrelated generic sites or private hosts.
2. Carry source platform, media ID, and source URL through inspection and completed
   results. Check extractor identity and reject collections explicitly. Use title
   fallbacks and display unknown resolution without `0p`; show actual dimensions
   for portrait video. Reject unplayable codecs rather than sorting them first.
3. Keep YouTube cookies and Firefox authentication retries scoped to YouTube.
   `App::inspect` and `start_download` currently clone the active YouTube session
   unconditionally. TikTok must receive no session in the first implementation.
   Domain filtering in `SessionCookies` is not a substitute for this explicit
   operation boundary. TikTok cookie support requires separate policy and testing.
4. Reuse the process, progress, cancellation, and no-overwrite engine. Preserve
   source identity in output names so cross-platform IDs cannot collide. Verify
   muxed H.264/AAC, video-only variants requiring audio, and truly silent sources;
   do not silently accept failed audio retrieval. Some listed formats have no
   dimensions and may still fail when actually fetched.
5. Replace YouTube-only input, timeout, and failure messages where appropriate.
   Distinguish inaccessible/private media, IP/network failures, unavailable posts,
   unsupported content, and extraction errors. TikTok errors must not prompt
   Firefox YouTube session refresh. Preserve safe error detail without exposing
   signed URL parameters.

## Acceptance before declaring TikTok supported

- [x] Add routing and metadata fixtures for the supported URL shapes, deceptive
  domains, missing fields, portrait formats, collections, and codec filtering.
- [x] Verify TikTok commands cannot receive a loaded YouTube session and cannot
  trigger its automatic refresh/retry behavior.
- [ ] Use an owned/permitted sample for real download, ffprobe stream checks,
  playback, cancellation, and no-overwrite behavior through the production app.
  Helper-level download, stream inspection, full decode, and no-overwrite passed
  for the supplied sample above; application playback/cancellation remain open.
- [ ] Obtain fresh short/mobile links and check redirect destinations before
  deciding whether to expand the first-release scope.
- [ ] Test real unavailable/private/photo cases separately from synthetic URLs.
- [ ] Run Rust tests, Clippy, and the existing local download/merge/cancel pipeline
  after shared-engine changes; verify the Windows UI manually.

Investigate X/Twitter only after the user chooses to move on from TikTok. No other
platform was investigated in this spike.
