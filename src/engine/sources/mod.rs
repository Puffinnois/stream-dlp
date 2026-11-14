//! Per-site rules: which links are accepted, share links, the formats offered, and how
//! yt-dlp's errors are explained. A new site is a new module here.
mod facebook;
pub(super) mod instagram;
pub(super) mod other;
mod tiktok;
mod x;
mod youtube;

use super::{EngineError, Video};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum Source {
    #[default]
    YouTube,
    TikTok,
    X,
    Facebook,
    Instagram,
    /// Any other public site, tried through yt-dlp's other extractors (experimental).
    Other,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::YouTube => "YouTube",
            Self::TikTok => "TikTok",
            Self::Instagram => "Instagram",
            Self::Facebook => "Facebook",
            Self::X => "X / Twitter",
            Self::Other => "Other sites (experimental)",
        }
    }

    /// The domains a named platform owns, and that stay with it even when its rules refuse a
    /// link there. `None` for Other.
    fn domains(self) -> &'static [&'static str] {
        match self {
            Self::YouTube => &["youtube.com", "youtu.be", "youtube-nocookie.com"],
            Self::TikTok => &["tiktok.com"],
            Self::X => &["x.com", "twitter.com"],
            Self::Facebook => &["facebook.com", "fb.com"],
            Self::Instagram => &["instagram.com", "instagr.am"],
            Self::Other => &[],
        }
    }

    /// Extra yt-dlp arguments for this site.
    pub(super) fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::Facebook => &["--use-extractors", "Facebook"],
            Self::Instagram => &["--use-extractors", "Instagram", "--yes-playlist"],
            Self::YouTube => &[],
            Self::TikTok => &["--use-extractors", "TikTok"],
            Self::X => &["--use-extractors", "Twitter", "--yes-playlist"],
            // No extractor restriction: any of yt-dlp's other extractors or its generic one may
            // answer. Cookies stay off, and FFmpeg, when yt-dlp hands it a stream, may only open
            // network protocols, never local files or other schemes.
            Self::Other => &[
                "--no-cookies",
                "--no-cookies-from-browser",
                "--downloader-args",
                "ffmpeg_i:-protocol_whitelist http,https,tcp,tls,crypto",
            ],
        }
    }

    pub(super) fn output_template(self) -> &'static str {
        match self {
            Self::YouTube => "%(title).150s [%(id)s] [%(format_id)s].%(ext)s",
            Self::TikTok => "TikTok %(title).140s [%(id)s] [%(format_id)s].%(ext)s",
            Self::Facebook => "Facebook %(title).140s [%(id)s] [%(format_id)s].%(ext)s",
            Self::Instagram => "Instagram %(title).140s [%(id)s] [%(format_id)s].%(ext)s",
            Self::X => "X %(title).140s [%(id)s] [%(format_id)s].%(ext)s",
            // The extractor names the site; the generic one also adds the page's domain, since
            // its IDs come from file names that many sites share.
            Self::Other => {
                "%(extractor_key).30s %(webpage_url_domain).60s %(title).100s [%(id)s] [%(format_id)s].%(ext)s"
            }
        }
    }

    /// Keeps what stream-dlp supports from this site, or explains why nothing is left.
    pub(super) fn check(self, video: &mut Video) -> Result<(), EngineError> {
        match self {
            Self::YouTube => Ok(()),
            Self::TikTok => tiktok::check(video),
            Self::X => x::check(video),
            Self::Instagram => instagram::check(video),
            Self::Facebook => facebook::check(video),
            Self::Other => other::check(video),
        }
    }

    /// Explains a failed yt-dlp run from its error output.
    pub(super) fn explain(self, errors: &str) -> EngineError {
        let lower = errors.to_lowercase();
        if self == Self::Other {
            return other::explain(&lower);
        }
        let has = |patterns: &[&str]| patterns.iter().any(|pattern| lower.contains(pattern));
        if has(&["drm protected", "drm-protected"]) {
            EngineError::Drm(self)
        } else if has(&["cookie", "session"]) && has(&["expired", "no longer valid"]) {
            EngineError::ExpiredSession(self)
        } else if self == Self::Instagram
            && has(&["login page", "login required", "log in", "login_requested"])
        {
            EngineError::SignInNeeded(self)
        } else if has(&[
            "http error 429",
            "http 429",
            "too many requests",
            "rate limit",
        ]) {
            EngineError::RateLimited(self)
        } else if self == Self::TikTok
            && has(&["private video", "video is private", "private account"])
        {
            EngineError::Unsupported(
                "This TikTok video is private. Choose a public video post.".into(),
            )
        } else if has(&[
            "sign in",
            "log in",
            "login",
            "requires authentication",
            "registered users",
            "not authorized",
            "not a bot",
            "http error 401",
        ]) {
            EngineError::SignInNeeded(self)
        } else if has(&["no video", "no formats found", "no downloadable video"])
            || (lower.contains("does not contain") && lower.contains("video"))
        {
            EngineError::MissingVideo(self)
        } else if lower.contains("timed out") {
            EngineError::TimedOut(self)
        } else if has(&[
            "connection",
            "resolve",
            "getaddrinfo failed",
            "unable to download webpage: <urlopen error",
        ]) {
            EngineError::Network(self)
        } else if has(&[
            "not available",
            "unavailable",
            "removed",
            "http 404",
            "http error 404",
        ]) || (self == Self::TikTok && lower.contains("100002"))
        {
            EngineError::Unavailable(self)
        } else if self == Self::Facebook
            && has(&["cannot parse data", "unable to extract", "cannot extract"])
        {
            EngineError::Helper("The download tools could not read this Facebook video. Check that it plays in your browser, then update the tools in Settings and retry.".into())
        } else if has(&["blocked", "http error 403", "http 403"]) {
            EngineError::Restricted(self)
        } else {
            EngineError::Helper(format!(
                "The {} download engine stopped unexpectedly. Check the link in your browser or update the tools.",
                self.label()
            ))
        }
    }
}

