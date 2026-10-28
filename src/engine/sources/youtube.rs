//! YouTube: watch, Shorts, embed, live and youtu.be links, and its sign-in check.
use super::Source;
use crate::engine::EngineError;

const HOSTS: [&str; 4] = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
];

/// The watch link for the video, without playlist or tracking parameters.
pub(super) fn normalize(url: &url::Url, host: &str) -> Result<String, &'static str> {
    let id = if host == "youtu.be" || host == "www.youtu.be" {
        url.path_segments()
            .and_then(|mut s| s.next())
            .map(str::to_owned)
    } else if HOSTS.contains(&host) {
        if url.path() == "/watch" {
            url.query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned())
        } else {
            let s: Vec<_> = url.path_segments().into_iter().flatten().collect();
            if s.len() >= 2 && matches!(s[0], "shorts" | "embed" | "live") {
                Some(s[1].to_owned())
            } else {
                None
            }
        }
    } else {
        None
    };
    let id = id
        .filter(|id| {
            id.len() == 11
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .ok_or("Paste a YouTube or full TikTok video link.")?;
    Ok(format!("https://www.youtube.com/watch?v={id}"))
}

/// Whether `link` is a watch link [`normalize`] made.
pub(super) fn is_canonical(link: &str) -> bool {
    url::Url::parse(link).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("www.youtube.com")
            && url.path() == "/watch"
    })
}

/// yt-dlp's last error line, or the sign-in check it reports.
pub(super) fn explain(errors: &str) -> EngineError {
    let tail = errors
        .lines()
        .rev()
        .find(|l| l.contains("ERROR:"))
        .unwrap_or(errors.trim());
    let tail: String = tail.chars().take(1200).collect();
    if tail.is_empty() {
        EngineError::Helper(
            "The download engine stopped unexpectedly. Try again or update the tools.".into(),
        )
    } else if tail.contains("Sign in")
        || tail.contains("confirm you’re not a bot")
        || tail.contains("confirm you're not a bot")
    {
        EngineError::SignInNeeded(Source::YouTube)
    } else {
        EngineError::Helper(tail.trim_start_matches("ERROR: ").to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse_link;
    use super::*;

    #[test]
    fn canonicalizes_video_links_without_playlist_or_tracking() {
        for link in [
            "https://youtu.be/BaW_jenozKc?t=5",
            "www.youtube.com/watch?v=BaW_jenozKc&list=123",
            "https://youtube.com/shorts/BaW_jenozKc",
            "https://www.youtube.com/shorts/BaW_jenozKc?feature=share",
            "https://m.youtube.com/shorts/BaW_jenozKc",
        ] {
            let link = parse_link(link).unwrap();
            assert_eq!(link.source, Source::YouTube);
            assert_eq!(link.url, "https://www.youtube.com/watch?v=BaW_jenozKc");
            assert!(is_canonical(&link.url));
        }
    }

    #[test]
    fn rejects_playlists_and_invalid_ids() {
        for link in [
            "https://youtube.com/playlist?list=abc",
            "https://youtu.be/a",
        ] {
            assert!(parse_link(link).is_err(), "{link}");
        }
    }

    #[test]
    fn explains_verification_errors() {
        let error = explain(
            "WARNING: No title found\nERROR: [youtube] id: Sign in to confirm you’re not a bot. Use --cookies-from-browser...",
        );
        assert!(error.needs_session());
        assert!(error.user_message().contains("verification check"));
        assert!(!error.user_message().contains("--cookies-from-browser"));
        let unavailable = explain("ERROR: Video unavailable");
        assert!(!unavailable.needs_session());
        assert!(unavailable.user_message().contains("Video unavailable"));
        assert!(explain("").user_message().contains("stopped unexpectedly"));
    }
}
