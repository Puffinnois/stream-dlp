//! TikTok: public video and share links, resolved anonymously with the bundled Deno; only
//! muxed video with audio is offered.
use super::{Link, Source};
use crate::{
    engine::{EngineError, Video},
    process,
};
use std::{
    io::Write,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) const HOSTS: [&str; 5] = [
    "www.tiktok.com",
    "tiktok.com",
    "m.tiktok.com",
    "vm.tiktok.com",
    "vt.tiktok.com",
];
const SHORT_HOSTS: [&str; 2] = ["vm.tiktok.com", "vt.tiktok.com"];

/// The `www.tiktok.com` form of a video or share link.
pub(super) fn normalize(url: &url::Url, host: &str) -> Result<String, &'static str> {
    let parts: Vec<_> = url
        .path()
        .trim_end_matches('/')
        .split('/')
        .skip(1)
        .collect();
    let short_token = if SHORT_HOSTS.contains(&host) && parts.len() == 1 {
        Some(parts[0])
    } else if !SHORT_HOSTS.contains(&host) && parts.len() == 2 && parts[0] == "t" {
        Some(parts[1])
    } else {
        None
    };
    if short_token.is_some_and(|token| {
        !token.is_empty()
            && token.len() <= 128
            && token
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    }) {
        let mut canonical = url.clone();
        canonical.set_scheme("https").map_err(|_| "Invalid URL")?;
        if matches!(host, "tiktok.com" | "m.tiktok.com") {
            canonical
                .set_host(Some("www.tiktok.com"))
                .map_err(|_| "Invalid host")?;
        }
        canonical.set_fragment(None);
        return Ok(canonical.to_string());
    }
    if SHORT_HOSTS.contains(&host) {
        return Err("Paste a TikTok video or share link.");
    }
    if parts.len() == 2
        && parts[0] == "v"
        && let Some(id) = parts[1].strip_suffix(".html")
        && !id.is_empty()
        && id.len() <= 20
        && id.bytes().all(|c| c.is_ascii_digit())
    {
        let mut canonical = url.clone();
        canonical.set_scheme("https").map_err(|_| "Invalid URL")?;
        canonical
            .set_host(Some("www.tiktok.com"))
            .map_err(|_| "Invalid host")?;
        canonical.set_path(&format!("/share/video/{id}"));
        canonical.set_fragment(None);
        return Ok(canonical.to_string());
    }
    if parts.len() == 3
        && parts[1] == "video"
        && (parts[0] == "share"
            || parts[0].strip_prefix('@').is_some_and(|u| {
                !u.is_empty()
                    && u.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
            }))
        && !parts[2].is_empty()
        && parts[2].len() <= 20
        && parts[2].bytes().all(|c| c.is_ascii_digit())
    {
        let mut canonical = url.clone();
        canonical.set_scheme("https").map_err(|_| "Invalid URL")?;
        canonical
            .set_host(Some("www.tiktok.com"))
            .map_err(|_| "Invalid host")?;
        canonical.set_fragment(None);
        return Ok(canonical.to_string());
    }
    Err(if parts.contains(&"photo") {
        "TikTok photo slideshows are not supported. Choose an individual video post."
    } else if parts.contains(&"live") {
        "TikTok live streams are not supported. Choose a recorded video post."
    } else {
        "Paste an individual TikTok video or share link. Profiles and collections are not supported."
    })
}

pub(super) fn is_short(link: &str) -> bool {
    url::Url::parse(link).is_ok_and(|u| {
        u.host_str().is_some_and(|host| SHORT_HOSTS.contains(&host))
            || (u.host_str() == Some("www.tiktok.com") && u.path().starts_with("/t/"))
    })
}

const NOT_RESOLVED: &str = "Could not resolve this TikTok share link. Open it in your browser and copy the full video link.";

