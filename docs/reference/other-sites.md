# Other sites (experimental)

How stream-dlp handles a link that belongs to none of its named platforms (roadmap U-01, with the
routing and destination policy of F-01). The code is `src/engine/sources/other.rs`; the user's
view is in the [guide](../guide.md#other-sites-experimental).

## Routing

`parse_link` (`src/engine/sources/mod.rs`) decides in this order:

1. A host in a platform's list (`HOSTS` in `youtube.rs`, `tiktok.rs`, `x.rs`, `facebook.rs`,
   `instagram.rs`) goes to that platform's `normalize`, with its own errors. A playlist, profile
   or photo post there is refused as before and never retried as Other.
2. Any other address of a named platform (its domain or a subdomain: `studio.youtube.com`,
   `web.facebook.com`, `mobile.x.com`, plus `youtube-nocookie.com`, `fb.com`, `instagr.am`) is
   refused as that platform's: "This … address is not supported".
3. A lookalike, where a platform's domain appears as whole labels before the end
   (`youtube.com.example.test`), is refused.
4. Everything else becomes `Source::Other` if it passes the link policy below.

Shorteners such as `t.co` or `fb.watch` are not named: they are Other links, and the redirect
check below refuses them when they lead to a named platform.

## Link policy

`other::normalize` accepts only:

- `http` or `https`, without user name, password or a non-default port;
- a DNS name, never an IP literal. The `url` crate turns decimal, hex, octal and short IPv4
  spellings (`2130706433`, `0x7f.1`, `0177.0.0.1`, `127.1`) and bracketed IPv6 into IP hosts, so
  they are all refused;
- at least two labels, each 1–63 characters of `a–z`, `0–9`, `-` or `_` (IDNs arrive as
  punycode), not starting or ending with `-`, and a top level that is not all digits;
- no local or internal ending: `localhost`, `local`, `localdomain`, `internal`, `intranet`, `lan`,
  `home`, `corp`, `private`, `arpa` (so `home.arpa` and reverse-DNS names), `invalid`, `onion`, and
  `test` outside `cfg(test)` (tests use `example.test` fixtures).

The canonical link keeps the query string, because many sites need it, and drops only the
fragment. No tracking parameters are removed for other sites.

## Destination policy

Before any helper runs, `other::resolve` follows the link itself:

1. Check the link policy, then look up the host with the standard library
   (`ToSocketAddrs`). Every returned address must be public: not loopback, private (RFC 1918),
   link-local, shared/CGNAT (`100.64.0.0/10`), `0.0.0.0/8`, IETF (`192.0.0.0/24`), benchmarking
   (`198.18.0.0/15`), documentation, multicast or reserved (`240.0.0.0/4`, broadcast); for IPv6
   not loopback, unspecified, unique-local (`fc00::/7`), link-local, site-local, multicast,
   documentation or discard, and the IPv4 address inside an IPv4-mapped, IPv4-compatible, NAT64
   (`64:ff9b::/96`) or 6to4 (`2002::/16`) address must be public too. One private answer refuses
   the link, so a name that resolves to both kinds is refused.
2. Send one anonymous GET (`net::location`: no cookies, the app's user agent, the system's
   certificates and proxy, redirects not followed, the body not read). A 301/302/303/307/308
   gives the next hop; any other answer, error statuses included, ends the chain. A site may
   refuse this client and still answer yt-dlp, so a 403 here is not an error.
3. A redirect target is parsed again by `parse_link`. If it is Other, steps 1–2 repeat for it. If
   it is a named platform's link it is refused ("This link leads to YouTube…"): handing it to the
   platform would let a link pasted as Other end up with a platform session at download time, so
   the user pastes the platform link instead. A target that fails the link policy is refused.
4. At most 5 redirects, a repeated URL ends the chain, and the whole resolution has 15 seconds.

yt-dlp then gets the last URL of the chain, not the pasted one, so it does not repeat the
redirects. Right before a download, `other::recheck` looks the host up again and applies the
address check, as DNS answers can change between inspection and download.

## No session, ever

- `Link::takes_session` is false for `Source::Other`, so `command_for` never writes a cookie file
  for it, whatever sessions are loaded (test `other_links_never_receive_any_platform_session`).
- yt-dlp also gets `--no-cookies --no-cookies-from-browser`, and `--ignore-config` keeps a user
  configuration from adding cookies, a netrc or other options.
- `SessionCookies::parse_for` accepts no domain for Other, so no Other session can exist.
- The UI hides the session link, the session panel (import, export guide, Firefox import) and
  **Connect session / retry help** for Other links. `needs_session` is false for every Other
  error, so automatic Firefox refresh never starts. A sign-in error says signing in is not
  supported for other sites.

## yt-dlp invocation and limits

- No `--use-extractors`: yt-dlp's other site extractors and the generic one may answer. A named
  platform's extractor can still answer (for example a page embedding a YouTube player); it runs
  anonymously, with the same limits.
