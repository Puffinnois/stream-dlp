//! Local video files: listing a folder's videos and checking a file with FFprobe.
use crate::process;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub const EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "m4v", "avi", "mpeg", "mpg", "mts", "m2ts", "3gp", "ogv", "flv",
    "wmv",
];

#[derive(Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub bytes: u64,
}
#[derive(Debug)]
pub struct Video {
    pub path: PathBuf,
    pub bytes: u64,
    pub container: String,
    pub duration: Option<f64>,
}

pub fn scan(folder: &Path, cancel: &AtomicBool) -> Result<(Vec<Entry>, bool), String> {
    let mut entries = Vec::new();
    let mut truncated = false;
    for item in std::fs::read_dir(folder)
        .map_err(|_| "Cannot read this folder. Check that it still exists and you have access.")?
    {
        if cancel.load(Ordering::Relaxed) {
            return Err("Folder scan cancelled.".into());
        }
        let Ok(item) = item else {
            continue;
        };
        let path = item.path();
        if !path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| EXTENSIONS.iter().any(|e| s.eq_ignore_ascii_case(e)))
        {
            continue;
        }
        let Ok(meta) = item.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if entries.len() == 1000 {
            truncated = true;
            break;
        }
        entries.push(Entry {
            path,
            bytes: meta.len(),
        });
    }
    entries.sort_by_key(|entry| {
        entry
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    });
    Ok((entries, truncated))
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<Stream>,
    format: ProbeFormat,
}
#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    #[serde(default)]
    disposition: Disposition,
}
#[derive(Deserialize, Default)]
struct Disposition {
    #[serde(default)]
    attached_pic: u32,
}
#[derive(Deserialize)]
struct ProbeFormat {
    format_name: String,
    duration: Option<String>,
}
fn parse_probe(path: PathBuf, bytes: u64, data: &[u8]) -> Result<Video, String> {
    let probe: Probe = serde_json::from_slice(data)
        .map_err(|_| "Could not read video details. Choose another video file.")?;
    if !probe
        .streams
        .iter()
        .any(|s| s.codec_type.as_deref() == Some("video") && s.disposition.attached_pic == 0)
    {
        return Err("This file has no playable video stream. Choose a video rather than an audio file or cover image.".into());
    }
    let duration = probe
        .format
        .duration
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.);
    Ok(Video {
        path,
        bytes,
        container: probe.format.format_name,
        duration,
    })
}
pub fn probe(path: &Path, cancel: &AtomicBool) -> Result<Video, String> {
    // Keep a normal absolute path for OS file-manager/player compatibility.
    // Windows canonicalization produces extended paths that shell apps may reject.
    let path = std::path::absolute(path)
        .map_err(|_| "This video is missing or cannot be accessed. Choose it again.")?;
    let meta = std::fs::metadata(&path).map_err(|_| "Cannot read this video.")?;
    if !meta.is_file() || meta.len() == 0 {
        return Err("Choose a nonempty video file.".into());
    }
    let output = process::Helper::new(
        Command::new(crate::tools::path("ffprobe"))
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file",
                "-probesize",
                "5000000",
                "-analyzeduration",
                "5000000",
                "-show_entries",
                "format=format_name,duration:stream=codec_type:stream_disposition=attached_pic",
                "-of",
                "json",
                "-i",
            ])
            .arg(&path),
        cancel,
    )
    .time_limit(Duration::from_secs(15))
    .output_limit(65_536)
    .ignore_stderr()
    .output()
    .map_err(|error| match error {
        process::Error::Start(_) => {
            "Cannot start FFprobe. Install the download tools and try again."
        }
        process::Error::Cancelled => "Video check cancelled.",
        process::Error::TimedOut => "Video check timed out. Choose another file.",
        process::Error::TooMuchOutput => "Video details were too large. Choose another file.",
        process::Error::Wait(_) => "The video check failed.",
    })?;
    if !output.status.success() {
        return Err("This file could not be read as a video. Choose another file.".into());
    }
    parse_probe(path, meta.len(), &output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_scan_is_scoped_sorted_and_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "z.MP4",
            "a video #1 🐦.mkv",
            "cookies.txt",
            "movie.mp4.part",
        ] {
            std::fs::write(dir.path().join(name), b"fixture").unwrap();
        }
        std::fs::create_dir(dir.path().join("subfolder.mp4")).unwrap();
        std::fs::write(dir.path().join("subfolder.mp4/nested.mp4"), b"fixture").unwrap();
        let (entries, truncated) = scan(dir.path(), &AtomicBool::new(false)).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(!truncated);
        assert!(entries[0].path.ends_with("a video #1 🐦.mkv"));
        assert!(scan(dir.path(), &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn rejects_audio_and_cover_art_and_accepts_missing_duration() {
        for streams in [
            r#"[{"codec_type":"audio"}]"#,
            r#"[{"codec_type":"video","disposition":{"attached_pic":1}}]"#,
        ] {
            let json = format!(r#"{{"streams":{streams},"format":{{"format_name":"mp3"}}}}"#);
            assert!(parse_probe("video".into(), 10, json.as_bytes()).is_err());
        }
        let video = parse_probe("video".into(), 10, br#"{"streams":[{"codec_type":"video"}],"format":{"format_name":"matroska,webm","duration":"N/A"}}"#).unwrap();
        assert!(video.duration.is_none());
    }
    #[test]
    #[ignore = "requires installed FFmpeg and FFprobe"]
    fn local_library_probe_checks_real_video_and_invalid_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip #1 🐦.mp4");
        let status = process::isolate(
            Command::new(crate::tools::path("ffmpeg"))
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=size=64x64:rate=1",
                    "-t",
                    "1",
                    "-c:v",
                    "libx264",
                ])
                .arg(&path),
        )
        .status()
        .unwrap();
        assert!(status.success());
        let video = probe(&path, &AtomicBool::new(false)).unwrap();
        assert!(video.duration.unwrap() > 0.);
        assert!(video.bytes > 0);
        assert!(probe(&path, &AtomicBool::new(true)).is_err());
        std::fs::write(dir.path().join("invalid.mp4"), b"not a video").unwrap();
        assert!(probe(&dir.path().join("invalid.mp4"), &AtomicBool::new(false)).is_err());
        if let Some(folder) = std::env::var_os("STREAM_DLP_VIDEO_TEST_FOLDER") {
            let (entries, _) = scan(Path::new(&folder), &AtomicBool::new(false)).unwrap();
            assert!(
                !entries.is_empty(),
                "Explicitly selected test folder has no videos"
            );
            for entry in entries.iter().take(2) {
                let video = probe(&entry.path, &AtomicBool::new(false)).unwrap();
                assert_eq!(video.bytes, entry.bytes);
                assert_eq!(video.path, entry.path);
            }
        }
    }
}