/// Follows a share link without cookies. `None` when cancelled.
pub(super) fn resolve(link: &str, cancel: &AtomicBool) -> Result<Option<Link>, EngineError> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let helper = |message: &str| EngineError::Helper(message.into());
    let mut script = tempfile::Builder::new()
        .suffix(".mjs")
        .tempfile()
        .map_err(|_| helper("Could not prepare TikTok link resolver."))?;
    script
        .write_all(include_bytes!("../../../scripts/tiktok-resolve.js"))
        .map_err(|_| helper("Could not prepare TikTok link resolver."))?;
    let output = match process::Helper::new(
        Command::new(crate::tools::path("deno"))
            .args(["run", "--no-config", "--no-lock", "--no-prompt", "--allow-net=tiktok.com:443,www.tiktok.com:443,m.tiktok.com:443,vm.tiktok.com:443,vt.tiktok.com:443"])
            .arg(script.path())
            .arg(link),
        cancel,
    )
    .time_limit(Duration::from_secs(20))
    .output_limit(8192)
    .ignore_stderr()
    .output()
    {
        Ok(output) => output,
        Err(process::Error::Cancelled) => return Ok(None),
        Err(process::Error::TimedOut) => {
            return Err(helper(
                "TikTok share-link resolution timed out. Try the full video link.",
            ));
        }
        Err(process::Error::Start(_)) => {
            return Err(helper(
                "Could not start TikTok link resolver. Check the download tools in Settings.",
            ));
        }
        Err(process::Error::TooMuchOutput) => return Err(helper(NOT_RESOLVED)),
        Err(process::Error::Wait(_)) => {
            return Err(helper("Could not resolve the TikTok share link."));
        }
    };
    if !output.status.success() {
        return Err(helper(NOT_RESOLVED));
    }
    let link = super::parse_link(String::from_utf8_lossy(&output.stdout).trim())?;
    if link.source != Source::TikTok || is_short(&link.url) {
        return Err(EngineError::Unsupported(
            "The TikTok share link did not lead to an individual video.".into(),
        ));
    }
    Ok(Some(link))
}

pub(super) fn check(video: &mut Video) -> Result<(), EngineError> {
    if video.extractor_key.as_deref() != Some("TikTok") {
        return Err(EngineError::Unsupported(
            "This link did not return an individual TikTok video.".into(),
        ));
    }
    // First TikTok release supports muxed, playable video only. Do not treat
    // an audio-only slideshow or an untested video-only stream as a video.
    video.formats.retain(|f| {
        f.audio() && matches!(f.vcodec.as_deref(), Some("h264" | "h265" | "avc1" | "hevc"))
    });
    if video.formats.is_empty() {
        return Err(EngineError::Unsupported("TikTok returned no supported video with audio. Photo posts, silent clips, and video-only formats are not supported yet.".into()));
    }
    Ok(())
}

