//! One video frame as RGB pixels, extracted with FFmpeg within time and size limits.
use crate::process;
use std::{path::Path, process::Command, sync::atomic::AtomicBool, time::Duration};

pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 270;
pub const FRAME_BYTES: usize = WIDTH * HEIGHT * 3;
pub fn extract(path: &Path, seconds: Option<f64>, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    // Scale before sampling a representative poster, keeping the sample buffer small.
    let mut filter = "scale=w='max(1,min(480,270*dar))':h='max(1,min(270,480/dar))',setsar=1,pad=480:270:(ow-iw)/2:(oh-ih)/2".to_string();
    if seconds.is_none() {
        filter.push_str(",thumbnail=30");
    }
    let seek = format!("{:.1}", seconds.unwrap_or(0.));
    let output = process::Helper::new(
        Command::new(crate::tools::path("ffmpeg"))
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-threads",
                "1",
                "-filter_threads",
                "1",
                "-protocol_whitelist",
                "file",
                "-ss",
                &seek,
                "-i",
            ])
            .arg(path)
            .args([
                "-map",
                "0:V:0",
                "-frames:v",
                "1",
                "-an",
                "-sn",
                "-vf",
                &filter,
                "-threads",
                "1",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "pipe:1",
            ]),
        cancel,
    )
    .time_limit(Duration::from_secs(10))
    .output_limit(FRAME_BYTES as u64)
    .ignore_stderr()
    .output()
    .map_err(|error| match error {
        process::Error::Start(_) => "Install the download tools to enable video previews.".into(),
        process::Error::Cancelled | process::Error::TimedOut | process::Error::TooMuchOutput => {
            "Preview cancelled or timed out. You can still open the video.".into()
        }
        process::Error::Wait(error) => error.to_string(),
    })?;
    if !output.status.success() || output.stdout.len() != FRAME_BYTES {
        return Err("This frame could not be previewed. You can still open the video.".into());
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    #[test]
    fn ffmpeg_previews_seek_letterbox_and_handle_short_or_invalid_sources() {
        let exe = crate::tools::path("ffmpeg");
        if !exe.exists() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("portrait clip #1 🐦.mp4");
        let status = process::isolate(
            Command::new(exe)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=90x160:rate=10",
                    "-t",
                    "0.8",
                    "-pix_fmt",
                    "yuv420p",
                ])
                .arg(&path),
        )
        .status()
        .unwrap();
        assert!(status.success());
        let cancel = AtomicBool::new(false);
        let first = extract(&path, Some(0.), &cancel).unwrap();
        let later = extract(&path, Some(0.5), &cancel).unwrap();
        assert_eq!(first.len(), FRAME_BYTES);
        assert_ne!(first, later, "Seeking must display a different moment");
        assert!(
            first[..3].iter().all(|v| *v < 5),
            "Portrait video has side bars"
        );
        assert!(
            first.chunks_exact(3).any(|p| p[0] > 100),
            "Preview contains video pixels"
        );
        assert_eq!(
            extract(&path, None, &cancel).unwrap().len(),
            FRAME_BYTES,
            "Poster sampling must flush clips with fewer than 30 frames"
        );
        assert!(extract(&path, Some(99.), &cancel).is_err());
        let invalid = dir.path().join("broken.mp4");
        std::fs::write(&invalid, b"not video").unwrap();
        assert!(extract(&invalid, None, &cancel).is_err());
        cancel.store(true, Ordering::Relaxed);
        assert!(extract(&path, None, &cancel).is_err());
    }
}
