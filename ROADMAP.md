# stream-dlp feature roadmap

Prepared October 21, 2026, against stream-dlp 0.3.3; reconciled with the code on November 13, 2026 (after v0.7.0: other sites, recent downloads, the pasted Streamable link, silent sources).

Task checkboxes track implementation. Firefox automatic session import is now implemented experimentally; see R-01 and A-01/A-02 for its validation limits. Remaining research findings are based on source inspection and upstream documentation unless a live test is explicitly recorded.

The phases below group features. They are not the phases 1–4 of the [restructure plan](docs/plans/2026-10-27-ffc-alignment.md), which is finished; refer to work by item ID (F-03, P-02).

## Next up

The user puts items here, in order. Work on the first one unless told otherwise, and remove it when it is merged. When the list is empty, ask which item to take; do not pick one.

No items queued.

## Direction and priorities

Keep one native Rust app for Windows and experimental macOS, reusing yt-dlp,
FFmpeg, and Deno. The initial Mac scope is Apple Silicon, manual cookie import,
manual updates, and automated Mac builds; real-device acceptance is tracked in
[the Mac checklist](docs/macos.md). Build shared platform and session support
first, then deliver features in small releases:

1. Investigate automatic YouTube session import and Streamable upload feasibility.
2. Generalize the existing download engine and session boundaries.
3. Ship assisted automatic YouTube session import for browsers proven to work.
4. Add TikTok and X/Twitter, then Instagram and Facebook.
5. Add an experimental universal URL mode.
6. Add access to previously downloaded files and a Streamable sharing workflow.
7. Implement direct Streamable uploads only if a supported integration becomes available.

The Streamable investigation belongs first so its external dependency is discovered early. Its outcome must not delay the downloader releases. The order of platforms can change after live feasibility tests.

## What already exists