- Inspection: `--playlist-items 1:25`, the usual 150-second time limit, and a 32 MB limit on
  each output stream (new: the other platforms' inspection still has no output limit). A page
  with several videos offers at most 25 in the existing chooser; entries that are not a
  downloadable video are skipped, and when none is left the first reason is shown. Nested
  collections are not opened.
- Download: only the chosen entry, by `--playlist-items N`, plus `--match-filter id = '…'` when
  the ID is safe to quote (`A–Z a–z 0–9 _ - .`). A whole collection is never downloaded.
- Formats: only `http`, `https`, `m3u8`, `m3u8_native`, `http_dash_segments(_generator)`
  protocols are kept; RTMP, RTSP, MMS, F4M and the rest are dropped. A format without codec
  details in a video container (`mp4`, `webm`, `mkv`…) counts as a video, as the generic
  extractor reports direct files that way.
- Refused: `live_status` `is_live` or `is_upcoming` (and `is_live: true`), audio-only results,
  DRM-only results, results without a video.
- File names: `%(extractor_key).30s %(webpage_url_domain).60s %(title).100s [%(id)s]
  [%(format_id)s].%(ext)s`. The extractor names the site; the page's domain is added because the
  generic extractor takes IDs from file names (`video`, `index`) that many sites share, and the
  no-overwrite rule would otherwise reuse an unrelated file.

## Errors

`other::explain` maps yt-dlp's output to: unsupported site or page ("Unsupported URL"), no
video, DRM, sign-in needed (including expired-cookie wording, as no session is ever sent),
unavailable (404/410), rate-limited (429), network, timeout, refused (403) and helper failure.
Each has its own message (`EngineError`'s `Display`), and none repeats the link, the host, the
extractor or helper output. Usage reports and feedback carry only the word `other`.

## What is and is not enforced on nested requests

The checks above cover the pasted link and its HTTP redirects. What yt-dlp and FFmpeg fetch
after that is only partly under the app's control. Validated with the bundled yt-dlp 2026.08.19
and a local fixture server:

Enforced:

- `file://` URLs: yt-dlp refuses them unless `--enable-file-urls` is given, which the app never
  does. An HLS playlist naming a `file:///` segment fails with yt-dlp's native downloader.
- FFmpeg as downloader: yt-dlp hands FFmpeg some streams (forced, or HLS features its own
  downloader lacks). The app passes `--downloader-args "ffmpeg_i:-protocol_whitelist
  http,https,tcp,tls,crypto"`, so FFmpeg cannot open local files or other protocols from a
  remote playlist; FFmpeg's HLS demuxer also refuses segment names with unexpected extensions.
  A normal HLS stream still downloads through FFmpeg with this whitelist.
- Non-HTTP formats are dropped before download (above), so RTMP/RTSP/MMS are never attempted.
- Merging and fixups only read the local files yt-dlp just wrote.

Not enforced (known limits):

- **yt-dlp's own requests are not restricted by destination.** yt-dlp follows redirects again,
  fetches embedded players, iframes, API endpoints, manifests, segments and keys, and none of
  these is checked against the address policy. A public page that embeds or redirects to a
  private address (for example a router at `192.168.1.1`), or a manifest whose segments point
  there, makes yt-dlp request it from the user's machine. yt-dlp has no option to restrict
  destinations, and doing it would need a filtering proxy in front of every helper request,
  which the app does not have.
- **DNS can change after the check (rebinding).** The app resolves the host, then yt-dlp
  resolves it again on its own. A name that answers a public address first and a private one
  later passes the check. `recheck` narrows the window before a download but cannot close it.
- **The redirect probe sees only HTTP redirects.** Meta refresh, JavaScript redirects and
  links inside the page are followed, if at all, by yt-dlp unchecked.
- **The probe and yt-dlp may see different answers.** Sites that redirect by user agent,
  cookies or region can send yt-dlp somewhere the probe did not go.
- **Other extractors run anonymously but run.** A page embedding a YouTube, TikTok, X, Facebook
  or Instagram player reaches that platform's extractor through Other, without a session and
  without that platform's extra rules (photo posts, share-link checks).
- **Proxies.** The probe and yt-dlp use the system's proxy settings; when a proxy is set, the
  address check applies to the name the app resolves, not to where the proxy connects.

These limits are why the mode is labelled experimental and why it never carries credentials:
the worst case is an anonymous request from the user's machine, the same as opening the page in
a browser, not a leaked session.

## How to test

- Default suite (`cargo xtask check`): routing, lookalikes, internal names and IP spellings
  (`other::tests`), every address class, redirects (cap, loop, cancellation, private hop,
  platform hop, DNS failure) with an injected lookup and fetch, the redirect probe against a
  local socket (`net::tests`), formats and fixtures for a direct file, an embedded page and an
  HLS/DASH result, live/audio-only/DRM refusal, error mapping, session isolation, the inspection
  arguments, the 25-entry cap and the file name template (`ytdlp::tests`), and the `other` word
  in usage and feedback.
- `other_sites_local_fixtures_inspect_and_download` (`#[ignore]`d, needs the tools and localhost
  sockets) generates a direct MP4, a page with an embedded `<video>` and an HLS stream, serves
  them on localhost and runs inspection and download through the real helpers. It starts after
  the pasted-link checks, which refuse localhost; there is no loopback allowance in the policy.
  Run it by name only:
  `cargo test other_sites_local_fixtures_inspect_and_download -- --ignored`.
- `a_live_public_link_resolves_through_the_destination_policy` (`#[ignore]`d) resolves the
  public link in `STREAM_DLP_OTHER_TEST_URL`. Use a well-known public sample video, and record
  only the behaviour, never the link.
