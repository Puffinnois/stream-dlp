//! Other sites (experimental): a public HTTP(S) link on no named platform, tried with yt-dlp's
//! remaining site extractors and its generic fallback. Never with a session. Before any helper
//! runs, the link and every redirect it makes must point at a public DNS name whose addresses are
//! all public. See `docs/reference/other-sites.md`.
use super::{EngineError, Link, Source, Video};
use std::{
    collections::HashSet,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use url::Url;

/// The most entries inspection reads from a page that holds several videos.
pub(in crate::engine) const MAX_ENTRIES: usize = 25;
/// The most redirects followed before a helper opens the link.
const MAX_REDIRECTS: usize = 5;
const RESOLVE_TIME: Duration = Duration::from_secs(15);

const INVALID: &str = "Paste a public http or https link to a video page or video file.";
const NOT_PUBLIC: &str = "This link points to a local or private network address. stream-dlp only opens public websites.";
const REDIRECT: &str = "This link redirects to an address stream-dlp does not open. Open it in your browser and copy the video page's link.";
const TOO_MANY_REDIRECTS: &str =
    "This link redirects too many times. Open it in your browser and copy the video page's link.";
pub(super) const UNSUPPORTED_SITE: &str = "This site or page is not supported. Other sites are experimental: stream-dlp tries the download tools' site readers and their fallback page reader, which do not work everywhere.";
const AUDIO_ONLY: &str = "This page offers audio only. stream-dlp downloads videos; audio-only results are not supported.";
const LIVE: &str =
    "This is a live or upcoming stream. Try again after the broadcast has ended and been saved.";
/// A request the download tools made after the pasted link was refused by the destination guard.
pub(in crate::engine) const NESTED_NOT_PUBLIC: &str = "This page tried to load the video from a local or private network address. stream-dlp only opens public websites.";
/// A request the download tools made after the pasted link was refused for its port.
pub(in crate::engine) const NESTED_PORT: &str = "This page tried to load the video from a network port that stream-dlp does not open for other sites.";
/// yt-dlp handed a secure stream to FFmpeg, which cannot reach it through the destination guard
/// without a protocol that would let a playlist bypass the guard.
const FFMPEG_METHOD: &str =
    "This stream needs a download method that stream-dlp does not allow for other sites.";
pub(in crate::engine) const HELPER: &str = "The download tools could not read this page. Other sites are experimental; check that the video plays in your browser, then update the tools in Settings and retry.";

/// Name endings that are never public websites. `.test` is allowed in tests, for fixtures.
const BLOCKED_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "localdomain",
    "internal",
    "intranet",
    "lan",
    "home",
    "corp",
    "private",
    "arpa",
    "invalid",
    "onion",
    #[cfg(not(test))]
    "test",
];

/// Protocols a video from another site may use; anything else (RTMP, RTSP, MMS, F4M…) is
/// dropped before it reaches a helper.
const PROTOCOLS: &[&str] = &[
    "http",
    "https",
    "m3u8",
    "m3u8_native",
    "http_dash_segments",
    "http_dash_segments_generator",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "webm", "mkv", "mov", "flv", "ts", "3gp"];
const AUDIO_EXTENSIONS: &[&str] = &["m4a", "mp3", "aac", "opus", "ogg", "oga", "wav", "flac"];

/// The canonical form of an Other link: the URL without its fragment. The query stays, because
/// sites need it, and no tracking parameters are removed.
pub(in crate::engine) fn normalize(url: &Url) -> Result<String, &'static str> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(INVALID);
    }
    let Some(url::Host::Domain(host)) = url.host() else {
        return Err(NOT_PUBLIC);
    };
    if !public_name(host) {
        return Err(NOT_PUBLIC);
    }
    let mut canonical = url.clone();
    canonical.set_fragment(None);
    Ok(canonical.into())
}

/// Whether `host` (already lowercased and in punycode, as `url` leaves it) looks like a public
/// DNS name: at least two labels, a non-numeric top level, and no local or internal ending.
pub(in crate::engine) fn public_name(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    let labels: Vec<_> = host.split('.').collect();
    host.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b))
        })
        && !labels
            .last()
            .is_some_and(|tld| tld.bytes().all(|b| b.is_ascii_digit()))
        && !BLOCKED_SUFFIXES.iter().any(|suffix| {
            host == *suffix
                || host
                    .strip_suffix(suffix)
                    .is_some_and(|rest| rest.ends_with('.'))
        })
}

