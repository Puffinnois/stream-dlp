# U-01 — What yt-dlp and FFmpeg send through a proxy

November 16, 2026, Windows 11, bundled yt-dlp 2026.08.19 and FFmpeg
N-127911-gadfe258a6a-20261021. These experiments decided how the destination guard for other
sites works ([reference](../reference/other-sites.md#destination-guard-for-nested-requests)).

## Setup

Synthetic fixtures only, made with the bundled FFmpeg from `lavfi` sources (a 2-second blue
160×90 picture and a 440 Hz tone):

- `clip.mp4` (MPEG-4 Part 2 and AAC), and `page.html` embedding it in a `<video>`;
- `stream.m3u8`: HLS with H.264 (`-f hls -hls_time 1 -hls_list_size 0`);
- `manifest.mpd`: DASH with separate H.264 and AAC representations
  (`-f dash -seg_duration 1 -use_template 1 -use_timeline 1`);
- HLS variants: AES-128 (`-hls_key_info_file`), fMP4 (`-hls_segment_type fmp4`), byte-range TS
  and byte-range fMP4 (`-hls_flags single_file`), the AES playlist rewritten to
  `METHOD=SAMPLE-AES`, and a playlist without `EXT-X-ENDLIST`;
- hostile playlists whose segment is `http://hostile.example/seg.ts`,
  `https://hostile.example/seg.ts`, `httpproxy://127.0.0.1:<sink>/…/seg.ts`, `tcp://…`,
  `tls://…` or `crypto+http://…`.

Three throwaway scripts in the bundled Deno, outside the repository: a fixture server on
127.0.0.1, a logging forward proxy (prints each request line; refuses hosts named `hostile`;
forwards CONNECT and absolute-URI requests), and a sink that logs every connection it receives
on another 127.0.0.1 port. yt-dlp ran with the app's other-site arguments:

```
yt-dlp --ignore-config --no-cookies --proxy http://127.0.0.1:<proxy>
  [--downloader ffmpeg | --downloader m3u8:native]
  --downloader-args "ffmpeg_i:-protocol_whitelist http,https,tcp,tls,crypto[,httpproxy]"
  --ffmpeg-location tools -P <out> http://127.0.0.1:<server>/<fixture>
```

## Results

1. **yt-dlp's own requests all use `--proxy`.** The page fetch, the generic extractor's download
   of the embedded video, native HLS (playlist and segments) and native DASH (manifest, init and
   media segments of both representations) all reached the proxy, even for a 127.0.0.1 target.
2. **Kept-alive proxy connections carry several requests.** A proxy that read only the first
   request of each connection and then piped bytes saw the page but not the embedded video, the
   HLS segment or the DASH segments: yt-dlp sent them on the same connection. With
   `Connection: close` forced on each request, every request appeared separately. The guard
   therefore forwards exactly one request per connection.
3. **FFmpeg as downloader follows yt-dlp's proxy for HTTP.** yt-dlp passes the proxy in
   `http_proxy`; FFmpeg's `http://` requests (playlist and segments, sent as absolute URIs with
   an explicit `:80`) reached the proxy, and a refused segment failed the download.
4. **FFmpeg needs `httpproxy` for HTTPS through a proxy.** With the whitelist
   `http,https,tcp,tls,crypto`, an `https://` segment failed before any connection with
   `Protocol 'httpproxy' not on whitelist`. With `httpproxy` added, FFmpeg sent
   `CONNECT hostile.example:443` to the proxy.
5. **`httpproxy` in the whitelist is a bypass.** A playlist segment
   `httpproxy://127.0.0.1:<sink>/127.0.0.1:<sink>/seg.ts` made FFmpeg connect straight to the
   sink, which logged a `CONNECT`: FFmpeg's HLS demuxer allows every protocol whose name starts
   with `http`. `tcp://`, `tls://` and `crypto+http://` segments were refused by FFmpeg and
   never reached the sink. Decision: `httpproxy` stays off the whitelist, and HTTPS through
   FFmpeg fails closed.
6. **`NO_PROXY`.** With `NO_PROXY=127.0.0.1,localhost,*`, yt-dlp's native downloader still sent
   every request through `--proxy`, but FFmpeg made one direct connection to the sink. Decision:
   remove `NO_PROXY` and `no_proxy` from the helper's environment.
7. **Which HLS streams still reach FFmpeg with `--downloader m3u8:native`.** Native: plain TS,
   AES-128, fMP4 with `EXT-X-MAP`, byte-range TS, byte-range fMP4. Handed to FFmpeg: the
   `SAMPLE-AES` playlist ("Unsupported features have been detected; extraction will be delegated
   to ffmpeg"), and the playlist without `EXT-X-ENDLIST`, which yt-dlp treats as live (inspection
   refuses live results). The `SAMPLE-AES` playlist with an `https://` segment failed with the
   whitelist message above, which the app maps to its own error.
8. **Proxy detection.** The app's probe (`ureq` with `win-system-proxy`) reads `ALL_PROXY`,
   `HTTPS_PROXY` and `HTTP_PROXY` (either case), then on Windows `ProxyEnable` and `ProxyServer`
   under the current user's Internet settings. yt-dlp uses Python's `getproxies`: the
   environment, then the same registry values on Windows, or the system configuration on macOS.
   `ureq` ignores a per-protocol `ProxyServer` (`http=…;https=…`) that Python uses, so the guard
   reads the registry itself through `reg query`, and `scutil --proxy` on macOS.

## Through the app

The `#[ignore]`d `other_sites_local_fixtures_inspect_and_download` repeats the main cases through
the app's own code and the real guard, with the fixtures served under a name only the guard's
test network can resolve. On November 16, 2026 on Windows it passed: the direct file, the page,
HLS and DASH (merged video and audio) were saved, and two hostile pages and two hostile
playlists (private name, private IP literal) failed with the guard's message while the private
listener received no connection. After the review the same day, nested requests may also use a
public IP literal and ports 1024–65535: two playlists whose segment is on port 8080, by name and
by public IP literal, were saved too.