/// A pasted link after validation: its site and canonical URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub source: Source,
    pub url: String,
}

impl Link {
    /// Revalidate the exact destination before attaching a platform cookie jar.
    pub(super) fn takes_session(&self) -> bool {
        self.source != Source::Other
            && parse_link(&self.url).is_ok_and(|canonical| canonical == *self)
            && !(self.source == Source::Facebook && facebook::is_share(&self.url))
            && !(self.source == Source::TikTok && tiktok::is_short(&self.url))
    }

    /// Follows a share link to the video it stands for. `None` when cancelled.
    pub(super) fn resolve(self, cancel: &AtomicBool) -> Result<Option<Self>, EngineError> {
        if self.source == Source::TikTok && tiktok::is_short(&self.url) {
            tiktok::resolve(&self.url, cancel)
        } else if self.source == Source::Facebook && facebook::is_share(&self.url) {
            facebook::resolve(&self.url, cancel)
        } else if self.source == Source::Other {
            other::resolve(&self.url, cancel)
        } else {
            Ok(Some(self))
        }
    }
}

/// Validates a pasted link and returns its canonical form.
pub fn parse_link(input: &str) -> Result<Link, EngineError> {
    let input = input.trim();
    let full = if input.starts_with("https://") || input.starts_with("http://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let unsupported = |message: &str| EngineError::Unsupported(message.into());
    let url = url::Url::parse(&full)
        .map_err(|_| unsupported("Paste a valid http or https link to a video."))?;
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err(unsupported(
            "Paste a standard video link without credentials or a custom port.",
        ));
    }
    let host = url.host_str().unwrap_or("");
    if instagram::HOSTS.contains(&host) {
        instagram::normalize(&url).map(|url| Link {
            source: Source::Instagram,
            url,
        })
    } else if facebook::HOSTS.contains(&host) {
        facebook::normalize(&url).map(|url| Link {
            source: Source::Facebook,
            url,
        })
    } else if x::HOSTS.contains(&host) {
        x::normalize(&url).map(|url| Link {
            source: Source::X,
            url,
        })
    } else if tiktok::HOSTS.contains(&host) {
        tiktok::normalize(&url, host).map(|url| Link {
            source: Source::TikTok,
            url,
        })
    } else if youtube::HOSTS.contains(&host) {
        youtube::normalize(&url, host).map(|url| Link {
            source: Source::YouTube,
            url,
        })
    } else if let Some(source) = owner(host) {
        // Another address of a named platform: it stays that platform's, never Other.
        return Err(EngineError::Unsupported(format!(
            "This {} address is not supported. Paste a link from the main {} site.",
            source.label(),
            source.label()
        )));
    } else if let Some(source) = NAMED.into_iter().find(|source| {
        source.domains().iter().any(|domain| {
            let marker = format!("{domain}.");
            host.starts_with(&marker) || host.contains(&format!(".{marker}"))
        })
    }) {
        return Err(EngineError::Unsupported(format!(
            "This address imitates {}. Paste a link from the real site.",
            source.label()
        )));
    } else {
        other::normalize(&url).map(|url| Link {
            source: Source::Other,
            url,
        })
    }
    .map_err(unsupported)
}

/// The named platform whose domains include `host`, or one of their subdomains.
fn owner(host: &str) -> Option<Source> {
    NAMED.into_iter().find(|source| {
        source
            .domains()
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    })
}