/// Whether a helper may connect to `ip`: not loopback, private, link-local, shared (CGNAT),
/// unique-local, unspecified, multicast, reserved or documentation space, including the IPv4
/// forms an IPv6 address can carry.
pub(in crate::engine) fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => public_v6(ip),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || ip.is_documentation()
        || (a == 198 && (b == 18 || b == 19))
        || ip.is_multicast()
        || a >= 240)
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let embedded = |high: u16, low: u16| {
        public_v4(Ipv4Addr::new(
            (high >> 8) as u8,
            high as u8,
            (low >> 8) as u8,
            low as u8,
        ))
    };
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    // IPv4-compatible (::a.b.c.d, deprecated), NAT64 (64:ff9b::/96) and 6to4 (2002::/16) carry
    // an IPv4 address that the connection may really go to.
    if s[..6] == [0; 6] && !ip.is_loopback() && !ip.is_unspecified() {
        return embedded(s[6], s[7]);
    }
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return embedded(s[6], s[7]);
    }
    if s[0] == 0x2002 {
        return embedded(s[1], s[2]);
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // unique-local
        || (s[0] & 0xffc0) == 0xfe80 // link-local
        || (s[0] & 0xffc0) == 0xfec0 // site-local, deprecated
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || s[..4] == [0x100, 0, 0, 0]) // discard
}

/// Looks up `url`'s host once and returns its addresses, refusing it (`Unsupported`) unless every
/// one is public. A failed or empty lookup is a network error. The destination guard connects
/// only to addresses this returned, so a later, different DNS answer is never used.
pub(in crate::engine) fn checked_addresses(
    url: &Url,
    lookup: &mut impl FnMut(&str, u16) -> io::Result<Vec<IpAddr>>,
) -> Result<Vec<IpAddr>, EngineError> {
    let host = url
        .host_str()
        .ok_or(EngineError::Unsupported(INVALID.into()))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses = lookup(host, port).map_err(|_| EngineError::Network(Source::Other))?;
    if addresses.is_empty() {
        return Err(EngineError::Network(Source::Other));
    }
    if !addresses.iter().copied().all(public_address) {
        return Err(EngineError::Unsupported(NOT_PUBLIC.into()));
    }
    Ok(addresses)
}

fn check_destination(
    url: &Url,
    lookup: &mut impl FnMut(&str, u16) -> io::Result<Vec<IpAddr>>,
) -> Result<(), EngineError> {
    checked_addresses(url, lookup).map(drop)
}

pub(in crate::engine) fn system_lookup(host: &str, port: u16) -> io::Result<Vec<IpAddr>> {
    Ok((host, port)
        .to_socket_addrs()?
        .map(|address| address.ip())
        .collect())
}

/// Checks the destination of an Other link again right before a download, as DNS answers can
/// change after inspection.
pub(in crate::engine) fn recheck(link: &str) -> Result<(), EngineError> {
    let url = Url::parse(link).map_err(|_| EngineError::Unsupported(INVALID.into()))?;
    normalize(&url).map_err(|message| EngineError::Unsupported(message.into()))?;
    check_destination(&url, &mut system_lookup)
}

/// One answer while following a link.
pub(super) enum Hop {
    /// Anything but a redirect: this is the page to give the helper.
    Final,
    /// A redirect to this `Location`, relative or absolute.
    Redirect(String),
}

/// Follows the link's redirects anonymously, checking the link and its addresses before each
/// request. `None` when cancelled.
pub(super) fn resolve(link: &str, cancel: &AtomicBool) -> Result<Option<Link>, EngineError> {
    let deadline = Instant::now() + RESOLVE_TIME;
    follow(link, cancel, &mut system_lookup, &mut |url| {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(EngineError::TimedOut(Source::Other))?;
        match crate::net::location(url, remaining) {
            Ok(Some(location)) => Ok(Hop::Redirect(location)),
            Ok(None) => Ok(Hop::Final),
            Err(crate::net::ProbeError::TimedOut) => Err(EngineError::TimedOut(Source::Other)),
            Err(crate::net::ProbeError::Failed) => Err(EngineError::Network(Source::Other)),
        }
    })
}