/// Classifies yt-dlp's errors without repeating them: they can contain signed media URLs.
pub(super) fn explain(errors: &str) -> EngineError {
    let lower = errors.to_lowercase();
    if lower.contains("log in")
        || lower.contains("login")
        || lower.contains("sign in")
        || lower.contains("private")
    {
        EngineError::SignInNeeded(Source::TikTok)
    } else if lower.contains("resolve")
        || lower.contains("timed out")
        || lower.contains("connection")
    {
        EngineError::Network(Source::TikTok)
    } else if lower.contains("429") || lower.contains("rate limit") {
        EngineError::RateLimited(Source::TikTok)
    } else if lower.contains("100002")
        || lower.contains("not available")
        || lower.contains("unavailable")
        || lower.contains("removed")
        || lower.contains("404")
    {
        EngineError::Unavailable(Source::TikTok)
    } else if lower.contains("blocked") || lower.contains("403") {
        EngineError::Restricted(Source::TikTok)
    } else {
        EngineError::Helper("TikTok could not provide this video. It may be unavailable or unsupported; check the link in your browser or update the download tools.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse_link;
    use super::*;

    #[test]
    fn share_and_mobile_routes_are_tiktok_links() {
        for link in [
            "https://vm.tiktok.com/ABC123/",
            "https://vt.tiktok.com/ABC123/",
            "https://www.tiktok.com/t/ABC123?lang=en",
            "https://m.tiktok.com/@creator/video/123",
            "https://m.tiktok.com/v/123.html",
        ] {
            assert_eq!(parse_link(link).unwrap().source, Source::TikTok, "{link}");
        }
        assert!(is_short(
            &parse_link("https://vm.tiktok.com/ABC123/").unwrap().url
        ));
        assert!(!is_short(
            &parse_link("https://m.tiktok.com/v/123.html").unwrap().url
        ));
        assert_eq!(
            parse_link("https://m.tiktok.com/v/123.html?lang=en")
                .unwrap()
                .url,
            "https://www.tiktok.com/share/video/123?lang=en"
        );
        for (url, explanation) in [
            ("https://www.tiktok.com/@creator/photo/123", "photo"),
            ("https://www.tiktok.com/@creator/live", "live"),
            ("https://www.tiktok.com/@creator", "Profiles"),
        ] {
            assert!(
                parse_link(url)
                    .unwrap_err()
                    .user_message()
                    .contains(explanation)
            );
        }
    }

    #[test]
    fn routing_rejects_unverified_content() {
        for link in [
            "https://www.tiktok.com/@creator/video/7622412096440929566",
            "tiktok.com/share/video/7622412096440929566/?lang=en",
        ] {
            let link = parse_link(link).unwrap();
            assert_eq!(link.source, Source::TikTok);
            assert!(link.url.starts_with("https://www.tiktok.com/"));
        }
        for link in [
            "https://vm.tiktok.com/abc/extra",
            "https://www.tiktok.com/t/abc/extra",
            "https://www.tiktok.com/@c/photo/123",
            "https://www.tiktok.com/@c/live",
            "https://www.tiktok.com/@c",
            "https://www.tiktok.com/@c/video/123/extra",
            "https://www.tiktok.com/@c/video/nope",
        ] {
            assert!(parse_link(link).is_err(), "{link}");
        }
    }

    #[test]
    fn errors_are_classified_without_echoing_helper_output() {
        for (raw, expected) in [
            (
                "HTTP 429 token=secret",
                EngineError::RateLimited(Source::TikTok),
            ),
            (
                "status 100002 token=secret",
                EngineError::Unavailable(Source::TikTok),
            ),
            (
                "Private video token=secret",
                EngineError::SignInNeeded(Source::TikTok),
            ),
            (
                "HTTP 403 token=secret",
                EngineError::Restricted(Source::TikTok),
            ),
            (
                "Failed to resolve token=secret",
                EngineError::Network(Source::TikTok),
            ),
        ] {
            let error = explain(raw);
            assert_eq!(error, expected);
            assert!(!error.user_message().contains("secret"));
        }
        let error = explain("ERROR: Sign in https://cdn.example/?secret=token");
        assert!(!error.needs_session());
        assert!(!error.user_message().starts_with("YouTube"));
        assert!(!error.user_message().contains("secret"));
        assert!(
            !explain("ERROR: odd failure ?secret=token")
                .user_message()
                .contains("secret")
        );
    }

    #[test]
    fn only_muxed_tiktok_video_with_audio_is_offered() {
        let link = parse_link("https://www.tiktok.com/@creator/video/123").unwrap();
        let json = r#"{"id":"123","extractor_key":"TikTok","title":null,"formats":[{"format_id":"ok","width":540,"height":958,"vcodec":"h264","acodec":"aac"},{"format_id":"unknown","vcodec":"h264","acodec":"aac"},{"format_id":"bad","vcodec":"bytevc2","acodec":"aac"},{"format_id":"silent","vcodec":"h264","acodec":"none"}]}"#;
        let video = crate::engine::media::parse_for(json, &link).unwrap();
        assert_eq!(video.source, Source::TikTok);
        assert_eq!(video.source_url, link.url);
        assert_eq!(video.title, "Video 123");
        assert_eq!(video.formats.len(), 2);
        assert_eq!(video.formats[0].quality(), "540 × 958");
        assert!(!video.formats[1].quality().contains("0p"));
        let parse = |json: &str| crate::engine::media::parse_for(json, &link);
        assert!(parse(&json.replace("TikTok", "Generic")).is_err());
        assert!(parse(r#"{"_type":"playlist","entries":[]}"#).is_err());
        assert!(parse(r#"{"extractor_key":"TikTok","formats":[{"format_id":"audio","vcodec":"none","acodec":"aac"}]}"#).is_err());
    }

    #[test]
    fn cancelled_resolution_does_not_start_a_helper() {
        assert!(
            resolve("https://vm.tiktok.com/ABC/", &AtomicBool::new(true))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn embedded_resolver_runs_without_network_for_direct_video() {
        if !crate::tools::ready() {
            return;
        }
        let link = "https://www.tiktok.com/@creator/video/123";
        assert_eq!(
            resolve(link, &AtomicBool::new(false))
                .unwrap()
                .map(|link| link.url)
                .as_deref(),
            Some(link)
        );
    }
}
