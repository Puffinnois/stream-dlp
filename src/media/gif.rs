//! Local, bounded two-pass GIF encoding with FFmpeg.
use crate::process::{self, Stream};
use std::{
    path::Path,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Settings {
    pub start: f64,
    pub length: f64,
    pub width: u32,
    pub fps: u32,
    pub colors: u32,
    pub smooth: bool,
    pub repeat: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            start: 0.,
            length: 5.,
            width: 480,
            fps: 15,
            colors: 128,
            smooth: false,
            repeat: true,
        }
    }
}
impl Settings {
    pub fn validate(&self, duration: f64) -> Result<(), String> {
        if !duration.is_finite()
            || !self.start.is_finite()
            || !self.length.is_finite()
            || self.start < 0.
            || self.length < 0.1
            || self.length > 60.
            || self.start + self.length > duration + 0.001
        {
            return Err(
                "Choose a clip between 0.1 and 60 seconds, within the source video.".into(),
            );
        }
        if !(160..=960).contains(&self.width)
            || !(5..=30).contains(&self.fps)
            || !(32..=256).contains(&self.colors)
        {
            return Err("GIF settings are outside the supported range.".into());
        }
        Ok(())
    }
    fn filter(&self) -> String {
        // Fit both dimensions, preserve aspect ratio, and never upscale.
        format!(
            "fps={},scale=w='min({},iw)':h='min({},ih)':force_original_aspect_ratio=decrease:flags=lanczos",
            self.fps, self.width, self.width
        )
    }
}
pub fn convert(
    source: &Path,
    destination: &Path,
    settings: &Settings,
    duration: f64,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(f32),
) -> Result<u64, String> {
    settings.validate(duration)?;
    if destination.exists() {
        return Err(
            "That filename already exists. Choose a new name; existing files are never replaced."
                .into(),
        );
    }
    let folder = destination.parent().ok_or("Choose a destination folder.")?;
    let work =
        tempfile::tempdir_in(folder).map_err(|e| format!("Cannot write to this folder: {e}"))?;
    let palette = work.path().join("palette.png");
    let output = work.path().join("output.gif");
    for pass in 0..2 {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let mut command = Command::new(crate::tools::path("ffmpeg"));
        command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-y",
                "-filter_threads",
                "1",
                "-filter_complex_threads",
                "1",
                "-threads",
                "2",
                "-protocol_whitelist",
                "file",
                "-ss",
                &settings.start.to_string(),
                "-t",
                &settings.length.to_string(),
                "-i",
            ])
            .arg(source);
        if pass == 0 {
            command
                .args([
                    "-map",
                    "0:V:0",
                    "-vf",
                    &format!(
                        "{},palettegen=max_colors={}:stats_mode=diff",
                        settings.filter(),
                        settings.colors
                    ),
                    "-frames:v",
                    "1",
                ])
                .arg(&palette);
        } else {
            let dither = if settings.smooth {
                "sierra2_4a"
            } else {
                "bayer:bayer_scale=3"
            };
            command
                .arg("-i")
                .arg(&palette)
                .args([
                    "-filter_complex",
                    &format!(
                        "[0:V:0]{}[v];[v][1:v]paletteuse=dither={dither}:diff_mode=rectangle[out]",
                        settings.filter()
                    ),
                    "-map",
                    "[out]",
                    "-an",
                    "-loop",
                    if settings.repeat { "0" } else { "-1" },
                    "-threads",
                    "2",
                ])
                .arg(&output);
        }
        command.args(["-progress", "pipe:1", "-nostats"]);
        run(&mut command, cancel, |seconds| {
            let fraction = (seconds / settings.length).clamp(0., 1.) as f32;
            on_progress(if pass == 0 {
                fraction * 0.35
            } else {
                0.35 + fraction * 0.64
            });
        })?;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    let size = std::fs::metadata(&output).map_err(|e| e.to_string())?.len();
    if size == 0 {
        return Err("FFmpeg produced an empty GIF.".into());
    }
    // Atomic no-clobber publication on the same filesystem, with no partial destination.
    tempfile::TempPath::try_from_path(output)
        .map_err(|e| e.to_string())?
        .persist_noclobber(destination)
        .map_err(|e| format!("Could not save GIF (choose a new filename): {e}"))?;
    Ok(size)
}
fn run(
    command: &mut Command,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f64),
) -> Result<(), String> {
    let mut errors = String::new();
    let status = process::Helper::new(command, cancel)
        .time_limit(Duration::from_secs(600))
        .output_limit(2_000_000)
        .lines(|stream, line| match stream {
            Stream::Stdout => {
                if let Some(time) = line
                    .strip_prefix("out_time_us=")
                    .and_then(|s| s.parse::<f64>().ok())
                {
                    progress(time / 1_000_000.);
                }
            }
            Stream::Stderr => {
                if errors.len() < 4000 {
                    errors.push_str(line);
                    errors.push('\n');
                }
            }
        })
        .map_err(|error| match error {
            process::Error::Start(e) => {
                format!("Cannot start FFmpeg: {e}. {}", crate::tools::help())
            }
            process::Error::Cancelled => "Cancelled".into(),
            process::Error::TimedOut => {
                "Conversion timed out. Try a smaller or shorter clip.".into()
            }
            process::Error::TooMuchOutput => "FFmpeg generated too much diagnostic output.".into(),
            process::Error::Wait(e) => format!("Could not monitor FFmpeg: {e}"),
        })?;
    if !status.success() {
        let errors: String = errors.chars().take(4000).collect();
        return Err(format!(
            "FFmpeg could not convert this clip. {}",
            errors.trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_clip_and_resource_limits() {
        let mut s = Settings::default();
        assert!(s.validate(10.).is_ok());
        s.start = 8.;
        assert!(s.validate(10.).is_err());
        s.start = f64::NAN;
        assert!(s.validate(10.).is_err());
        s.start = 0.;
        s.length = 61.;
        assert!(s.validate(100.).is_err());
        s.length = 5.;
        s.width = 4000;
        assert!(s.validate(10.).is_err());
    }
    #[test]
    fn ffmpeg_conversion_and_no_clobber() {
        let exe = crate::tools::path("ffmpeg");
        if !exe.exists() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("test clip.mp4");
        let status = process::isolate(
            Command::new(exe)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=320x180:rate=10",
                    "-t",
                    "1",
                    "-pix_fmt",
                    "yuv420p",
                ])
                .arg(&source),
        )
        .status()
        .unwrap();
        assert!(status.success());
        let destination = dir.path().join("clip.gif");
        let cancel = AtomicBool::new(false);
        let settings = Settings {
            length: 0.8,
            start: 0.1,
            ..Settings::default()
        };
        assert!(convert(&source, &destination, &settings, 1., &cancel, |_| {}).unwrap() > 0);
        let original = std::fs::read(&destination).unwrap();
        assert!(original.starts_with(b"GIF89a"));
        assert!(convert(&source, &destination, &settings, 1., &cancel, |_| {}).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), original);
        cancel.store(true, Ordering::Relaxed);
        let cancelled = dir.path().join("cancelled.gif");
        assert!(convert(&source, &cancelled, &settings, 1., &cancel, |_| {}).is_err());
        assert!(!cancelled.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