fn follow(
    link: &str,
    cancel: &AtomicBool,
    lookup: &mut impl FnMut(&str, u16) -> io::Result<Vec<IpAddr>>,
    fetch: &mut impl FnMut(&str) -> Result<Hop, EngineError>,
) -> Result<Option<Link>, EngineError> {
    let mut current = Url::parse(link).map_err(|_| EngineError::Unsupported(INVALID.into()))?;
    normalize(&current).map_err(|message| EngineError::Unsupported(message.into()))?;
    let mut visited = HashSet::new();
    for _ in 0..=MAX_REDIRECTS {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if !visited.insert(current.to_string()) {
            break;
        }
        check_destination(&current, lookup)?;
        let hop = fetch(current.as_str())?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let Hop::Redirect(location) = hop else {
            return Ok(Some(Link {
                source: Source::Other,
                url: normalize(&current).map_err(|m| EngineError::Unsupported(m.into()))?,
            }));
        };
        let next = current
            .join(&location)
            .map_err(|_| EngineError::Unsupported(REDIRECT.into()))?;
        // A redirect onto a named platform is refused rather than handed to that platform, so
        // a link pasted as Other never ends up with a platform session.
        match super::parse_link(next.as_str()) {
            Ok(link) if link.source == Source::Other => {
                current =
                    Url::parse(&link.url).map_err(|_| EngineError::Unsupported(REDIRECT.into()))?;
            }
            Ok(link) => {
                return Err(EngineError::Unsupported(format!(
                    "This link leads to {}. Open it in your browser and paste the {} link itself.",
                    link.source.label(),
                    link.source.label()
                )));
            }
            Err(_) => return Err(EngineError::Unsupported(REDIRECT.into())),
        }
    }
    Err(EngineError::Unsupported(TOO_MANY_REDIRECTS.into()))
}

/// Marks formats whose codecs the page did not report but whose container is a video, keeps
/// only the protocols allowed for other sites, and refuses live and audio-only results.
pub(in crate::engine) fn prepare(video: &mut Video) -> Result<(), EngineError> {
    if matches!(
        video.live_status.as_deref(),
        Some("is_live" | "is_upcoming")
    ) {
        return Err(EngineError::Unsupported(LIVE.into()));
    }
    video.formats.retain(|f| {
        f.protocol
            .as_deref()
            .is_some_and(|p| p.split('+').all(|part| PROTOCOLS.contains(&part)))
    });
    for f in &mut video.formats {
        f.direct_video = f.vcodec.is_none()
            && f.acodec.as_deref() != Some("none")
            && VIDEO_EXTENSIONS.contains(&f.ext.as_str());
    }
    if !video.formats.is_empty()
        && !video.formats.iter().any(|f| f.video())
        && video.formats.iter().any(|f| {
            f.audio() || (f.vcodec.is_none() && AUDIO_EXTENSIONS.contains(&f.ext.as_str()))
        })
    {
        return Err(EngineError::Unsupported(AUDIO_ONLY.into()));
    }
    Ok(())
}

pub(super) fn check(video: &mut Video) -> Result<(), EngineError> {
    if !video.formats.iter().any(|f| f.video()) {
        return Err(EngineError::MissingVideo(Source::Other));
    }
    Ok(())
}

/// An entry ID that is safe inside a yt-dlp `--match-filter` string.
pub(in crate::engine) fn filter_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}

