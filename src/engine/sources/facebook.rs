//! Individual Facebook videos and anonymous, bounded share redirects.
use super::{EngineError, Link, Source, Video};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use url::Url;

pub(super) const HOSTS: &[&str] = &["facebook.com", "www.facebook.com", "m.facebook.com"];
const UNSUPPORTED: &str = "Paste an individual Facebook Reel, watch, video or video share link. Feeds, groups and collections are unsupported.";
const REDIRECT: &str = "This Facebook share link did not resolve to an individual video. Open it in your browser and copy the video link.";

fn numeric(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.bytes().all(|c| c.is_ascii_digit())
}

pub(super) fn normalize(url: &Url) -> Result<String, &'static str> {
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    let base = "https://www.facebook.com";
    match parts.as_slice() {
        ["watch"] => {
            let mut values = url.query_pairs().filter(|(name, _)| name == "v");
            let (_, id) = values.next().ok_or(UNSUPPORTED)?;
            if numeric(&id) && values.next().is_none() {
                return Ok(format!("{base}/watch/?v={id}"));
            }
        }
        // The Reel extractor delegates to this watch route with the same media ID.
        ["reel", id] if numeric(id) => return Ok(format!("{base}/watch/?v={id}")),
        [account, "videos", id]
            if numeric(id)
                && !matches!(*account, "groups" | "events" | "watch" | "live" | "share")
                && !account.is_empty()
                && account
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)) =>
        {
            return Ok(format!("{base}/{account}/videos/{id}/"));
        }
        ["share", kind @ ("r" | "v"), token]
            if !token.is_empty()
                && token.len() <= 128
                && token
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)) =>
        {
            return Ok(format!("{base}/share/{kind}/{token}/"));
        }
        _ => {}
    }
    Err(UNSUPPORTED)
}

pub(super) fn is_share(url: &str) -> bool {
    url.starts_with("https://www.facebook.com/share/")
}

fn destination(current: &str, location: &str) -> Result<Link, EngineError> {
    let invalid = || EngineError::Unsupported(REDIRECT.into());
    let url = Url::parse(current)
        .and_then(|u| u.join(location))
        .map_err(|_| invalid())?;
    if url.scheme() != "https" || !url.host_str().is_some_and(|host| HOSTS.contains(&host)) {
        return Err(invalid());
    }
    super::parse_link(url.as_str()).map_err(|_| invalid())
}

pub(super) fn resolve(url: &str, cancel: &AtomicBool) -> Result<Option<Link>, EngineError> {
    let agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut current = url.to_owned();
    let mut visited = HashSet::new();
    for _ in 0..5 {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if !visited.insert(current.clone()) {
            break;
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(EngineError::TimedOut(Source::Facebook))?;
        let response = agent
            .get(&current)
            .config()
            .timeout_global(Some(remaining))
            .build()
            .call();
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let response = response.map_err(|error| match error {
            ureq::Error::Timeout(_) => EngineError::TimedOut(Source::Facebook),
            ureq::Error::StatusCode(code) => {
                Source::Facebook.explain(&format!("HTTP error {code}"))
            }
            _ => EngineError::Network(Source::Facebook),
        })?;
        if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            break;
        }
        let Some(location) = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
        else {
            break;
        };
        let next = destination(&current, location)?;
        if !is_share(&next.url) {
            return Ok(Some(next));
        }
        current = next.url;
    }
    Err(EngineError::Unsupported(REDIRECT.into()))
}

pub(super) fn check(video: &mut Video) -> Result<(), EngineError> {
    if video.extractor_key.as_deref() != Some("Facebook") {
        return Err(EngineError::Unsupported(UNSUPPORTED.into()));
    }
    if !video.id.as_deref().is_some_and(numeric) {
        return Err(EngineError::Unsupported(
            "Facebook returned no usable video ID.".into(),
        ));
    }
    if !video.formats.iter().any(|f| f.video()) {
        return Err(EngineError::MissingVideo(Source::Facebook));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{media::parse_for, parse_link};

    #[test]
    fn routes_and_redirects_stay_on_individual_videos() {
        let base = "https://www.facebook.com/share/r/Example/";
        for path in ["/share/r/Example", "/share/v/Example/"] {
            assert!(!destination(base, path).unwrap().takes_session());
        }
        for path in [
            "/reel/123/",
            "/example/videos/123/",
            "https://m.facebook.com/watch/?v=123&tracking=1",
        ] {
            let link = destination(base, path).unwrap();
            assert!(link.takes_session());
            assert!(link.url.contains("123"));
        }
        for target in [
            "//facebook.com.invalid/reel/123",
            "http://www.facebook.com/reel/123",
            "//name@facebook.com/reel/123",
            "//facebook.com:8443/reel/123",
            "/groups/example",
            "/watch/?v=1&v=2",
            "/watch/?list=123",
            "/share/p/Example",
            "/live/",
        ] {
            assert!(destination(base, target).is_err());
        }
        assert!(
            parse_link(base)
                .unwrap()
                .resolve(&AtomicBool::new(true))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn progressive_format_and_missing_metadata_survive_inspection() {
        let link = parse_link("facebook.com/reel/123").unwrap();
        let mut metadata = serde_json::json!({"id":"123","extractor_key":"Facebook","formats":[{"format_id":"sd","protocol":"https","ext":"mp4"}]});
        let video = parse_for(&metadata.to_string(), &link).unwrap();
        assert_eq!(video.formats[0].selector(), "sd");
        assert_eq!(video.formats[0].quality(), "Video · unknown resolution");
        assert_eq!(video.source_url, link.url);
        metadata["entries"] = serde_json::json!([]);
        assert!(matches!(
            parse_for(&metadata.to_string(), &link),
            Err(EngineError::Unsupported(_))
        ));
        assert_eq!(
            Source::Facebook.explain("Only available for registered users"),
            EngineError::SignInNeeded(Source::Facebook)
        );
        assert_eq!(
            Source::Facebook.explain("HTTP Error 404"),
            EngineError::Unavailable(Source::Facebook)
        );
    }
}
