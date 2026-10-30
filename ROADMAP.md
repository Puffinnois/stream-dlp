# stream-dlp feature roadmap

Prepared October 21, 2026, against stream-dlp 0.3.3.

Task checkboxes track implementation. Firefox automatic session import is now implemented experimentally; see R-01 and A-01/A-02 for its validation limits. Remaining research findings are based on source inspection and upstream documentation unless a live test is explicitly recorded.

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

- `src/main.rs`: egui interface, background workers, a single active download, manual cookie import, and the latest completed file path.
- `src/backend.rs`: YouTube-only URL normalization, metadata parsing, format selection, yt-dlp execution, progress, cancellation, and final-path detection.
- `src/session.rs`: Netscape cookie validation, YouTube-only domain filtering, in-memory session storage, and disposable per-operation cookie files.
- `src/export_help.rs`: guided manual export and Windows Downloads folder handling.
- `Setup tools.ps1` and `build.ps1`: verified helper installation and portable release packaging.

Reusable strengths include shell-free process arguments, audio/video merging, no-overwrite behavior, cancellation, and local pipeline tests. Changes are needed around URL routing, platform metadata, site-specific errors, authentication, and selecting media when a post contains multiple videos. The app currently has no persistent download library or upload subsystem.

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

Progress October 21, 2026: [offline probe and decision record](docs/browser-session-spike.md).
The installed helper passed nine synthetic-profile cases. A native, scoped Firefox
importer was subsequently implemented and tested against the user's explicitly
authorized profile while Firefox was open. Full authenticated-download acceptance,
candidate comparison, and the real-browser/Windows matrix remain open.

- [ ] Test the bundled yt-dlp and a candidate updated version on Windows 10/11 with Firefox, Chrome, and Edge.
- [ ] Cover default and secondary profiles, browser open/closed, missing profile, signed-out profile, locked database, decryption failure, and expired sessions.
- [x] Determine whether any supported mechanism can capture a private session; otherwise explicitly retain manual private-window export. No supported extraction established; retain manual export per upstream guidance.
- [ ] Compare direct browser loading with a helper that imports, filters, and returns only permitted domains before network use. Verify the actual command/output behavior rather than assuming yt-dlp offers a domain-scoped export.
- [ ] Record browser/version results without session values. Use dedicated test profiles and never inspect a developer's regular browser profile implicitly.

**Done when:** there is a documented supported-browser list, a proven session-data path, and a fallback for every failure. If secure scoped import cannot be proven, ship guided import first and retain automation as experimental.

### R-02 — Streamable feasibility spike

Decision October 21, 2026: [Streamable feasibility record](docs/streamable-feasibility.md).
Select browser-assisted sharing. With explicit user approval, guest website
uploads of a synthetic H.264 clip and an existing AV1/AAC download both processed
and played from their share links. The signed-in account case remains untested.
The live guest UI says links expire after 2 days, conflicting with the help
article's 24-hour statement; do not promise a fixed guest retention period.
Direct upload remains blocked by the documented read-only API.

- [x] Recheck official API documentation and integration options before implementation.
- [ ] Verify the website's signed-in and signed-out upload flows, file requirements, limits, retention, and resulting link behavior using a small owned sample clip.
- [x] Determine whether Streamable offers an approved upload integration, and what account authorization it requires. The public API excludes uploads; no supported upload authorization was identified. Any contact with Streamable is a separate user-authorized action.
- [x] Record separate outcomes for account-based upload, anonymous upload, and browser-assisted sharing.

**Done when:** a short decision record selects a supported direct integration or the assisted workflow. Do not build against undocumented private endpoints as the default integration.

### R-03 — Platform feasibility spike

October 24, 2026: [TikTok investigation](docs/tiktok-feasibility.md) compares the
bundled and latest stable helpers using 20 anonymous metadata probes. Direct
individual-video metadata succeeded. A user-supplied sample passed helper-level
download, full audio/video decode, and no-overwrite checks. Short/mobile links and
production-app acceptance remain open. Investigations proceed one platform at a time; the other
platforms have not been investigated yet.

- [ ] Test representative public links for TikTok, X/Twitter, Instagram, and Facebook with the installed and candidate yt-dlp versions.
- [ ] Include desktop/mobile links, share redirects, video posts, reels, multiple-video posts, unavailable media, and login-required cases.
- [ ] Record supported URL patterns, required cookies, missing metadata, collection responses, and failures for each platform.

**Done when:** each platform has an explicit first-release scope and test examples. A blocked platform does not prevent another from shipping.

## Phase 1 — Shared downloader foundation

### F-01 — Platform routing and URL handling

Depends on R-03.