/// Explains a failed yt-dlp run for another site. Nothing from `errors` is repeated.
pub(super) fn explain(lower: &str) -> EngineError {
    let has = |patterns: &[&str]| patterns.iter().any(|pattern| lower.contains(pattern));
    let source = Source::Other;
    if lower.contains("not on whitelist") {
        // FFmpeg refused a protocol outside `-protocol_whitelist`: `httpproxy` for a secure
        // stream behind the destination guard, or a playlist naming a local file or another
        // scheme.
        EngineError::Unsupported(FFMPEG_METHOD.into())
    } else if has(&["drm protected", "drm-protected", "drm content"]) {
        EngineError::Drm(source)
    } else if has(&["unsupported url", "is not a valid url"]) {
        EngineError::Unsupported(UNSUPPORTED_SITE.into())
    } else if has(&[
        "http error 429",
        "http 429",
        "too many requests",
        "rate limit",
    ]) {
        EngineError::RateLimited(source)
    } else if has(&[
        "sign in",
        "log in",
        "login",
        "requires authentication",
        "registered users",
        "subscribers only",
        "members only",
        "premium",
        "cookies",
        "http error 401",
    ]) {
        EngineError::SignInNeeded(source)
    } else if has(&["no video", "no formats found", "no downloadable video"])
        || (lower.contains("does not contain") && lower.contains("video"))
    {
        EngineError::MissingVideo(source)
    } else if lower.contains("timed out") {
        EngineError::TimedOut(source)
    } else if has(&[
        "connection",
        "resolve",
        "getaddrinfo failed",
        "name or service not known",
        "<urlopen error",
        "ssl",
        "certificate",
    ]) {
        EngineError::Network(source)
    } else if has(&[
        "not available",
        "unavailable",
        "removed",
        "http error 404",
        "http error 410",
        "http 404",
    ]) {
        EngineError::Unavailable(source)
    } else if has(&["blocked", "http error 403", "http 403", "forbidden"]) {
        EngineError::Restricted(source)
    } else {
        EngineError::Helper(HELPER.into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse_link;
    use super::*;
    use crate::engine::media::parse_for;

    fn other(link: &str) -> Link {
        Link {
            source: Source::Other,
            url: link.into(),
        }
    }

    #[test]
    fn unknown_public_hosts_become_other_links_with_query_and_without_fragment() {
        for (input, canonical) in [
            (
                "https://video.example.test/watch?id=7&t=3#comments",
                "https://video.example.test/watch?id=7&t=3",
            ),
            (
                "media.example.test/clip.mp4",
                "https://media.example.test/clip.mp4",
            ),
            (
                "http://Example.Test/path/?utm_source=x",
                "http://example.test/path/?utm_source=x",
            ),
            (
                "https://xn--bcher-kva.example.test/v",
                "https://xn--bcher-kva.example.test/v",
            ),
            (
                "https://bücher.example.test/v",
                "https://xn--bcher-kva.example.test/v",
            ),
            (
                "https://notyoutube.example.test/watch?v=1",
                "https://notyoutube.example.test/watch?v=1",
            ),
            ("https://example.test:443/v", "https://example.test/v"),
        ] {
            let link = parse_link(input).unwrap();
            assert_eq!(link.source, Source::Other, "{input}");
            assert_eq!(link.url, canonical, "{input}");
            assert!(!link.takes_session(), "{input}");
        }
    }

    #[test]
    fn named_platform_hosts_never_fall_through_to_other() {
        for (input, source) in [
            ("https://www.youtube.com/playlist?list=abc", Source::YouTube),
            ("https://youtu.be/a", Source::YouTube),
            ("https://studio.youtube.com/video/abc", Source::YouTube),
            (
                "https://www.youtube-nocookie.com/embed/BaW_jenozKc",
                Source::YouTube,
            ),
            ("https://www.tiktok.com/@creator/photo/123", Source::TikTok),
            ("https://www.tiktok.com/@creator", Source::TikTok),
            ("https://shop.tiktok.com/view/1", Source::TikTok),
            ("https://x.com/example", Source::X),
            ("https://mobile.x.com/example/status/1", Source::X),
            ("https://web.facebook.com/watch/?v=1", Source::Facebook),
            ("https://www.facebook.com/groups/example", Source::Facebook),
            ("https://instagram.com/example", Source::Instagram),
            ("https://help.instagram.com/p/Example", Source::Instagram),
        ] {
            let Err(EngineError::Unsupported(message)) = parse_link(input) else {
                panic!("{input} must stay a rejected {source:?} link");
            };
            assert!(!message.contains("Other sites"), "{input}: {message}");
        }
    }

    #[test]
    fn lookalikes_internal_names_and_ip_literals_are_rejected() {
        for input in [
            // Lookalikes of named platforms.
            "https://youtube.com.example.test/watch?v=BaW_jenozKc",
            "https://www.tiktok.com.example.test/@c/video/1",
            "https://login.facebook.com.example.test/reel/1",
            "https://instagram.com.example.test/p/Example",
            "https://x.com.example.test/a/status/1",
            // Not public names.
            "https://localhost/video.mp4",
            "https://localhost./video.mp4",
            "https://media.localhost/video.mp4",
            "https://nas.local/video.mp4",
            "https://nas.LOCAL./video.mp4",
            "https://app.internal/video.mp4",
            "https://box.lan/video.mp4",
            "https://router.home.arpa/",
            "https://1.0.0.127.in-addr.arpa/",
            "https://hidden.onion/video.mp4",
            "https://intranet/video.mp4",
            "https://nas/video.mp4",
            "https://under_score/video.mp4",
            "https://-bad.example.test/",
            // IP literals in every spelling the URL parser accepts.
            "https://127.0.0.1/video.mp4",
            "https://127.1/video.mp4",
            "https://2130706433/video.mp4",
            "https://0x7f000001/video.mp4",
            "https://0x7f.0.0.1/video.mp4",
            "https://0177.0.0.1/video.mp4",
            "https://127.0.0.1./video.mp4",
            "https://93.184.215.14/video.mp4",
            "https://[::1]/video.mp4",
            "https://[::ffff:127.0.0.1]/video.mp4",
            "https://[2001:4860::8888]/video.mp4",
            // Schemes, credentials and ports.
            "ftp://example.test/video.mp4",
            "file:///C:/video.mp4",
            "https://user:pass@example.test/video.mp4",
            "https://user@example.test/video.mp4",
            "https://example.test:8443/video.mp4",
            "http://example.test:443/video.mp4",
            "https://exa mple.test/",
        ] {
            assert!(
                matches!(parse_link(input), Err(EngineError::Unsupported(_))),
                "{input}"
            );
        }
    }

    #[test]
    fn address_policy_refuses_every_non_public_class() {
        for blocked in [
            "127.0.0.1",
            "127.255.0.9",
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.254",
            "192.0.0.8",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "100::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:100.64.0.1",
            "::127.0.0.1",
            "64:ff9b::a00:1",
            "2002:c0a8:101::1",
        ] {
            assert!(!public_address(blocked.parse().unwrap()), "{blocked}");
        }
        for public in [
            "93.184.215.14",
            "8.8.8.8",
            "100.63.255.255",
            "100.128.0.1",
            "172.32.0.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::1",
        ] {
            assert!(public_address(public.parse().unwrap()), "{public}");
        }
    }

    fn public_lookup(_: &str, _: u16) -> io::Result<Vec<IpAddr>> {
        Ok(vec!["93.184.215.14".parse().unwrap()])
    }

    #[test]
    fn redirects_are_checked_hop_by_hop_and_capped() {
        let cancel = AtomicBool::new(false);
        // A chain within the cap ends on the last page, query kept.
        let mut hops = 0;
        let link = follow(
            "https://start.example.test/a",
            &cancel,
            &mut public_lookup,
            &mut |url| {
                hops += 1;
                Ok(if url.ends_with("/a") {
                    Hop::Redirect("/b?x=1#frag".into())
                } else if url.contains("/b") {
                    Hop::Redirect("https://cdn.example.test/video.mp4?sig=2".into())
                } else {
                    Hop::Final
                })
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(hops, 3);
        assert_eq!(link.url, "https://cdn.example.test/video.mp4?sig=2");
        assert_eq!(link.source, Source::Other);
        // Six redirects in a row are too many.
        let mut count = 0;
        let error = follow(
            "https://start.example.test/0",
            &cancel,
            &mut public_lookup,
            &mut |_| {
                count += 1;
                Ok(Hop::Redirect(format!("/{count}")))
            },
        )
        .unwrap_err();
        assert_eq!(error, EngineError::Unsupported(TOO_MANY_REDIRECTS.into()));
        assert_eq!(count, MAX_REDIRECTS + 1);
        // A loop stops too.
        assert!(
            follow(
                "https://a.example.test/",
                &cancel,
                &mut public_lookup,
                &mut |_| Ok(Hop::Redirect("https://a.example.test/".into()))
            )
            .is_err()
        );
        // Cancelled before the first request.
        assert!(
            follow(
                "https://a.example.test/",
                &AtomicBool::new(true),
                &mut public_lookup,
                &mut |_| panic!("no request after cancel")
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn redirects_to_private_or_platform_destinations_are_refused() {
        let cancel = AtomicBool::new(false);
        for target in [
            "http://127.0.0.1/",
            "http://[::1]/",
            "http://localhost/",
            "http://nas.local/",
            "file:///C:/secret",
            "https://user@other.example.test/",
            "https://other.example.test:8080/",
            "https://youtube.com.example.test/",
        ] {
            let error = follow(
                "https://a.example.test/",
                &cancel,
                &mut public_lookup,
                &mut |url| {
                    Ok(if url.starts_with("https://a.") {
                        Hop::Redirect(target.into())
                    } else {
                        panic!("{target} must not be requested")
                    })
                },
            )
            .unwrap_err();
            assert_eq!(error, EngineError::Unsupported(REDIRECT.into()), "{target}");
        }
        // A public name that resolves to a private address is refused before it is requested.
        let mut lookup = |host: &str, _: u16| -> io::Result<Vec<IpAddr>> {
            Ok(if host == "rebind.example.test" {
                vec![
                    "93.184.215.14".parse().unwrap(),
                    "10.0.0.5".parse().unwrap(),
                ]
            } else {
                vec!["93.184.215.14".parse().unwrap()]
            })
        };
        let error = follow(
            "https://a.example.test/",
            &cancel,
            &mut lookup,
            &mut |url| {
                assert!(!url.contains("rebind"), "private destination was requested");
                Ok(Hop::Redirect("https://rebind.example.test/".into()))
            },
        )
        .unwrap_err();
        assert_eq!(error, EngineError::Unsupported(NOT_PUBLIC.into()));
        // And the pasted link itself.
        assert!(
            follow(
                "https://rebind.example.test/",
                &cancel,
                &mut lookup,
                &mut |_| { panic!("no request to a private address") }
            )
            .is_err()
        );
        // A redirect onto a named platform is refused, so no session can follow it.
        for target in [
            "https://www.youtube.com/watch?v=BaW_jenozKc",
            "https://www.tiktok.com/@creator/video/123",
            "https://x.com/example/status/1",
        ] {
            let Err(EngineError::Unsupported(message)) = follow(
                "https://short.example.test/x",
                &cancel,
                &mut public_lookup,
                &mut |url| {
                    Ok(if url.contains("short.") {
                        Hop::Redirect(target.into())
                    } else {
                        panic!("platform page must not be requested")
                    })
                },
            ) else {
                panic!("{target}");
            };
            assert!(message.contains("paste the"), "{message}");
        }
        // DNS failure is a network error, not a crash or a pass.
        assert_eq!(
            follow(
                "https://gone.example.test/",
                &cancel,
                &mut |_, _| Err(io::Error::other("no such host")),
                &mut |_| panic!("no request without an address")
            )
            .unwrap_err(),
            EngineError::Network(Source::Other)
        );
    }

    #[test]
    fn direct_file_embedded_page_and_manifest_fixtures_offer_video() {
        // A direct video file, as the generic extractor reports it: no codecs, a video container.
        let direct = serde_json::json!({
            "id": "clip", "title": "clip", "extractor_key": "Generic", "_type": "video",
            "formats": [{"format_id": "0", "url": "https://media.example.test/clip.mp4",
                "protocol": "https", "ext": "mp4"}]
        });
        let link = other("https://media.example.test/clip.mp4");
        let video = parse_for(&direct.to_string(), &link).unwrap();
        assert_eq!(video.formats.len(), 1);
        assert!(video.formats[0].video());
        assert_eq!(video.formats[0].selector(), "0");
        assert_eq!(video.source, Source::Other);
        // A page with an embedded <video>: progressive files with codec details.
        let page = serde_json::json!({
            "id": "page-7", "title": "Sample page", "extractor_key": "Generic",
            "formats": [
                {"format_id": "low", "protocol": "https", "ext": "webm", "vcodec": "vp9",
                    "acodec": "opus", "height": 360},
                {"format_id": "high", "protocol": "https", "ext": "mp4", "vcodec": "avc1",
                    "acodec": "mp4a", "height": 720},
                {"format_id": "rtmp", "protocol": "rtmp", "ext": "flv", "vcodec": "h264",
                    "acodec": "aac", "height": 1080}
            ]
        });
        let video =
            parse_for(&page.to_string(), &other("https://www.example.test/post/7")).unwrap();
        let ids: Vec<_> = video.formats.iter().map(|f| f.format_id.as_str()).collect();
        assert_eq!(ids, ["high", "low"], "RTMP must be dropped");
        // An HLS and DASH manifest result: separate video and audio renditions.
        let manifest = serde_json::json!({
            "id": "stream", "title": "stream", "extractor_key": "Generic",
            "live_status": "not_live",
            "formats": [
                {"format_id": "hls-720", "protocol": "m3u8_native", "ext": "mp4",
                    "vcodec": "avc1.64001f", "acodec": "none", "height": 720},
                {"format_id": "hls-audio", "protocol": "m3u8_native", "ext": "mp4",
                    "vcodec": "none", "acodec": "mp4a.40.2"},
                {"format_id": "dash-v1", "protocol": "http_dash_segments", "ext": "mp4",
                    "vcodec": "avc1", "acodec": "none", "height": 480},
                {"format_id": "variant", "protocol": "m3u8_native", "ext": "mp4", "height": 240}
            ]
        });
        let video = parse_for(
            &manifest.to_string(),
            &other("https://cdn.example.test/master.m3u8"),
        )
        .unwrap();
        assert_eq!(video.formats[0].format_id, "hls-720");
        assert!(video.formats[0].selector().starts_with("hls-720+"));
        assert!(
            video
                .formats
                .iter()
                .any(|f| f.format_id == "variant" && f.video())
        );
    }

    #[test]
    fn live_audio_only_drm_and_empty_results_are_refused() {
        let link = other("https://www.example.test/v");
        let live = |status: &str| {
            serde_json::json!({"live_status": status, "formats": [
                {"format_id": "0", "protocol": "m3u8_native", "ext": "mp4", "vcodec": "avc1"}
            ]})
            .to_string()
        };
        for status in ["is_live", "is_upcoming"] {
            assert_eq!(
                parse_for(&live(status), &link).unwrap_err(),
                EngineError::Unsupported(LIVE.into())
            );
        }
        assert!(parse_for(&live("was_live"), &link).is_ok());
        assert!(matches!(
            parse_for(
                r#"{"is_live":true,"formats":[{"format_id":"0","protocol":"https","ext":"mp4"}]}"#,
                &link
            ),
            Err(EngineError::Unsupported(_))
        ));
        for audio in [
            r#"{"formats":[{"format_id":"0","protocol":"https","ext":"mp3"}]}"#,
            r#"{"formats":[{"format_id":"a","protocol":"https","ext":"m4a","vcodec":"none","acodec":"mp4a"}]}"#,
        ] {
            assert_eq!(
                parse_for(audio, &link).unwrap_err(),
                EngineError::Unsupported(AUDIO_ONLY.into())
            );
        }
        assert_eq!(
            parse_for(
                r#"{"formats":[{"format_id":"0","protocol":"https","ext":"mp4","has_drm":true}]}"#,
                &link
            )
            .unwrap_err(),
            EngineError::Drm(Source::Other)
        );
        for empty in [
            r#"{"formats":[]}"#,
            r#"{"formats":[{"format_id":"x","protocol":"rtsp","ext":"mp4","vcodec":"h264"}]}"#,
            r#"{"formats":[{"format_id":"i","protocol":"https","ext":"jpg","vcodec":"none","acodec":"none"}]}"#,
        ] {
            assert_eq!(
                parse_for(empty, &link).unwrap_err(),
                EngineError::MissingVideo(Source::Other),
                "{empty}"
            );
        }
    }

    #[test]
    fn errors_are_distinct_and_never_echo_the_link_or_helper_output() {
        let secret = " https://private.example.test/v?token=secret [generic] raw-output";
        for (raw, expected) in [
            (
                "ERROR: Unsupported URL:",
                EngineError::Unsupported(UNSUPPORTED_SITE.into()),
            ),
            (
                "ERROR: [generic] No video formats found!",
                EngineError::MissingVideo(Source::Other),
            ),
            (
                "ERROR: This video is DRM protected",
                EngineError::Drm(Source::Other),
            ),
            (
                "ERROR: This video is only available for registered users",
                EngineError::SignInNeeded(Source::Other),
            ),
            (
                "ERROR: Use --cookies-from-browser or --cookies for the authentication",
                EngineError::SignInNeeded(Source::Other),
            ),
            (
                "ERROR: session cookies have expired",
                EngineError::SignInNeeded(Source::Other),
            ),
            (
                "ERROR: HTTP Error 404: Not Found",
                EngineError::Unavailable(Source::Other),
            ),
            (
                "ERROR: HTTP Error 429: Too Many Requests",
                EngineError::RateLimited(Source::Other),
            ),
            (
                "ERROR: Unable to download webpage: <urlopen error [Errno 11001] getaddrinfo failed>",
                EngineError::Network(Source::Other),
            ),
            (
                "ERROR: The read operation timed out",
                EngineError::TimedOut(Source::Other),
            ),
            (
                "ERROR: HTTP Error 403: Forbidden",
                EngineError::Restricted(Source::Other),
            ),
            (
                "[httpproxy @ 0000] Protocol 'httpproxy' not on whitelist 'http,https,tcp,tls,crypto'!\nERROR: ffmpeg exited with code 1",
                EngineError::Unsupported(FFMPEG_METHOD.into()),
            ),
            ("ERROR: something new", EngineError::Helper(HELPER.into())),
        ] {
            let error = Source::Other.explain(&format!("{raw}{secret}"));
            assert_eq!(error, expected, "{raw}");
            let message = error.user_message();
            for leak in [
                "private.example",
                "secret",
                "token",
                "raw-output",
                "generic",
                "http",
            ] {
                assert!(!message.to_lowercase().contains(leak), "{raw}: {message}");
            }
        }
        let messages: HashSet<_> = [
            EngineError::Unsupported(UNSUPPORTED_SITE.into()),
            EngineError::MissingVideo(Source::Other),
            EngineError::Drm(Source::Other),
            EngineError::SignInNeeded(Source::Other),
            EngineError::Unavailable(Source::Other),
            EngineError::RateLimited(Source::Other),
            EngineError::Network(Source::Other),
            EngineError::TimedOut(Source::Other),
            EngineError::Helper(HELPER.into()),
            EngineError::Unsupported(FFMPEG_METHOD.into()),
            EngineError::Unsupported(NESTED_NOT_PUBLIC.into()),
            EngineError::Unsupported(NESTED_PORT.into()),
            EngineError::Unsupported(NOT_PUBLIC.into()),
        ]
        .iter()
        .map(EngineError::user_message)
        .collect();
        assert_eq!(messages.len(), 13);
        let sign_in = EngineError::SignInNeeded(Source::Other).user_message();
        assert!(sign_in.contains("not supported for other sites"));
        assert!(!EngineError::SignInNeeded(Source::Other).needs_session());
    }

    #[test]
    #[ignore = "requires internet access and STREAM_DLP_OTHER_TEST_URL, a public sample video"]
    fn a_live_public_link_resolves_through_the_destination_policy() {
        let link = parse_link(&std::env::var("STREAM_DLP_OTHER_TEST_URL").unwrap()).unwrap();
        assert_eq!(link.source, Source::Other);
        let resolved = link.resolve(&AtomicBool::new(false)).unwrap().unwrap();
        assert_eq!(resolved.source, Source::Other);
        assert!(!resolved.takes_session());
        recheck(&resolved.url).unwrap();
    }

    #[test]
    fn filter_ids_cannot_break_out_of_the_match_filter() {
        assert!(filter_safe_id("clip-01_a.b"));
        for id in ["", "a'b", "a b", "a\\b", "a&b", &"x".repeat(129)] {
            assert!(!filter_safe_id(id), "{id}");
        }
    }
}
