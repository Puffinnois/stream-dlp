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

## Destination guard for nested requests

The checks above cover the pasted link and its HTTP redirects. Everything yt-dlp and FFmpeg
fetch after that (embedded players, iframes, API endpoints, yt-dlp's own redirects, manifests,
segments, keys) goes through the destination guard, `src/engine/guard.rs`. Experiments and their
commands: [research record](../research/other-site-destination-guard.md).

- **What it is.** A forward proxy written with the standard library, bound to `127.0.0.1` on a
  free port. It starts for one inspection or one download of an Other link and stops when the
  helper ends or is cancelled: the listener closes and every open connection is shut down, so
  its threads end too (one still looking up or connecting ends within those limits). Nothing listens while the app is idle, and the named platforms never use it.
- **How the helpers use it.** yt-dlp gets `--proxy http://127.0.0.1:<port>`. yt-dlp passes the
  proxy to FFmpeg in `http_proxy` when it hands FFmpeg a stream. `NO_PROXY` and `no_proxy` are
  removed from yt-dlp's environment, so neither FFmpeg (which honours it) nor anything else
  started by yt-dlp can skip the guard.
- **What it checks.** For each request: `CONNECT host:port` (HTTPS, tunnelled, never decrypted)
  or a plain request with an absolute `http://` URI. Nested requests follow their own rule
  (`guard::destination`), looser than the pasted link's, because media servers and CDNs use
  other ports and bare addresses:
  - the scheme is `http` or `https` (`CONNECT` counts as `https`), without credentials;
  - the port is 80, 443 or 1024–65535 (so `:8080` and `:8443` work; SSH, mail and the other
    services below 1024 do not);
  - the host is a name that passes the link policy's name rule (`public_name`: no local or
    internal ending) or an IPv4 or bracketed IPv6 literal;
  - every address is public (`public_address`, the rule `resolve` uses: one private answer
    refuses). A name is looked up once (`other::checked_addresses`); an IP literal is checked
    as it is.

  The guard connects only to an address it has just checked. No second lookup is ever made,
  which closes DNS rebinding for every request the helpers make, the pasted link included. The
  forwarded `Host` header carries the port when it is not the default. A refused target gets
  `403`, a malformed request `400`, a name that cannot be resolved or reached `502`. The pasted
  link and its redirects keep the stricter link policy (`normalize`: default ports and DNS names
  only).
- **One request per connection.** yt-dlp keeps proxy connections alive and sends later requests,
  to any host, over the same connection. The guard forwards exactly one plain request per
  connection: hop-by-hop headers (`Proxy-Connection`, `Connection` and the headers it names,
  `Keep-Alive`, `Upgrade`, `TE`, `Trailer`, `Expect`, proxy credentials) are dropped, both sides
  are told `Connection: close`, a request body is forwarded only up to its `Content-Length`
  (`Transfer-Encoding` in a request is refused), and the connection is closed after the
  response.
- **Limits.** 32 client connections at once (more get `503`); request heads up to 16 KB and
  response heads up to 64 KB; 15 seconds to send the request head and 15 to connect; a
  connection idle for 60 seconds in both directions is closed. Threads: one listener, one per
  connection and one more per HTTPS tunnel, so at most 65. Only loopback clients are served.
- **Errors.** When a helper fails after the guard refused one of its requests, the user sees why,
  without the host, the port or the link, instead of a network or site error: "This page tried
  to load the video from a local or private network address…" for an address or a local name,
  and "This page tried to load the video from a network port that stream-dlp does not open for
  other sites." for a port. The guard counts the two reasons separately; an address refusal
  wins when both happened.
- **The pre-checks stay.** `resolve` (pasted link and HTTP redirects, with named-platform
  refusal) and `recheck` (before a download) run as before; the guard is an addition.

### FFmpeg and HTTPS

FFmpeg reaches an HTTPS address through a proxy only with its `httpproxy` protocol. Adding
`httpproxy` to the protocol whitelist would open a hole: FFmpeg's HLS demuxer accepts any
protocol whose name starts with `http`, and a playlist segment `httpproxy://<private
address>/…` made FFmpeg connect straight to that address (reproduced). So the whitelist stays
`http,https,tcp,tls,crypto`, and an HTTPS stream that FFmpeg would download fails closed. To keep
FFmpeg out of the way, Other links get `--downloader m3u8:native`: HLS uses yt-dlp's own
downloader, which goes through the guard, and FFmpeg only gets what that downloader hands on.
FFmpeg's protocol refusal ("not on whitelist") has its own message: "This stream needs a download
method that stream-dlp does not allow for other sites."

With `m3u8:native` forced, these HLS variants stayed with yt-dlp's downloader (bundled yt-dlp
2026.08.19): a plain TS playlist, AES-128 encryption with a key file, fMP4 segments with
`EXT-X-MAP`, byte-range TS and byte-range fMP4 in a single file. DASH (`http_dash_segments`)
uses yt-dlp's downloader by default.