- [ ] Add a small platform model: YouTube, TikTok, Instagram, X/Twitter, Facebook, and Other.
- [ ] Separate common HTTP(S) validation from each platform's URL normalization. Keep YouTube's existing canonicalization and regression cases.
- [ ] Recognize exact domains and approved share-link hosts; reject deceptive lookalikes and URLs containing credentials.
- [ ] Preserve query parameters required by a site. Remove tracking parameters only when known to be safe.
- [ ] Define handling of redirects and non-public destinations, especially before enabling arbitrary URLs.

### F-02 — General metadata and media selection

- [x] Carry media ID, source URL, extractor identity, and platform with metadata and completed-file results.
- [x] Handle absent titles, uploader, duration, resolution, and size with useful fallbacks; avoid showing `0p` for unknown resolution.
- [ ] Handle direct playable media and yt-dlp collection results explicitly. For multi-video posts, let the user choose one video; do not silently pick the first or download an entire collection.
- [ ] Keep format selection and audio merging shared, including audio-less source clips where valid. Distinguish those from failed audio retrieval.
- [ ] Include source identity in filenames or collision handling so identical IDs from different platforms cannot falsely reuse an unrelated completed file.

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

### F-03 — Per-platform sessions and errors

- [ ] Generalize `SessionCookies` using explicit platform domain policies, without dropping its current validation and cleanup protections.
- [ ] Maintain a separate session for each platform. Verify cookie domain, path, secure, and expiration behavior through the production process path.
- [ ] Never pass a combined all-platform cookie jar to arbitrary URLs. Disable session attachment for Other by default; any future site session requires explicit host scoping.
- [ ] Replace YouTube-only interface labels, timeouts, and sign-in errors with platform-aware messages.
- [ ] Distinguish unsupported link, missing video, authentication, expired session, rate limit, unavailable content, network failure, DRM, and helper failure.
- [ ] Redact cookies, sensitive URL parameters, and account details from logs and displayed raw errors.

**Phase acceptance:** YouTube retains its existing behavior; fixtures for other sites parse correctly; session-isolation tests and the local download/merge/cancel pipeline pass. No new network-backed feature is declared supported from fixtures alone.

## Phase 2 — Assisted automatic YouTube sessions

Depends on R-01 and F-03.

### A-01 — Browser connection interface

Implemented experimentally for Firefox: profile discovery/default selection,
background import with cancellation and query deadline, import-and-retry, refresh,
and manual fallback. Chrome/Edge show guided export. Full clean-machine acceptance
and the remaining live matrix are not yet complete.

- [ ] Add **Import from browser** alongside manual import, listing only supported installed browsers and available profiles.
- [ ] Explain the requested browser access before the first import. Store only the user's chosen source preference by default.
- [ ] Import on a background worker with progress, cancellation, timeout, and actionable failure messages.
- [ ] On success, retry the pending inspection/download once; prevent repeated automatic authentication loops.

### A-02 — Scoped import and lifecycle

Implemented for Firefox: native read-only SQLite selection of YouTube cookies,
expiration filtering and container isolation, memory-only sessions, disposable
scoped working files, refresh/removal, and optional bounded refresh from the
connected profile after an authentication error. No all-site database copy is made.

- [ ] Use the proven R-01 mechanism and filter to the approved YouTube domains before storing the session or using it for network operations.
- [ ] Keep session values in memory and use restricted disposable files only where necessary. If a prototype temporarily materializes broader browser cookies, document that limitation and prove access restrictions and cleanup before accepting that design.
- [ ] Preserve cleanup on success, failure, cancellation, and normal shutdown; define a safe strategy for stale temporary files after a crash.
- [ ] Provide **Refresh**, **Remove session**, and a clear loaded/expired/failed status. A parsed cookie file is not proof of a valid signed-in session.
- [ ] If enabled, refresh only the previously selected source when authentication fails, with bounded retries. Do not scan other profiles silently.
- [ ] Fall back to manual export for unsupported encryption, private sessions, or cookie rotation problems.

### A-03 — Conditional extension investigation

Deferred Chrome automatic-mode options and the recommended extension prototype
are saved in [Chrome auto-mode options](docs/chrome-auto-mode-options.md), including
dedicated-profile and assisted-export alternatives, constraints, and resume steps.
No Chrome automatic integration has been implemented or selected yet.

- [ ] If Chrome/Edge extraction is blocked, evaluate an optional extension limited to the needed site and a local authenticated handoff.
- [ ] Estimate extension maintenance, distribution, and permission requirements before treating it as a separate release commitment.

**Phase acceptance:** at least one advertised browser completes the full connect-and-retry flow on a clean supported Windows machine; unsupported browsers fail helpfully; unrelated cookies are excluded; manual import continues working.

## Phase 3 — Named platform releases

Depends on F-01 through F-03. Ship each platform independently after live validation.

### P-01 — TikTok

