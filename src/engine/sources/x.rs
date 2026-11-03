use super::{EngineError, Video};

pub(super) const HOSTS: &[&str] = &["x.com", "twitter.com", "mobile.twitter.com"];

pub(super) fn normalize(url: &url::Url) -> Result<String, &'static str> {
    let parts: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
    if let ["", account, "status", id] = parts.as_slice()
        && url.scheme() == "https"
        && !account.is_empty()
        && account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
    {
        return Ok(format!("https://x.com/{account}/status/{id}"));
    }
    Err("Paste the full HTTPS link to an individual X or Twitter post.")
}

pub(super) fn check(video: &mut Video) -> Result<(), EngineError> {
    if video.extractor_key.as_deref() != Some("Twitter")
        || !video
            .id
            .as_deref()
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(EngineError::Unsupported(
            "X returned an unsupported media entry.".into(),
        ));
    }
    video.formats.retain(|f| f.direct_video);
    if video.formats.is_empty() {
        return Err(EngineError::Unsupported(
            "X returned no downloadable native MP4 video.".into(),
        ));
    }
    Ok(())
}