/// The named platform that owns the pasted link's host, even when that platform's rules refuse
/// the link. `None` when the text is not a link or its host belongs to no named platform; never
/// [`Source::Other`].
pub fn named_platform(input: &str) -> Option<Source> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let full = if input.starts_with("https://") || input.starts_with("http://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    owner(url::Url::parse(&full).ok()?.host_str()?)
}

const NAMED: [Source; 5] = [
    Source::YouTube,
    Source::TikTok,
    Source::X,
    Source::Facebook,
    Source::Instagram,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_platform_owns_its_hosts_and_subdomains_but_never_lookalikes() {
        for (input, source) in [
            (
                "https://www.youtube.com/watch?v=abcdefghijk",
                Source::YouTube,
            ),
            ("youtu.be/abcdefghijk", Source::YouTube),
            (
                "https://music.youtube.com/watch?v=abcdefghijk",
                Source::YouTube,
            ),
            // Refused by TikTok's rules, still TikTok's.
            ("https://www.tiktok.com/@creator", Source::TikTok),
            ("vm.tiktok.com/Example/", Source::TikTok),
            ("http://mobile.twitter.com/example/status/1", Source::X),
            ("x.com", Source::X),
            ("https://web.facebook.com/groups/example", Source::Facebook),
            ("fb.com/example", Source::Facebook),
            (
                "  https://instagram.com/stories/example/1  ",
                Source::Instagram,
            ),
            ("instagr.am/p/Example/", Source::Instagram),
        ] {
            assert_eq!(named_platform(input), Some(source), "{input}");
        }
        // Every host a platform accepts belongs to it.
        for (hosts, source) in [
            (&youtube::HOSTS[..], Source::YouTube),
            (&tiktok::HOSTS[..], Source::TikTok),
            (x::HOSTS, Source::X),
            (facebook::HOSTS, Source::Facebook),
            (instagram::HOSTS, Source::Instagram),
        ] {
            for host in hosts {
                assert_eq!(named_platform(host), Some(source), "{host}");
            }
        }
        for input in [
            "",
            "   ",
            "not a link at all",
            "https://",
            "https://youtube.com.example/watch?v=abcdefghijk",
            "https://notyoutube.com/watch?v=abcdefghijk",
            "https://tiktok.com-video.example/@a/video/1",
            "https://x.com.invalid/a/status/1",
            "https://fakefacebook.com/reel/1",
            "https://media.example/clip.mp4",
            "fb.watch/example",
            "t.co/example",
        ] {
            assert_eq!(named_platform(input), None, "{input:?}");
        }
    }

    #[test]
    fn deceptive_hosts_credentials_and_ports_are_rejected() {
        for link in [
            "https://youtube.com.evil.com/watch?v=BaW_jenozKc",
            "https://youtube.com@evil.com/watch?v=BaW_jenozKc",
            "file:///tmp/a",
            "--exec calc",
            "https://youtube.com:8080/watch?v=BaW_jenozKc",
            "https://www.tiktok.com.evil.com/@c/video/123",
            "https://www.tiktok.com@evil.com/@c/video/123",
            "https://www.tiktok.com:8080/@c/video/123",
        ] {
            assert!(
                matches!(parse_link(link), Err(EngineError::Unsupported(_))),
                "{link}"
            );
        }
    }

    #[test]
    fn errors_keep_platform_and_drop_private_details() {
        for source in [Source::YouTube, Source::TikTok, Source::X] {
            for (raw, expected) in [
                (
                    "session cookies have expired",
                    EngineError::ExpiredSession(source),
                ),
                ("DRM protected", EngineError::Drm(source)),
                ("No video found", EngineError::MissingVideo(source)),
                ("timed out", EngineError::TimedOut(source)),
            ] {
                let error =
                    source.explain(&format!("{raw} https://example.test/@account?token=secret"));
                assert_eq!(error, expected);
                assert!(!error.user_message().contains("secret"));
            }
            assert!(
                matches!(source.explain("Cookie: secret"), EngineError::Helper(message) if !message.contains("secret"))
            );
        }
    }

    #[test]
    fn only_canonical_video_links_take_sessions() {
        for (link, takes) in [
            ("https://youtu.be/BaW_jenozKc", true),
            ("https://vm.tiktok.com/ABC123/", false),
            ("https://www.tiktok.com/@creator/video/123", true),
        ] {
            assert_eq!(parse_link(link).unwrap().takes_session(), takes, "{link}");
        }
        let local = Link {
            source: Source::YouTube,
            url: "http://127.0.0.1:8080/manifest.mpd".into(),
        };
        assert!(!local.takes_session());
    }
}