Initial scope and implementation requirements are recorded in the
[TikTok feasibility decision](docs/tiktok-feasibility.md). Full direct video links
are implemented experimentally, with production-backend download, decode,
no-overwrite, session isolation, and startup-cancellation validation. Short/mobile
links and wider acceptance remain open; no other platform was added.

- [ ] Support individual public video URLs and verified short/share links.
- [ ] Handle vertical video, missing creator metadata, and available format variations.
- [ ] Explain photo slideshows, live links, unavailable content, and authentication requirements explicitly.

### P-02 — X / Twitter

- [ ] Accept both `x.com` and `twitter.com` post URLs and verified share variants.
- [ ] Support video posts and a selector for posts with multiple videos.
- [ ] Test login-required posts, unavailable media, and extracted animated media; show only output types the app can actually save.

### P-03 — Instagram

- [ ] Support verified Reel and video-post URL patterns.
- [ ] Select video entries in mixed carousels; explain photo-only posts.
- [ ] Add domain-scoped manual sessions and authentication recovery for the supported content. Reuse browser import only after separate compatibility validation.

### P-04 — Facebook

- [ ] Support verified watch, reel, mobile, and share-link patterns for individual videos.
- [ ] Handle redirects and metadata differences; add domain-scoped session support where required.
- [ ] Clearly explain unavailable or inaccessible posts and unsupported collections.

**Acceptance for each platform:** an owned/permitted public sample inspects, downloads, and plays with the expected streams; applicable session cases work; redirects, missing metadata, failures, cancellation, and multi-media behavior are verified. Record test date and helper version. Preserve the YouTube regression suite for every release.

## Phase 4 — Universal URL mode

Depends on Phase 1 and experience from at least the first named-platform releases.

### U-01 — Other sites / Auto-detect

- [ ] Route recognized sites to their platform handler; allow other public HTTP(S) URLs through yt-dlp's remaining site-specific extractors and generic fallback.
- [ ] Label this **Other sites (experimental)** and explain that compatibility varies. Universal means an attempt at supported media extraction, not every website.
- [ ] Support public direct video files and non-DRM HLS/DASH where the existing tools can handle them.
- [ ] Define resource limits for redirects, inspection time, metadata size, and collection size. Keep bulk downloads and arbitrary local/network-resource access outside the initial scope.
- [ ] Implement the F-01 destination policy for redirects and nested media requests; validate what yt-dlp and FFmpeg actually fetch rather than checking only the pasted URL.
- [ ] Leave authentication off by default and surface meaningful unsupported-site, DRM, and missing-media results.

**Phase acceptance:** public direct-file, embedded-video, and manifest fixtures work; unsupported pages fail cleanly; arbitrary URLs cannot inherit another platform's credentials.

## Phase 5 — Downloaded files and Streamable sharing

### S-01 — Find current and older downloads

Depends on F-02; can proceed independently of the Streamable API decision.

- [x] Keep the completed file available for a **Share on Streamable** action.
- [x] Add **Choose existing video** for files downloaded before this feature existed or outside stream-dlp.
- [ ] Add a small local recent-download list if desired for convenience: final path, title, source platform, and completion time; no cookies or credentials.
- [ ] If history is included, make it optional, removable, and tolerant of moved/deleted files. Browsing an existing file must work without history.
- [x] Probe selected files with FFprobe to check readability, container, streams, duration, and size.

Implemented a session-only **My videos** folder browser with a bounded,
nonrecursive list, refresh, file details, play/reveal actions, and existing-file
selection. No history is persisted. Shared metadata work in F-02 remains separate.

### S-02 — Browser-assisted Streamable workflow

Depends on R-02 and S-01; recommended initial sharing release under the current API restriction.

- [x] Open Streamable and reveal the selected video in Explorer so the user can select or drag it into the website.
- [x] Explain whether login is required according to the verified website behavior; use the user's browser session.
- [ ] Let the user optionally paste the resulting share URL back into stream-dlp for opening/copying.
- [x] Keep the local file and download result intact when the handoff is cancelled or the website fails.

**Phase acceptance:** both a newly downloaded file and an older file can enter the workflow. The app reports only the handoff it performed; it does not claim to have detected upload completion.

### S-02A — Experimental automatic website uploads

Implemented at the user's request; [design and validation](docs/automatic-streamable-upload.md).
This automates the website rather than using an upload API. The manual S-02
handoff remains available. S-03/S-04's direct-API blockers are unchanged.