Module by module in the [Source layout](docs/reference/development.md#source-layout).

- Downloads (`src/engine/`): YouTube, and experimental TikTok, X/Twitter, Facebook and Instagram (P-01 to P-04) and other sites (U-01): link routing, format list, audio/video merging with silent sources saved as video only and lost audio reported as a failure (F-02), multi-video choice with an explicit download-all (F-04), progress, cancellation, no-overwrite.
- Sessions (`src/session/`): isolated manual cookie imports for YouTube, TikTok, X, Facebook and Instagram, plus experimental Firefox import for YouTube (F-03). The Download page's session link names the platform of the pasted address. Other sites never take a session.
- In-app playback of a finished download and of any video on My videos, with sound on Windows (S-06).
- My videos and previews, with an optional recent-download list whose file checks run in the background (S-01), the GIF converter, and browser-assisted Streamable sharing with the pasted link kept for the file, plus experimental automatic uploads (S-02, S-02A).
- Background app updates and download-tool updates (`src/update/`, `src/tools/`); Dev mode and dev notes (`src/dev/`).

Still missing: direct Streamable uploads (blocked, S-03/S-04); clean-machine acceptance of the Firefox import and the live browser matrix (R-01, A-01/A-02); automatic Chrome/Edge import (A-03; a Dev mode prototype exists, live acceptance open); signed-in Streamable and platform session cases. Other sites, recent downloads, the pasted Streamable link and silent sources shipped in v0.8.0; download-all (F-04), the destination guard and the Dev mode extension prototype in v0.9.0.

## Feasibility findings

### Automatic YouTube cookie import: feasible in principle, browser dependent

yt-dlp documents `--cookies-from-browser`. Windows Chromium cookie access also has documented decryption problems, so a working Firefox prototype does not establish Chrome or Edge support. YouTube rotates browser sessions, and its specific export guidance distinguishes private-window exports from regular browser-cookie extraction. See [yt-dlp cookie FAQ](https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp), [Windows decryption issue](https://github.com/yt-dlp/yt-dlp/issues/10927), and [YouTube session guidance](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies).

Recommended product: **Connect browser → choose browser/profile → import session → retry**. After that explicit action, the app handles the import. Offer optional automatic refresh from that selected profile when needed; keep anonymous downloads first and manual import available. Login and verification remain in the user's browser.

Do not promise silent import from every browser. Do not adopt the FAQ's combined all-site cookie export as the default YouTube workflow: the YouTube-specific guide warns it does not capture the recommended private session.

### Additional platforms: reuse upstream extractors, validate each workflow

yt-dlp lists TikTok, Instagram, Twitter, Facebook, and generic extraction. This establishes a foundation, not reliable support for every link or content type. Its own documentation says live tests are necessary because sites change. See [supported extractors](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md).

Initial scope: individual video posts and reels, with a media selector where necessary. Stories, profile feeds, bulk playlists, live broadcasts, and photo-only posts are outside the first release.

### Streamable: direct upload currently has an external blocker

Streamable's [official API documentation](https://streamable-support.zendesk.com/hc/en-us/articles/35415672400916-API-Documentation) currently describes a read-only API and says uploads must happen through its website. Direct upload with an account and anonymous direct upload are therefore both research items, not committed features. Old examples of upload endpoints are insufficient evidence of current supported access.

Ship a browser-assisted workflow if this remains unchanged: choose a downloaded file, open Streamable, and reveal the file for selection on the website. This reduces navigation but still requires the user to complete the upload. Whether the website currently permits uploads without an account needs verification. Never label an assisted handoff as a completed upload.

## Phase 0 — Resolve the unknowns

### R-01 — Browser session feasibility spike

Progress October 21, 2026: [offline probe and decision record](docs/research/browser-session-spike.md).
The installed helper passed nine synthetic-profile cases. A native, scoped Firefox
importer was subsequently implemented and tested against the user's explicitly
authorized profile while Firefox was open. Full authenticated-download acceptance,
candidate comparison, and the real-browser/Windows matrix remain open.

- [ ] Test the bundled yt-dlp and a candidate updated version on Windows 10/11 with Firefox, Chrome, and Edge. *Open (2026-11-13): only the installed helper ran, offline; the candidate was never run and no live Windows 10/11 browser matrix is recorded.*
- [ ] Cover default and secondary profiles, browser open/closed, missing profile, signed-out profile, locked database, decryption failure, and expired sessions. *Open (2026-11-17): live on Firefox 160 with two dedicated throwaway profiles: browser open and closed, a signed-out profile (now refused with a sign-in message instead of being loaded), a missing profile and a path with spaces and non-ASCII letters ([record](docs/research/browser-session-spike.md#live-run-on-dedicated-profiles-november-17-2026)). Signed-in, expired-session and decryption-failure cases, Chrome, Edge and Windows 10 have no live run.*
- [x] Determine whether any supported mechanism can capture a private session; otherwise explicitly retain manual private-window export. No supported extraction established; retain manual export per upstream guidance.
- [ ] Compare direct browser loading with a helper that imports, filters, and returns only permitted domains before network use. Verify the actual command/output behavior rather than assuming yt-dlp offers a domain-scoped export. *Open (2026-11-13): the native scoped Firefox importer replaced the helper idea; no side-by-side comparison of actual command behavior is recorded.*
- [ ] Record browser/version results without session values. Use dedicated test profiles and never inspect a developer's regular browser profile implicitly. *Open (2026-11-17): the signed-out Firefox results come from dedicated test profiles; the only signed-in result still comes from one explicitly authorized profile.*

**Done when:** there is a documented supported-browser list, a proven session-data path, and a fallback for every failure. If secure scoped import cannot be proven, ship guided import first and retain automation as experimental.

### R-02 — Streamable feasibility spike

Decision October 21, 2026: [Streamable feasibility record](docs/research/streamable-feasibility.md).
Select browser-assisted sharing. With explicit user approval, guest website
uploads of a synthetic H.264 clip and an existing AV1/AAC download both processed
and played from their share links. The signed-in account case remains untested.
The live guest UI says links expire after 2 days, conflicting with the help
article's 24-hour statement; do not promise a fixed guest retention period.
Direct upload remains blocked by the documented read-only API.

- [x] Recheck official API documentation and integration options before implementation.
- [ ] Verify the website's signed-in and signed-out upload flows, file requirements, limits, retention, and resulting link behavior using a small owned sample clip. *Open (2026-11-13): guest uploads verified; the signed-in flow, limits and a fixed retention period are unverified.*
- [x] Determine whether Streamable offers an approved upload integration, and what account authorization it requires. The public API excludes uploads; no supported upload authorization was identified. Any contact with Streamable is a separate user-authorized action.
- [x] Record separate outcomes for account-based upload, anonymous upload, and browser-assisted sharing.

**Done when:** a short decision record selects a supported direct integration or the assisted workflow. Do not build against undocumented private endpoints as the default integration.

### R-03 — Platform feasibility spike

November 3, 2026: P-01 covers individual TikTok videos and verified short links.
P-02 covers native X post videos, including selected entries and silent animations.
See the [TikTok investigation](docs/research/tiktok-feasibility.md), the P-01
short-link results below, and the [X release verification](docs/research/x-feasibility.md).

**Research complete.** The [four-platform helper comparison](docs/research/platform-feasibility.md)
sets the initial Facebook target to individual public videos, including Reels,
watch links, and the two verified share forms. Initial Instagram probes reached login on all five fixtures. P-03 later verified a Reel and carousel. See P-04 for Facebook implementation and acceptance status.

- [x] Test representative public links for TikTok, X/Twitter, Instagram, and Facebook with the installed and candidate yt-dlp versions.
- [x] Include desktop/mobile links, share redirects, video posts, reels, multiple-video posts, unavailable media, and login-required cases.
- [x] Record supported URL patterns, cookie evidence, missing metadata, collection responses, failures, and unverified cases for each platform.

**Done when:** each platform has an explicit first-release scope and test examples. A blocked platform does not prevent another from shipping.

## Phase 1 — Shared downloader foundation

### F-01 — Platform routing and URL handling

Depends on R-03.

- [x] Add a small platform model: YouTube, TikTok, Instagram, X/Twitter, Facebook, and Other.
- [x] Separate common HTTP(S) validation from each platform's URL normalization. Keep YouTube's existing canonicalization and regression cases.
- [x] Recognize exact domains and approved share-link hosts; reject deceptive lookalikes and URLs containing credentials.
- [x] Preserve query parameters required by a site. Remove tracking parameters only when known to be safe.
- [x] Define handling of redirects and non-public destinations, especially before enabling arbitrary URLs.

November 13, 2026: `parse_link` checks credentials and ports for every link, then
hands a platform's own hosts to its `normalize`. Other addresses of a named
platform and lookalikes are refused as that platform's; only hosts of no named
platform become `Source::Other`, so a refused YouTube or TikTok link is never
retried as Other. Other links keep their query and drop only the fragment.
Redirects and non-public destinations are defined in the
[other-sites reference](docs/reference/other-sites.md); the existing platform
regression tests pass unchanged.

### F-02 — General metadata and media selection

- [x] Carry media ID, source URL, extractor identity, and platform with metadata and completed-file results.
- [x] Handle absent titles, uploader, duration, resolution, and size with useful fallbacks; avoid showing `0p` for unknown resolution.
- [x] Handle direct playable media and yt-dlp collection results explicitly. For multi-video posts, let the user choose one video; do not silently pick the first or download an entire collection. (F-04 adds saving the listed videos as an explicit choice.)
- [x] Keep format selection and audio merging shared, including audio-less source clips where valid. Distinguish those from failed audio retrieval.
- [x] Include source identity in filenames or collision handling so identical IDs from different platforms cannot falsely reuse an unrelated completed file.

October 24, 2026: inspection and completed results now retain a shared media
identity for the implemented YouTube/TikTok routes. Download workers receive an
identity snapshot, and the completion screen labels the saved source. Missing IDs
or extractor metadata remain optional. This stays in memory for the current
download; no persistent history is introduced. Parsing and UI handoff regressions,
the default Rust suite (42 passed, 8 ignored), Clippy, and the local
download/merge/cancel pipeline pass. Tests used a workspace temporary directory;
the local pipeline required localhost socket access outside the sandbox.

Missing-metadata follow-up: YouTube and TikTok fixtures now cover missing, null,
and blank titles/creators, unknown or invalid duration, zero/missing dimensions,
and unknown sizes. The UI uses explicit unknown labels, retains ID-based title
fallbacks, distinguishes estimated sizes, and shows small files as less than 1 MB.
Unknown audio bitrate and container also have clear labels. The Rust suite now
passes 44 tests (8 opt-in tests ignored); Clippy and the local pipeline pass.

November 13, 2026: every site now shares one sound decision per format
(`engine::Sound`): a source whose every stream reports no audio is labelled
**This video has no sound** before download and saved as video only (previously a
YouTube-style or Facebook DASH silent source asked for audio that did not exist and
failed); video-only formats of sources with audio still merge as before. A merge
that fails, or a file saved without the audio its format promised (checked with
FFprobe), fails with its own lost-sound message instead of a silent success.
Direct files whose site reports no codecs (X, Facebook SD/HD, other sites) are saved
as they are. TikTok's own first-release rule still refuses silent clips. Covered
by deterministic tests for split and combined/direct formats and by the local
pipeline; silent sources other than the earlier X and Instagram samples are
verified from fixtures only.

### F-03 — Per-platform sessions and errors

- [x] Generalize `SessionCookies` using explicit platform domain policies, without dropping its current validation and cleanup protections.
- [x] Maintain a separate session for each platform. Verify cookie domain, path, secure, and expiration behavior through the production process path.
- [x] Never pass a combined all-platform cookie jar to arbitrary URLs. Disable session attachment for Other by default; any future site session requires explicit host scoping.
- [x] Replace YouTube-only interface labels, timeouts, and sign-in errors with platform-aware messages.
- [x] Distinguish unsupported link, missing video, authentication, expired session, rate limit, unavailable content, network failure, DRM, and helper failure.
- [x] Redact cookies, sensitive URL parameters, and account details from logs and displayed raw errors.

**Phase acceptance:** YouTube retains its existing behavior; fixtures for other sites parse correctly; session-isolation tests and the local download/merge/cancel pipeline pass. No new network-backed feature is declared supported from fixtures alone.

### F-04 — Download all videos of a multi-video post

Depends on F-02. Keeps F-02's rule (never silently pick the first, never download a whole collection) and adds one explicit choice beside choosing one video. [Reference](docs/reference/download-all.md).

- [x] Offer **Download all N videos** under the existing chooser; nothing downloads until it is pressed, and choosing one entry works as before.
- [x] Download the listed entries one after another, each through the single-entry download path (session and platform rules, Other-sites limits and destination guard, sound decision and lost-sound check, no-overwrite, file names); never one yt-dlp run for the collection.
- [x] Scope: exactly the entries the chooser shows, at most 25; no nested collections, playlists or profile feeds.
- [x] One quality for the batch, chosen before it starts (Best, 1080p, 720p, 480p), with a per-entry fallback.
- [x] Progress names the running entry (**Video 2 of 4**); Cancel stops it and the rest and keeps the saved files.
- [x] A failed entry does not stop the batch; the end lists saved files with their actions and failures with their messages; a batch with nothing saved is a failure.
- [x] Recent downloads records each saved file; usage reports and feedback gain nothing that identifies a post.
- [ ] Verify live multi-video posts on X, Instagram, Facebook and other sites. *Open (2026-11-17): X verified live (a four-video and a two-video post, every video saved). On another site two real pages ran through the batch in order, but the site rate-limited most entries; the batch now retries a rate-limited video once after a pause and spaces out the later ones (five of six saved on the rerun). Instagram needs a session (not tried) and Facebook post links with several videos are refused before inspection ([results](docs/reference/download-all.md#tests)).*

November 16, 2026: implemented in `src/app/batch.rs`; the engine is unchanged. Automatic session refresh does not run during a batch: a sign-in error fails that entry with its usual message. Two entries that would get the same file name save only the first; the second is reported as a failure instead of as a save.

## Phase 2 — Assisted automatic YouTube sessions

Depends on R-01 and F-03.

### A-01 — Browser connection interface

Implemented experimentally for Firefox: profile discovery/default selection,
background import with cancellation and query deadline, import-and-retry, refresh,
and manual fallback. Chrome/Edge show guided export. Full clean-machine acceptance
and the remaining live matrix are not yet complete.

- [ ] Add **Import from browser** alongside manual import, listing only supported installed browsers and available profiles. *Open (2026-11-13): implemented for Firefox only (Chrome/Edge get the export guide); the phase's clean-machine acceptance has not been run.*
- [ ] Explain the requested browser access before the first import. Store only the user's chosen source preference by default. *Open (2026-11-13): implemented for Firefox only (Chrome/Edge get the export guide); the phase's clean-machine acceptance has not been run.*
- [ ] Import on a background worker with progress, cancellation, timeout, and actionable failure messages. *Open (2026-11-13): implemented for Firefox only (Chrome/Edge get the export guide); the phase's clean-machine acceptance has not been run.*
- [ ] On success, retry the pending inspection/download once; prevent repeated automatic authentication loops. *Open (2026-11-13): implemented for Firefox only (Chrome/Edge get the export guide); the phase's clean-machine acceptance has not been run.*

### A-02 — Scoped import and lifecycle

Implemented for Firefox: native read-only SQLite selection of YouTube cookies,
expiration filtering and container isolation, memory-only sessions, disposable
scoped working files, refresh/removal, and optional bounded refresh from the
connected profile after an authentication error. No all-site database copy is made.

- [ ] Use the proven R-01 mechanism and filter to the approved YouTube domains before storing the session or using it for network operations. *Open (2026-11-13): the Firefox importer filters to YouTube domains before storing; R-01's mechanism is not yet proven on the live matrix.*
- [ ] Keep session values in memory and use restricted disposable files only where necessary. If a prototype temporarily materializes broader browser cookies, document that limitation and prove access restrictions and cleanup before accepting that design. *Open (2026-11-13): implemented (memory-only, no all-site copy); awaits the phase's clean-machine acceptance.*
- [ ] Preserve cleanup on success, failure, cancellation, and normal shutdown; define a safe strategy for stale temporary files after a crash. *Open (2026-11-13): stale-file strategy implemented: each start removes `stream-dlp-session-*` working files older than 24 hours from the temporary folder, by name and age only ([reference](docs/reference/session-files.md)); the cleanup on success, failure, cancellation and shutdown awaits the phase's clean-machine acceptance.*
- [ ] Provide **Refresh**, **Remove session**, and a clear loaded/expired/failed status. A parsed cookie file is not proof of a valid signed-in session. *Open (2026-11-13): implemented for Firefox and manual imports; awaits the phase's clean-machine acceptance.*
- [ ] If enabled, refresh only the previously selected source when authentication fails, with bounded retries. Do not scan other profiles silently. *Open (2026-11-13): implemented and unit-tested (one refresh per attempt, same profile); awaits the phase's clean-machine acceptance.*
- [ ] Fall back to manual export for unsupported encryption, private sessions, or cookie rotation problems. *Open (2026-11-13): the manual export guide is offered; live failure cases (encryption, rotation) are untested.*

### A-03 — Conditional extension investigation

Deferred Chrome automatic-mode options and the recommended extension prototype
are saved in [Chrome auto-mode options](docs/research/chrome-auto-mode-options.md), including
dedicated-profile and assisted-export alternatives, constraints, and resume steps.
The user chose option 1 (companion extension and native messaging).

*Prototype (2026-11-16):* built on `feat/chrome-extension-import` and shown only in Dev mode,
labelled experimental: the `extension/` folder (MV3, cookies + native messaging, YouTube host
permissions only), the app's own executable as a per-user native messaging host registered on
Connect and removed on Disconnect, an authenticated loopback channel to the running app, cookie
re-filtering through the YouTube session policy, and one bounded refresh from the same
connection ([reference](docs/reference/chrome-extension.md)). Verified offline only: Rust and
Deno tests, and the built release executable relaying framed messages directly and through
`cmd /d /c`. Not yet run in a real Chrome or Edge, and no signed-in YouTube inspection,
download or refresh through it is proven; the
[manual acceptance checklist](docs/reference/chrome-extension.md#manual-acceptance-checklist)
is open. November 17, 2026: in a real Chrome 157 on a throwaway profile the extension loaded
with its fixed ID and the popup reported a missing registration correctly; the connection
itself could not be tried from the agent's session
([record](docs/reference/chrome-extension.md#partial-run-in-a-real-chrome-november-17-2026)). November 20, 2026: the Windows uninstaller removes the registration by running `stream-dlp.exe --unregister-extension` (the Disconnect code; a portable copy's registration is kept, updates keep it); verified by unit tests and an installer compile, the uninstall itself only by CI's Package check ([removal](docs/reference/chrome-extension.md#removal)).

- [ ] If Chrome/Edge extraction is blocked, evaluate an optional extension limited to the needed site and a local authenticated handoff. *Open (2026-11-20): option 1 was selected and its prototype shipped in Dev mode with v0.9.0 (above); the evaluation ends with the manual acceptance checklist, which only a person can run and nobody has run yet. This box stays open until then.*
- [x] Estimate extension maintenance, distribution, and permission requirements before treating it as a separate release commitment. *Done (2026-11-20): [estimate](docs/reference/chrome-extension.md#distribution-and-maintenance-estimate-november-20-2026) of shipping `extension/` for load-unpacked use versus Chrome Web Store and Edge Add-ons listings (accounts, privacy policy, review, policy risk, the ID change, ongoing maintenance); removal on uninstall is implemented. Recommendation: stay a Dev mode prototype, no release commitment, until the manual acceptance checklist passes; then ship the unpacked folder before any store listing.*

**Phase acceptance:** at least one advertised browser completes the full connect-and-retry flow on a clean supported Windows machine; unsupported browsers fail helpfully; unrelated cookies are excluded; manual import continues working.

## Phase 3 — Named platform releases

Depends on F-01 through F-03. Ship each platform independently after live validation.

### P-01 — TikTok

Initial scope and implementation requirements are recorded in the
[TikTok feasibility decision](docs/research/tiktok-feasibility.md). Full direct video links
are implemented experimentally, with production-backend download, decode,
no-overwrite, session isolation, and startup-cancellation validation. Since v0.3.9,
short share links (`vm.tiktok.com`, `vt.tiktok.com`, `tiktok.com/t/…`) resolve to the
video first. One fresh `vm.tiktok.com` link and three `vt.tiktok.com` links passed
the production-backend test on November 2, 2026. The maintainer confirmed the
`tiktok.com/t/…` form worked in manual testing.

- [x] Support individual public video URLs and verified short/share links.
- [x] Handle vertical video, missing creator metadata, and available format variations.
- [x] Explain photo slideshows, live links, unavailable content, and authentication requirements explicitly.

### P-02 — X / Twitter

**Done.** All P-02 cases for public X/Twitter posts passed validation by
November 3, 2026. The v0.5.0 backend saved and decoded a single video, a
selected entry from a multi-video post, and a silent animated MP4. It also
handled photo-only, unavailable, and login-required posts.
See the [live test results](docs/research/x-feasibility.md).

- [x] Accept both `x.com` and `twitter.com` post URLs and verified share variants.
- [x] Support video posts and a selector for posts with multiple videos.
- [x] Test login-required posts, unavailable media, and extracted animated media; show only output types the app can actually save.

### P-03 — Instagram

- [x] Support direct Reel and video-post URL patterns.
- [x] Select video entries in mixed carousels and explain photo-only posts.
- [x] Add domain-scoped manual session import and retry for Instagram.

November 3, 2026, Windows and bundled yt-dlp 2026.08.19. The app downloaded and decoded a Reel with sound and the selected second video from a three-video carousel without sound. Mixed and photo-only posts and session isolation have offline coverage. Live mixed/photo-only posts and signed-in recovery remain unverified. See the [Instagram verification record](docs/research/platform-feasibility.md#instagram-p-03).

### P-04 — Facebook

- [x] Support verified watch, reel, mobile, and share-link patterns for individual videos.
- [x] Handle redirects and metadata differences; add domain-scoped manual session support.
- [x] Clearly explain unavailable or inaccessible posts and unsupported collections.

November 3, 2026 (bundled yt-dlp 2026.08.19). Offline tests cover routing, redirect destinations, missing metadata, collections and session isolation. The production backend anonymously resolved a permitted Reel share link, inspected the video, and saved one file. FFmpeg decoded both its video and audio streams to completion. Signed-in recovery remains unverified. See the [Facebook feasibility record](docs/research/platform-feasibility.md#facebook-p-04).

**Acceptance for each platform:** an owned/permitted public sample inspects, downloads, and plays with the expected streams; applicable session cases work; redirects, missing metadata, failures, cancellation, and multi-media behavior are verified. Record test date and helper version. Preserve the YouTube regression suite for every release.

## Phase 4 — Universal URL mode

Depends on Phase 1 and experience from at least the first named-platform releases.

### U-01 — Other sites / Auto-detect

- [x] Route recognized sites to their platform handler; allow other public HTTP(S) URLs through yt-dlp's remaining site-specific extractors and generic fallback.
- [x] Label this **Other sites (experimental)** and explain that compatibility varies. Universal means an attempt at supported media extraction, not every website.
- [x] Support public direct video files and non-DRM HLS/DASH where the existing tools can handle them. *Done (2026-11-16): a real DASH manifest inspects and downloads through the app's own code.*
- [x] Define resource limits for redirects, inspection time, metadata size, and collection size. Keep bulk downloads and arbitrary local/network-resource access outside the initial scope.
- [x] Implement the F-01 destination policy for redirects and nested media requests; validate what yt-dlp and FFmpeg actually fetch rather than checking only the pasted URL. *Done (2026-11-16): every helper request goes through the destination guard; limits: a system or environment proxy turns it off, and HTTPS streams only FFmpeg can download fail.*
- [x] Leave authentication off by default and surface meaningful unsupported-site, DRM, and missing-media results.

November 13, 2026, bundled yt-dlp 2026.08.19: implemented in
`src/engine/sources/other.rs` ([reference](docs/reference/other-sites.md)). Other
links never take a session (tested with sessions for all five platforms
loaded). Limits: 5 checked redirects in 15 seconds, the existing 150-second
inspection, 32 MB of metadata, 25 entries with one chosen, never a collection.
Live, audio-only, DRM and non-HTTP results are refused with their own messages.

Direct files and HLS/DASH stay open: synthetic fixtures for a direct file, an
embedded page and HLS/DASH results pass, and the bundled yt-dlp, run by hand
with the app's arguments against local fixtures, found and saved a direct MP4,
a page's embedded video and an HLS stream; one public sample file inspected as
a direct video. The `#[ignore]`d end-to-end test through the app's own code
(`other_sites_local_fixtures_inspect_and_download`) passed on Windows on
November 13, 2026; DASH through a real manifest is untested. Nested requests stay open
because only part of them can be enforced: the pasted link and its HTTP
redirects are checked, `file://` stays disabled and FFmpeg gets a network-only
protocol whitelist (both verified against a hostile local playlist), but
yt-dlp's own later requests (embeds, manifests, segments, its own redirects)
are not checked against the address policy, and DNS can change between the
check and yt-dlp's lookup. See the reference's known limits.

November 16, 2026, bundled yt-dlp 2026.08.19 and FFmpeg N-127911: both open
items done. A destination guard (`src/engine/guard.rs`), a loopback proxy
started for each inspection and download of an Other link, now carries every
request of yt-dlp and of the FFmpeg it starts (`--proxy`, `NO_PROXY` removed).
It applies the address policy to each request (public names or public IP
literals, ports 80, 443 or 1024–65535; the pasted link keeps the stricter link
policy), connects only to the address it checked (no rebinding) and allows one
request per connection.
[Experiments](docs/research/other-site-destination-guard.md) showed that
yt-dlp's own requests, native HLS and native DASH all use the proxy, and that
FFmpeg reaches HTTPS through it only with `httpproxy`, which would let a
playlist bypass the guard. So HLS is forced onto yt-dlp's own downloader and
HTTPS streams that only FFmpeg can download (for example `SAMPLE-AES`) fail
with their own message. The `#[ignore]`d end-to-end test passed on Windows: a
direct file, an embedded page, HLS, a real DASH manifest and HLS segments on
port 8080 (by name and by public IP literal) downloaded through the guard, and hostile pages and playlists pointing at private addresses were
refused without the private address being reached. When the system or the
environment sets a proxy, the guard is off and only the pasted link and its
redirects are checked. Not yet run on the Mac.

**Phase acceptance:** public direct-file, embedded-video, and manifest fixtures work; unsupported pages fail cleanly; arbitrary URLs cannot inherit another platform's credentials.

## Phase 5 — Downloaded files and Streamable sharing

### S-01 — Find current and older downloads

Depends on F-02; can proceed independently of the Streamable API decision.

- [x] Keep the completed file available for a **Share on Streamable** action.
- [x] Add **Choose existing video** for files downloaded before this feature existed or outside stream-dlp.
- [x] Add a small local recent-download list if desired for convenience: final path, title, source platform, and completion time; no cookies or credentials.
- [x] If history is included, make it optional, removable, and tolerant of moved/deleted files. Browsing an existing file must work without history.
- [x] Probe selected files with FFprobe to check readability, container, streams, duration, and size.

Implemented a session-only **My videos** folder browser with a bounded,
nonrecursive list, refresh, file details, play/reveal actions, and existing-file
selection. Shared metadata work in F-02 remains separate.

November 13, 2026: an optional **Recent downloads** list, off by default
(Settings), keeps the last 50 downloads' path, title, platform word and time in
`history.json`; entries can be removed or the list cleared, moved or deleted files
stay listed and muted, and selecting one works like choosing an existing video
([reference](docs/reference/download-history.md)).

### S-06 — Play videos in the app

Depends on S-01. Requested November 17, 2026: the downloaded video is visible in the app and plays there, and so does every video on My videos; it has to feel seamless, reactive and fast. [Reference](docs/reference/in-app-playback.md).

- [x] A player widget of its own (`src/app/player.rs`): poster, play/pause, position bar with seeking and dragging, volume and mute, keyboard, and a button for the system player.
- [x] Frames from the bundled FFmpeg on a worker thread, at most three ahead of the picture; no media library and no new dependency. Nothing decodes or waits on the UI thread; seeking and switching videos stop the previous helpers at once.
- [x] Sound on Windows through the system's wave output, declared by hand; the picture follows the sound's clock.
- [x] Download page: the saved video plays under **Download complete**; each video a download-all saved has a thumbnail that plays it.
- [x] My videos: a thumbnail per video that plays it at once, thumbnails loaded three at a time in the background.
- [ ] Sound on macOS. *Open (2026-11-17): the Mac build plays the picture only. It needs an output written for macOS (or an audio crate, which is a dependency to decide on) and the PCM encoder in the Mac FFmpeg build.*
- [ ] Watch and listen on real downloads on Windows and on a Mac. *Open (2026-11-17): on Windows, generated H.264, VP9 and AV1 clips with AAC and Opus sound were played in the app's window on My videos, with the app's sound level measured ([results](docs/reference/in-app-playback.md#in-the-window-november-17-2026-windows-11-debug-build)). No person has listened, no real download was played, the Download page's player was not shown live, and the Mac build was only compiled and checked by CI.*

### S-02 — Browser-assisted Streamable workflow

Depends on R-02 and S-01; recommended initial sharing release under the current API restriction.

- [x] Open Streamable and reveal the selected video in Explorer so the user can select or drag it into the website.
- [x] Explain whether login is required according to the verified website behavior; use the user's browser session.
- [x] Let the user optionally paste the resulting share URL back into stream-dlp for opening/copying.
- [x] Keep the local file and download result intact when the handoff is cancelled or the website fails.

**Phase acceptance:** both a newly downloaded file and an older file can enter the workflow. The app reports only the handoff it performed; it does not claim to have detected upload completion.

November 13, 2026: after **Use website manually**, an optional field takes the
pasted Streamable link (only `https://streamable.com/<id>`, normalised), shown
with Open and Copy as the link the user pasted; it is kept in memory for the
selected file only.

### S-02A — Experimental automatic website uploads

Implemented at the user's request; [design and validation](docs/reference/automatic-streamable-upload.md).
This automates the website rather than using an upload API. The manual S-02
handoff remains available. S-03/S-04's direct-API blockers are unchanged.

- [x] Explicit automatic-upload action from the selected-video page, shared by completed downloads and existing files.
- [x] Dedicated temporary private browser, background worker, website progress/status, cancellation, and bounded waiting.
- [x] Exactly one submission per attempt; preserve a known but unverified link after ambiguous completion.
- [x] Verify playback in a separate tab before reporting completion; show and copy the returned share link.
- [x] Test unchanged file bytes, inactive inputs, pre-existing links, and playable output with a local fixture.
- [x] Perform an explicitly authorized synthetic guest upload through the production runtime on Windows/Edge.
- [ ] Validate account sign-in, challenges, large/rejected files, and other supported OS/browser combinations. *Open (2026-11-13): only a synthetic guest upload on Windows/Edge was performed.*

### S-03 — Conditional direct uploads with a user account

Blocked until R-02 identifies supported upload access.

- [ ] Implement the approved authorization method; use Windows credential storage for persistent secrets and provide disconnect/removal. *Open (2026-11-13): blocked: no supported upload API (R-02).*
- [ ] Add file upload, byte progress, cancellation, bounded retries, processing status, and the final share link. *Open (2026-11-13): blocked: no supported upload API (R-02).*
- [ ] Handle quota/size/duration limits, expired authorization, offline failures, and ambiguous completion without blindly creating duplicates. *Open (2026-11-13): blocked: no supported upload API (R-02).*
- [ ] Keep upload state separate from download state. A failed upload must not turn a successful download into a failed download. *Open (2026-11-13): blocked: no supported upload API (R-02).*
- [ ] Offer **Download then upload** only as an explicit opt-in after the standalone upload flow is reliable. *Open (2026-11-13): blocked: no supported upload API (R-02).*

### S-04 — Conditional uploads without a user account

Blocked until supported anonymous upload is verified separately.

- [ ] Confirm the approved anonymous mechanism, limits, retention, ownership, and deletion capabilities. *Open (2026-11-13): blocked: no supported upload API (R-02).*
- [ ] Explain those constraints before upload; keep cancellation and recovery behavior consistent with account uploads. *Open (2026-11-13): blocked: no supported upload API (R-02).*
- [ ] If anonymous access is unavailable, show a clear account-required or browser-assisted option. *Open (2026-11-13): blocked: no supported upload API (R-02).*

Do not embed a shared stream-dlp Streamable account or secret in the executable as a substitute for anonymous uploads.

### S-05 — Conditional media preparation

- [ ] Use verified Streamable requirements to decide whether a file needs remuxing or conversion. *Open (2026-11-13): conditional on direct uploads; not started.*
- [ ] Prefer the original file or lossless remux when compatible. Offer explicit conversion with progress only when required. *Open (2026-11-13): conditional on direct uploads; not started.*
- [ ] Preserve the original, support cancellation, check disk space, and remove temporary derivatives safely. *Open (2026-11-13): conditional on direct uploads; not started.*

**Direct-upload acceptance, if unblocked:** a supported account upload returns a playable URL after processing; anonymous upload is verified independently; retries, cancellation, credential removal, and temporary-file cleanup pass.

## Release checks and maintenance

- [x] Add deterministic tests for URL handling, metadata variants, collection selection, platform-specific errors, cookie isolation, and any new persistent records. November 13, 2026: audited per platform and area; the missing cases (credentials and ports for X, Facebook and Instagram, X and Instagram missing metadata, TikTok collections, Facebook and Instagram error classes, TikTok and X cookie jars, platform lookalikes in mixed exports, the recent-downloads setting) were added. New features still need their own.
- [x] Keep live platform tests opt-in and separate from deterministic tests. Record environmental failures instead of weakening assertions to hide them. November 13, 2026: every network, account or real-file test is `#[ignore]`d and run only by name (AGENTS.md); the research records keep environmental failures.
- [x] Run `cargo xtask check`, and the existing local pipeline test (`cargo test --locked local_download_pipeline -- --ignored`) for relevant engine changes. November 16, 2026: `cargo xtask pipeline` runs the four ignored tests that need only the bundled tools and localhost, each by its exact name, and AGENTS.md requires a passing run for changes under `src/engine/` and `src/media/`. It is a rule for agents, not a CI gate: the Checks workflow has no helpers.
- [ ] Verify clean Windows operation, long/Unicode paths, redirected Downloads, cancellation, and portable packaging for each shipped capability. *Open (2026-11-17): CI's Package check covers packaging and cancellation is unit-tested. The local pipeline now downloads and merges into a folder with non-ASCII names and into a 283-character folder (passed on Windows 11 with Windows long paths enabled; the default, long paths disabled, is untested). Clean-machine and redirected-Downloads checks are not recorded, and the path cases cover the shared download path, not each capability.*
- [ ] Update the README, in-app help, package description, third-party notices where needed, and supported-platform/browser list. *Open (2026-11-13): a step for every release, not a one-off.*
- [x] Validate helper updates through the existing verified setup process; record the tested yt-dlp version for each release and provide a helpful update path when extraction breaks. November 16, 2026: release notes after 0.8.0 end with `Tested with yt-dlp X.`, and `cargo xtask release` refuses when that line is missing or names another version than the bundled yt-dlp; `cargo xtask tools` prints the line. Verified tool updates and the update hint already existed.
- [ ] For any future direct uploader, test against mock responses first and perform a small authorized real upload before calling it supported. *Open (2026-11-13): no direct uploader exists yet.*

## Suggested release boundaries

- **Release A:** shared foundation and the proven browser-session import flow, with manual fallback. *Foundation shipped; Firefox import is experimental and unproven on a clean machine.*
- **Release B:** TikTok and X/Twitter, each independently validated. *Shipped: TikTok in v0.3.9, X in v0.5.0 (validated by v0.6.1).*
- **Release C:** Instagram and Facebook, including their required session handling. *Shipped in v0.7.0; signed-in recovery unverified.*
- **Release D:** experimental Other sites mode. *Shipped in v0.8.0; the destination guard for nested requests in v0.9.0.*
- **Release E:** current/existing-file selection and browser-assisted Streamable sharing. *File selection and the handoff shipped by v0.3.5; the recent-download list and the pasted link shipped in v0.8.0.*
- **Later, conditional:** direct Streamable uploads, anonymous mode, optional conversion, and download-then-upload automation.

These are scope boundaries, not calendar promises. Estimate work after the three feasibility spikes; unknown browser access and external upload support make firm dates premature. S-01/S-02 can move earlier if sharing is the highest priority.

## Decisions to revisit after the spikes

Recommended defaults are: one pasted URL with automatic platform detection; individual-video downloads; browser imports initiated by the user; sessions kept in memory; no credentials for Other sites; existing-file browsing available without history; and Streamable browser assistance until a supported upload path is proven.

Future scope decisions include which browsers to advertise, whether to build an extension, and whether to persist optional history. Multi-video posts gained an explicit download-all action in F-04, limited to the listed entries; bulk downloading of playlists, profiles or feeds stays out of scope. Additional feature ideas can be appended as separate tasks with dependencies and acceptance criteria.

## Dev release channel (October 21, 2026; removed October 30, 2026)

History only: the app has one stable channel now. Do not rebuild this.

- [x] Add persistent Public/Dev selection in About & tools and show the installed build channel.
- [x] Isolate stable releases from explicitly tagged Dev prereleases; install only on request.
- [x] Allow explicit Dev-to-Public rollback and reject stale background check results.
- [x] Package Dev artifacts separately with clear installer branding and the same app identity.
- [x] Include the selector in Public v0.3.5; v0.3.4 requires a newer installer to regain it.
- [x] Windows acceptance confirmed by the user on October 22, 2026, including channel installation and representative Streamable uploads.

Replaced on October 30, 2026 by a single stable channel with background updates; see the [update rework](docs/plans/2026-10-27-ffc-alignment.md).
