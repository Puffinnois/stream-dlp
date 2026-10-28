//! Per-site rules: which links are accepted, share links, the formats offered, and how
//! yt-dlp's errors are explained. A new site is a new module here.
mod tiktok;
mod youtube;

use super::{EngineError, Video};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    YouTube,
    TikTok,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::YouTube => "YouTube",
            Self::TikTok => "TikTok",
        }
    }

    /// Extra yt-dlp arguments for this site.
    pub(super) fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::YouTube => &[],
            Self::TikTok => &["--use-extractors", "TikTok"],
        }
    }

    pub(super) fn output_template(self) -> &'static str {
        match self {
            Self::YouTube => "%(title).150s [%(id)s] [%(format_id)s].%(ext)s",
            Self::TikTok => "TikTok %(title).140s [%(id)s] [%(format_id)s].%(ext)s",
        }
    }

    /// Whether yt-dlp's own output may reach the UI. TikTok's can contain signed media URLs.
    pub(super) fn shows_helper_output(self) -> bool {
        self == Self::YouTube
    }

    /// Keeps what stream-dlp supports from this site, or explains why nothing is left.
    pub(super) fn check(self, video: &mut Video) -> Result<(), EngineError> {
        match self {
            Self::YouTube => Ok(()),
            Self::TikTok => tiktok::check(video),
        }
    }

    /// Explains a failed yt-dlp run from its error output.
    pub(super) fn explain(self, errors: &str) -> EngineError {
        match self {
            Self::YouTube => youtube::explain(errors),
            Self::TikTok => tiktok::explain(errors),
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
    /// Whether the YouTube session may go with this link: only to YouTube itself.
    pub(super) fn takes_session(&self) -> bool {
        self.source == Source::YouTube && youtube::is_canonical(&self.url)
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
        .map_err(|_| unsupported("Paste a valid YouTube or TikTok video link."))?;
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err(unsupported(
            "Paste a standard video link without credentials or a custom port.",
        ));
    }
    let host = url.host_str().unwrap_or("");
    if tiktok::HOSTS.contains(&host) {
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
    fn only_youtube_links_take_the_session() {
        for (link, takes) in [
            ("https://youtu.be/BaW_jenozKc", true),
            ("https://vm.tiktok.com/ABC123/", false),
            ("https://www.tiktok.com/@creator/video/123", false),
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