- [x] Explicit automatic-upload action from the selected-video page, shared by completed downloads and existing files.
- [x] Dedicated temporary private browser, background worker, website progress/status, cancellation, and bounded waiting.
- [x] Exactly one submission per attempt; preserve a known but unverified link after ambiguous completion.
- [x] Verify playback in a separate tab before reporting completion; show and copy the returned share link.
- [x] Test unchanged file bytes, inactive inputs, pre-existing links, and playable output with a local fixture.
- [x] Perform an explicitly authorized synthetic guest upload through the production runtime on Windows/Edge.
- [ ] Validate account sign-in, challenges, large/rejected files, and other supported OS/browser combinations.

### S-03 — Conditional direct uploads with a user account

Blocked until R-02 identifies supported upload access.

- [ ] Implement the approved authorization method; use Windows credential storage for persistent secrets and provide disconnect/removal.
- [ ] Add file upload, byte progress, cancellation, bounded retries, processing status, and the final share link.
- [ ] Handle quota/size/duration limits, expired authorization, offline failures, and ambiguous completion without blindly creating duplicates.
- [ ] Keep upload state separate from download state. A failed upload must not turn a successful download into a failed download.
- [ ] Offer **Download then upload** only as an explicit opt-in after the standalone upload flow is reliable.

### S-04 — Conditional uploads without a user account

Blocked until supported anonymous upload is verified separately.

- [ ] Confirm the approved anonymous mechanism, limits, retention, ownership, and deletion capabilities.
- [ ] Explain those constraints before upload; keep cancellation and recovery behavior consistent with account uploads.
- [ ] If anonymous access is unavailable, show a clear account-required or browser-assisted option.

Do not embed a shared stream-dlp Streamable account or secret in the executable as a substitute for anonymous uploads.

### S-05 — Conditional media preparation

- [ ] Use verified Streamable requirements to decide whether a file needs remuxing or conversion.
- [ ] Prefer the original file or lossless remux when compatible. Offer explicit conversion with progress only when required.
- [ ] Preserve the original, support cancellation, check disk space, and remove temporary derivatives safely.

**Direct-upload acceptance, if unblocked:** a supported account upload returns a playable URL after processing; anonymous upload is verified independently; retries, cancellation, credential removal, and temporary-file cleanup pass.

## Release checks and maintenance

- [ ] Add deterministic tests for URL handling, metadata variants, collection selection, platform-specific errors, cookie isolation, and any new persistent records.
- [ ] Keep live platform tests opt-in and separate from deterministic tests. Record environmental failures instead of weakening assertions to hide them.
- [ ] Run `cargo test --locked`, `cargo clippy --all-targets --locked -- -D warnings`, and the existing local pipeline test for relevant engine changes.
- [ ] Verify clean Windows operation, long/Unicode paths, redirected Downloads, cancellation, and portable packaging for each shipped capability.
- [ ] Update the README, in-app help, package description, third-party notices where needed, and supported-platform/browser list.
- [ ] Validate helper updates through the existing verified setup process; record the tested yt-dlp version for each release and provide a helpful update path when extraction breaks.
- [ ] For any future direct uploader, test against mock responses first and perform a small authorized real upload before calling it supported.

## Suggested release boundaries

- **Release A:** shared foundation and the proven browser-session import flow, with manual fallback.
- **Release B:** TikTok and X/Twitter, each independently validated.
- **Release C:** Instagram and Facebook, including their required session handling.
- **Release D:** experimental Other sites mode.
- **Release E:** current/existing-file selection and browser-assisted Streamable sharing.
- **Later, conditional:** direct Streamable uploads, anonymous mode, optional conversion, and download-then-upload automation.

These are scope boundaries, not calendar promises. Estimate work after the three feasibility spikes; unknown browser access and external upload support make firm dates premature. S-01/S-02 can move earlier if sharing is the highest priority.

## Decisions to revisit after the spikes

Recommended defaults are: one pasted URL with automatic platform detection; individual-video downloads; browser imports initiated by the user; sessions kept in memory; no credentials for Other sites; existing-file browsing available without history; and Streamable browser assistance until a supported upload path is proven.

Future scope decisions include which browsers to advertise, whether to build an extension, whether to persist optional history, and whether multi-video posts later gain a download-all action. Additional feature ideas can be appended as separate tasks with dependencies and acceptance criteria.

## Dev release channel (October 21, 2026)

- [x] Add persistent Public/Dev selection in About & tools and show the installed build channel.
- [x] Isolate stable releases from explicitly tagged Dev prereleases; install only on request.
- [x] Allow explicit Dev-to-Public rollback and reject stale background check results.
- [x] Package Dev artifacts separately with clear installer branding and the same app identity.
- [x] Include the selector in Public v0.3.5; v0.3.4 requires a newer installer to regain it.
- [x] Windows acceptance confirmed by the user on October 22, 2026, including channel installation and representative Streamable uploads.

Replaced on October 30, 2026 by a single stable channel with background updates; see the [update rework](docs/plans/2026-10-27-ffc-alignment.md).
