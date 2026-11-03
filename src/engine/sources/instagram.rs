//! Individual Instagram Reels and feed posts.
use super::{EngineError, Video};
use url::Url;

pub(super) const HOSTS: &[&str] = &["instagram.com", "www.instagram.com"];

pub(in crate::engine) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}

pub(super) fn normalize(url: &Url) -> Result<String, &'static str> {
    let parts: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
    if let ["", kind @ ("p" | "reel"), id] = parts.as_slice()
        && valid_id(id)
    {
        return Ok(format!("https://www.instagram.com/{kind}/{id}/"));
    }
    Err(
        "Paste the link to an individual Instagram Reel or post. Stories, profiles, feeds and share redirects are unsupported.",
    )
}

pub(super) fn check(video: &mut Video) -> Result<(), EngineError> {
    if video.extractor_key.as_deref() != Some("Instagram")
        || !video.id.as_deref().is_some_and(valid_id)
    {
        return Err(EngineError::Unsupported(
            "Instagram returned an unsupported media entry.".into(),
        ));
    }
    if !video.formats.iter().any(|f| f.video()) {
        return Err(EngineError::MissingVideo(super::Source::Instagram));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::parse_link;
    use super::*;

    #[test]
    fn accepts_only_direct_post_and_reel_routes() {
        for kind in ["p", "reel"] {
            let link =
                parse_link(&format!("instagram.com/{kind}/Example_1-/?igsh=tracking")).unwrap();
            assert_eq!(link.source, super::super::Source::Instagram);
            assert_eq!(
                link.url,
                format!("https://www.instagram.com/{kind}/Example_1-/")
            );
            assert!(link.takes_session());
        }
        for url in [
            "instagram.com/stories/example/123",
            "instagram.com/example",
            "instagram.com/share/reel/Example",
            "instagram.com/p/Example/embed",
            "instagram.com/p/%2F",
            "instagram.com.invalid/p/Example",
            "https://name@instagram.com/p/Example",
            "https://instagram.com:8443/p/Example",
        ] {
            assert!(parse_link(url).is_err());
        }
    }

    #[test]
    fn login_redirect_offers_manual_recovery_even_when_rate_limiting_is_mentioned() {
        let source = super::super::Source::Instagram;
        assert_eq!(
            source.explain("redirected to login page, likely rate limited"),
            EngineError::SignInNeeded(source)
        );
        assert_eq!(
            source.explain("HTTP Error 429"),
            EngineError::RateLimited(source)
        );
        assert_eq!(
            source.explain("There is no video in this post"),
            EngineError::MissingVideo(source)
        );
    }
}