These still go to FFmpeg, and over HTTPS they fail with the message above (known limit):

- encryption methods yt-dlp does not decrypt itself, such as `SAMPLE-AES` without a DRM key
  format (yt-dlp: "Unsupported features have been detected; extraction will be delegated to
  ffmpeg");
- live playlists (no `EXT-X-ENDLIST`), which inspection already refuses as live;
- any format whose protocol yt-dlp hands to FFmpeg directly; the protocols allowed for Other
  links (`http`, `https`, `m3u8`, `m3u8_native`, `http_dash_segments(_generator)`) do not, once
  `m3u8` is set to native.

Plain `http://` streams that reach FFmpeg still work, through the guard.

## What is still not enforced (known limits)

- **A system or environment proxy turns the guard off.** When a proxy is set, found the way
  yt-dlp finds one (`ALL_PROXY`, `HTTPS_PROXY` or `HTTP_PROXY` in either case; then on Windows
  the Internet settings `ProxyEnable` and `ProxyServer`, on macOS `scutil --proxy` with HTTP or
  HTTPS enabled), the helpers keep using that proxy and the guard is not started: chaining
  proxies is not supported. The address checks then apply only to the pasted link and its HTTP
  redirects, on the names the app resolves, not to where the proxy connects. PAC files and
  automatic proxy detection are used by neither yt-dlp nor the app, so they do not count.
- **Meta refresh, JavaScript redirects and links inside a page** are still followed, if at all,
  by yt-dlp, without the named-platform refusal of `resolve`, but every request they cause is
  now address-checked by the guard.
- **The redirect probe itself can be rebound.** `resolve` checks a name's addresses, then the
  probe (`net::location`, ureq) looks the name up again to send its one GET, so a name that
  changes its answer in between can make that single anonymous request reach a private address.
  It reads no body and sends no cookies; the helpers' requests go through the guard either way.
- **The probe and yt-dlp may see different answers.** Sites that redirect by user agent, cookies
  or region can send yt-dlp somewhere the probe did not go; the guard still checks where.
- **Other extractors run anonymously but run.** A page embedding a YouTube, TikTok, X, Facebook
  or Instagram player reaches that platform's extractor through Other, without a session and
  without that platform's extra rules (photo posts, share-link checks). Its requests go through
  the guard like any other.
- **HTTPS streams that only FFmpeg can download** fail (above).

The mode stays experimental and never carries credentials: the worst case is an anonymous
request from the user's machine to a public address, the same as opening the page in a browser.

## How to test

- Default suite (`cargo xtask check`): routing, lookalikes, internal names and IP spellings
  (`other::tests`), every address class, redirects (cap, loop, cancellation, private hop,
  platform hop, DNS failure) with an injected lookup and fetch, the redirect probe against a
  local socket (`net::tests`), formats and fixtures for a direct file, an embedded page and an
  HLS/DASH result, live/audio-only/DRM refusal, error mapping (guard refusal and FFmpeg protocol
  refusal included), session isolation, the inspection arguments (`m3u8:native`, no
  `httpproxy`), the guarded command (`--proxy`, `NO_PROXY` removed), the 25-entry cap and the
  file name template (`ytdlp::tests`), and the `other` word in usage and feedback.
- Guard (`guard::tests`, injected lookup and connections): address classes through the proxy,
  `CONNECT` and plain HTTP, one request per connection, private hosts and closed ports with
  their separate reasons, public IPv4 and IPv6 literals and unprivileged ports allowed (with the
  port in `Host`), malformed requests, rebinding (a lookup that answers public, then private, is used once per request and
  only the checked address is connected), head, connection and time limits, shutdown with open
  tunnels, loopback-only clients, and proxy detection.
- `other_sites_local_fixtures_inspect_and_download` (`#[ignore]`d, needs the tools and localhost
  sockets) generates a direct MP4, a page with an embedded `<video>`, an HLS stream and a DASH
  manifest (FFmpeg's dash muxer, video and audio representations), plus HLS playlists whose
  segment is on port 8080 by name and by public IP literal, serves them on localhost
  under the name `fixture.example.test`, and runs inspection and download through the real
  helpers and the guard. A test-only network in the guard (`guard::test_network`) resolves that
  name to a public-looking address and routes it to the fixture server; yt-dlp cannot resolve the
  name itself, so the downloads prove that every request went through the guard. Hostile
  fixtures, pages and HLS playlists whose video or segment is on a name that resolves to
  127.0.0.1 or on a 127.0.0.1 literal, must fail with the guard's message, and a listener on the
  private address must receive no connection. It starts after the pasted-link checks, which
  refuse localhost. Run it by name only:
  `cargo test other_sites_local_fixtures_inspect_and_download -- --ignored`.
- `a_live_public_link_resolves_through_the_destination_policy` (`#[ignore]`d) resolves the
  public link in `STREAM_DLP_OTHER_TEST_URL`. Use a well-known public sample video, and record
  only the behaviour, never the link.
