//! Per-site rules: which links are accepted, share links, the formats offered, and how
//! yt-dlp's errors are explained. A new site is a new module here.
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
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::YouTube => "YouTube",
            Self::TikTok => "TikTok",
            Self::X => "X / Twitter",
        }
    }

    /// Extra yt-dlp arguments for this site.
    pub(super) fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::YouTube => &[],
            Self::TikTok => &["--use-extractors", "TikTok"],
            Self::X => &["--use-extractors", "Twitter", "--yes-playlist"],
        }
    }

    pub(super) fn output_template(self) -> &'static str {
        match self {
            Self::YouTube => "%(title).150s [%(id)s] [%(format_id)s].%(ext)s",
            Self::TikTok => "TikTok %(title).140s [%(id)s] [%(format_id)s].%(ext)s",
            Self::X => "X %(title).140s [%(id)s] [%(format_id)s].%(ext)s",
        }
    }

    /// Keeps what stream-dlp supports from this site, or explains why nothing is left.
    pub(super) fn check(self, video: &mut Video) -> Result<(), EngineError> {
        match self {
            Self::YouTube => Ok(()),
            Self::TikTok => tiktok::check(video),
            Self::X => x::check(video),
        }
    }

    /// Explains a failed yt-dlp run from its error output.
    pub(super) fn explain(self, errors: &str) -> EngineError {
        let lower = errors.to_lowercase();
        let has = |patterns: &[&str]| patterns.iter().any(|pattern| lower.contains(pattern));
        if has(&["drm protected", "drm-protected"]) {
            EngineError::Drm(self)
        } else if has(&["cookie", "session"]) && has(&["expired", "no longer valid"]) {
            EngineError::ExpiredSession(self)
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
        parse_link(&self.url).is_ok_and(|canonical| canonical == *self)
            && !(self.source == Source::TikTok && tiktok::is_short(&self.url))
    }

    /// Follows a share link to the video it stands for. `None` when cancelled.
    pub(super) fn resolve(self, cancel: &AtomicBool) -> Result<Option<Self>, EngineError> {
        if self.source == Source::TikTok && tiktok::is_short(&self.url) {
            tiktok::resolve(&self.url, cancel)
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
        .map_err(|_| unsupported("Paste a valid YouTube, TikTok or X video link."))?;
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err(unsupported(
            "Paste a standard video link without credentials or a custom port.",
        ));
    }
    let host = url.host_str().unwrap_or("");
    if x::HOSTS.contains(&host) {
        x::normalize(&url).map(|url| Link {
            source: Source::X,
            url,
        })
    } else if tiktok::HOSTS.contains(&host) {
        tiktok::normalize(&url, host).map(|url| Link {
            source: Source::TikTok,
            url,
        })
    } else {
        youtube::normalize(&url, host).map(|url| Link {
            source: Source::YouTube,
            url,
        })
    }
    .map_err(unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

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
