//! What yt-dlp reports about a video and its formats, with the labels the UI shows.
use super::{EngineError, Link, Source};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize)]
pub struct Format {
    pub format_id: String,
    #[serde(default, deserialize_with = "string_or_default")]
    pub ext: String,
    pub height: Option<u32>,
    pub width: Option<u32>,
    pub fps: Option<f64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub filesize: Option<f64>,
    pub filesize_approx: Option<f64>,
    pub abr: Option<f64>,
    pub format_note: Option<String>,
    pub language: Option<String>,
    pub has_drm: Option<bool>,
}

impl Format {
    pub fn video(&self) -> bool {
        self.vcodec.as_deref().is_some_and(|v| v != "none")
    }
    pub fn audio(&self) -> bool {
        self.acodec.as_deref().is_some_and(|v| v != "none")
    }
    pub fn quality(&self) -> String {
        if self.video() {
            let Some(height) = self.height.filter(|h| *h > 0) else {
                return "Video · unknown resolution".into();
            };
            if let Some(width) = self.width.filter(|w| *w > 0)
                && height > width
            {
                return format!("{width} × {height}");
            }
            format!(
                "{}p{}",
                height,
                if positive(self.fps).is_some_and(|fps| fps > 30.) {
                    format!(" · {} fps", self.fps.unwrap_or(0.).round())
                } else {
                    String::new()
                }
            )
        } else {
            positive(self.abr)
                .map(|abr| format!("Audio · {} kbps", abr.round().max(1.)))
                .unwrap_or_else(|| "Audio · unknown bitrate".into())
        }
    }
    pub fn details(&self) -> String {
        let codec = if self.video() {
            self.vcodec.as_deref()
        } else {
            self.acodec.as_deref()
        }
        .unwrap_or("unknown");
        let mut parts = vec![codec.to_owned()];
        if let Some(note) = nonblank(self.format_note.as_deref()) {
            parts.push(note.to_owned());
        }
        if let Some(lang) = nonblank(self.language.as_deref()) {
            parts.push(lang.to_owned());
        }
        parts.push(format!("ID {}", self.format_id));
        if let (Some(w), Some(h)) = (self.width, self.height)
            && w > 0
            && h > 0
        {
            parts.push(format!("{w} × {h}"));
        }
        parts.join(" · ")
    }
    pub fn size(&self) -> String {
        let exact = positive(self.filesize);
        exact
            .or(positive(self.filesize_approx))
            .map(|n| {
                format!(
                    "{}{} MB",
                    if exact.is_none() { "~" } else { "" },
                    if n < 1_048_576. {
                        "<1".into()
                    } else {
                        (n / 1_048_576.).round().to_string()
                    }
                )
            })
            .unwrap_or_else(|| "Size unknown".into())
    }
    pub fn container_label(&self) -> String {
        nonblank(Some(&self.ext))
            .map(str::to_uppercase)
            .unwrap_or_else(|| "Unknown container".into())
    }
    pub fn selector(&self) -> String {
        if self.video() && !self.audio() {
            let audio = if self.ext == "mp4" {
                "bestaudio[ext=m4a]/bestaudio"
            } else if self.ext == "webm" {
                "bestaudio[ext=webm]/bestaudio"
            } else {
                "bestaudio"
            };
            format!("{}+({audio})", self.format_id)
        } else {
            self.format_id.clone()
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Video {
    #[serde(skip)]
    pub source: Source,
    #[serde(skip)]
    pub source_url: String,
    pub id: Option<String>,
    pub extractor_key: Option<String>,
    #[serde(default, deserialize_with = "string_or_default")]
    pub title: String,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    #[serde(default)]
    pub formats: Vec<Format>,
    #[serde(default)]
    pub is_live: bool,
    /// yt-dlp's `_type`: `video`, or a playlist or other collection.
    #[serde(rename = "_type")]
    kind: Option<String>,
    /// Whether the result lists `entries`, as collections do.
    #[serde(default, rename = "entries", deserialize_with = "present")]
    collection: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaIdentity {
    pub source: Source,
    pub source_url: String,
    pub id: Option<String>,
    pub extractor_key: Option<String>,
}

impl MediaIdentity {
    pub fn link(&self) -> Link {
        Link {
            source: self.source,
            url: self.source_url.clone(),
        }
    }
}

impl Video {
    pub fn creator_label(&self) -> &str {
        nonblank(self.uploader.as_deref()).unwrap_or("Unknown creator")
    }

    pub fn duration_label(&self) -> String {
        let Some(duration) = positive(self.duration) else {
            return "Duration unknown".into();
        };
        if duration < 1. {
            return "Less than 1 second".into();
        }
        let seconds = duration as u64;
        if seconds >= 3600 {
            format!(
                "{}:{:02}:{:02}",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            )
        } else {
            format!("{}:{:02}", seconds / 60, seconds % 60)
        }
    }

    pub fn identity(&self) -> MediaIdentity {
        MediaIdentity {
            source: self.source,
            source_url: self.source_url.clone(),
            id: self.id.clone(),
            extractor_key: self.extractor_key.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompletedDownload {
    pub path: PathBuf,
    pub media: MediaIdentity,
}

fn positive(value: Option<f64>) -> Option<f64> {
    value.filter(|n| n.is_finite() && *n > 0.)
}

fn nonblank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

fn string_or_default<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    serde::de::IgnoredAny::deserialize(d).map(|_| true)
}

/// Reads yt-dlp's `--dump-single-json` output, keeping the downloadable formats with the
/// best video first.
pub fn parse_video(json: &str) -> Result<Video, EngineError> {
    let mut video: Video = serde_json::from_str(json)
        .map_err(|e| EngineError::Helper(format!("Could not read the video details: {e}")))?;
    if video.collection || video.kind.as_deref().is_some_and(|kind| kind != "video") {
        return Err(EngineError::Unsupported(
            "Choose one video; profiles and collections are not supported.".into(),
        ));
    }
    if video.is_live {
        return Err(EngineError::Unsupported(
            "This is a live stream. Try again after the broadcast has ended.".into(),
        ));
    }
    if video.title.trim().is_empty() {
        video.title = nonblank(video.id.as_deref())
            .map(|id| format!("Video {id}"))
            .unwrap_or_else(|| "Untitled video".into());
    }
    video
        .formats
        .retain(|f| f.has_drm != Some(true) && (f.video() || f.audio()));
    video.formats.sort_by(|a, b| {
        b.video()
            .cmp(&a.video())
            .then(b.height.cmp(&a.height))
            .then(b.fps.unwrap_or(0.).total_cmp(&a.fps.unwrap_or(0.)))
            .then(a.ext.cmp(&b.ext))
            .then(b.abr.unwrap_or(0.).total_cmp(&a.abr.unwrap_or(0.)))
    });
    if video.formats.is_empty() {
        return Err(EngineError::Unsupported(
            "No downloadable formats were returned for this video.".into(),
        ));
    }
    Ok(video)
}

/// [`parse_video`] for `link`, applying its site's rules.
pub(super) fn parse_for(json: &str, link: &Link) -> Result<Video, EngineError> {
    let mut video = parse_video(json)?;
    video.source = link.source;
    video.source_url = link.url.clone();
    link.source.check(&mut video)?;
    Ok(video)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_metadata_has_honest_labels_for_both_platforms() {
        for (url, source, extractor) in [
            (
                "https://www.youtube.com/watch?v=abcdefghijk",
                Source::YouTube,
                "Youtube",
            ),
            (
                "https://www.tiktok.com/@creator/video/123",
                Source::TikTok,
                "TikTok",
            ),
        ] {
            let link = Link {
                source,
                url: url.into(),
            };
            for missing in [
                serde_json::json!({}),
                serde_json::json!({
                    "title": null, "id": null, "uploader": null, "duration": null
                }),
                serde_json::json!({
                    "title": "  ", "id": " ", "uploader": "\t", "duration": 0
                }),
            ] {
                let mut fixture = missing;
                fixture["extractor_key"] = extractor.into();
                fixture["formats"] = serde_json::json!([{
                    "format_id": "muxed", "vcodec": "h264", "acodec": "aac",
                    "ext": null, "height": 0, "width": 0, "filesize": 0
                }]);
                let video = parse_for(&fixture.to_string(), &link).unwrap();
                assert_eq!(video.title, "Untitled video");
                assert_eq!(video.creator_label(), "Unknown creator");
                assert_eq!(video.duration_label(), "Duration unknown");
                let format = &video.formats[0];
                assert_eq!(format.quality(), "Video · unknown resolution");
                assert_eq!(format.container_label(), "Unknown container");
                assert_eq!(format.size(), "Size unknown");
                assert!(!format.details().contains("0 × 0"));
                // Missing display metadata must not change download selection.
                assert_eq!(format.selector(), "muxed");
            }
        }
    }

    #[test]
    fn valid_and_partial_metadata_keep_useful_details() {
        let mut video = parse_video(
            r#"{"id":"123","uploader":" Creator ","duration":65,
            "formats":[{"format_id":"v","vcodec":"h264","acodec":"aac",
                "height":1080,"width":1920,"fps":60,"ext":"mp4","filesize":2097152}]}"#,
        )
        .unwrap();
        assert_eq!(video.title, "Video 123");
        assert_eq!(video.creator_label(), "Creator");
        for (duration, label) in [
            (65., "1:05"),
            (3661., "1:01:01"),
            (0.5, "Less than 1 second"),
            (-1., "Duration unknown"),
            (f64::NAN, "Duration unknown"),
        ] {
            video.duration = Some(duration);
            assert_eq!(video.duration_label(), label);
        }
        let format = &mut video.formats[0];
        assert_eq!(format.quality(), "1080p · 60 fps");
        assert_eq!(format.container_label(), "MP4");
        assert_eq!(format.size(), "2 MB");
        format.filesize = Some(-1.);
        format.filesize_approx = Some(3145728.);
        assert_eq!(format.size(), "~3 MB");
        format.filesize = Some(100.);
        assert_eq!(format.size(), "<1 MB");
        format.filesize = Some(f64::INFINITY);
        format.filesize_approx = None;
        assert_eq!(format.size(), "Size unknown");
        format.width = Some(0);
        assert_eq!(format.quality(), "1080p · 60 fps");
        assert!(!format.details().contains("0 ×"));
        format.height = None;
        assert_eq!(format.quality(), "Video · unknown resolution");
        format.vcodec = Some("none".into());
        for abr in [None, Some(0.), Some(-1.), Some(f64::NAN)] {
            format.abr = abr;
            assert_eq!(format.quality(), "Audio · unknown bitrate");
        }
        format.abr = Some(128.);
        assert_eq!(format.quality(), "Audio · 128 kbps");
    }

    #[test]
    fn inspection_retains_identity_with_optional_extractor_metadata() {
        for (url, extractor, source) in [
            (
                "https://www.youtube.com/watch?v=abcdefghijk",
                "Youtube",
                Source::YouTube,
            ),
            (
                "https://www.tiktok.com/@creator/video/123",
                "TikTok",
                Source::TikTok,
            ),
        ] {
            let json = serde_json::json!({
                "id": "123", "extractor_key": extractor,
                "formats": [{"format_id": "muxed", "vcodec": "h264", "acodec": "aac"}]
            });
            let link = Link {
                source,
                url: url.into(),
            };
            let video = parse_for(&json.to_string(), &link).unwrap();
            assert_eq!(
                video.identity(),
                MediaIdentity {
                    source,
                    source_url: url.into(),
                    id: Some("123".into()),
                    extractor_key: Some(extractor.into()),
                }
            );
            assert_eq!(video.identity().link(), link);
        }
        let video = parse_for(
            r#"{"formats":[{"format_id":"muxed","vcodec":"h264","acodec":"aac"}]}"#,
            &Link {
                source: Source::YouTube,
                url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
            },
        )
        .unwrap();
        assert_eq!(video.identity().id, None);
        assert_eq!(video.identity().extractor_key, None);
    }

    #[test]
    fn collections_and_live_streams_are_refused() {
        for json in [
            r#"{"_type":"playlist","entries":[]}"#,
            r#"{"entries":null,"formats":[{"format_id":"18","vcodec":"avc1","acodec":"mp4a"}]}"#,
            r#"{"is_live":true,"formats":[{"format_id":"18","vcodec":"avc1","acodec":"mp4a"}]}"#,
        ] {
            assert!(matches!(
                parse_video(json),
                Err(EngineError::Unsupported(_))
            ));
        }
        assert!(
            parse_video(
                r#"{"_type":"video","formats":[{"format_id":"18","vcodec":"avc1","acodec":"mp4a"}]}"#
            )
            .is_ok()
        );
    }

    #[test]
    fn filters_storyboards_drm_and_sorts_highest_video_first() {
        let video = parse_video(r#"{"title":"Test","formats":[{"format_id":"sb","vcodec":"none","acodec":"none"},{"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a","abr":128},{"format_id":"18","ext":"mp4","height":360,"vcodec":"avc1","acodec":"mp4a"},{"format_id":"137","ext":"mp4","height":1080,"vcodec":"avc1","acodec":"none"},{"format_id":"drm","vcodec":"avc1","has_drm":true}]}"#).unwrap();
        assert_eq!(video.formats.len(), 3);
        assert_eq!(
            video.formats[0].selector(),
            "137+(bestaudio[ext=m4a]/bestaudio)"
        );
        assert_eq!(video.formats[1].selector(), "18");
        assert_eq!(video.formats[2].selector(), "140");
    }
}
