//! Video frames for the in-app player: FFmpeg decodes a file from a chosen moment and pipes
//! scaled frames, which a worker thread turns into RGBA pixels as fast as they are taken.
use crate::process;
use serde::Deserialize;
use std::{
    io::{BufReader, Read},
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::AtomicBool,
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

/// The longest side of a decoded frame. The player is never drawn larger.
pub const MAX_SIDE: usize = 960;
/// Frames decoded ahead of the one on screen. FFmpeg waits while this many are untaken, so a
/// paused video costs nothing.
const AHEAD: usize = 3;
/// The most frames a second a run is asked for; a player that falls behind asks for fewer.
pub const MAX_RATE: f64 = 60.;

pub const NO_TOOLS: &str = "Install the download tools to play videos here.";
pub const NOT_PLAYABLE: &str =
    "This file could not be played here. You can still open it in your video player.";

/// What the player needs to know about a file besides its frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Info {
    /// `None` when the container does not say; the position bar then cannot seek.
    pub duration: Option<f64>,
    pub audio: bool,
}

fn parse_info(data: &[u8]) -> Option<Info> {
    #[derive(Deserialize)]
    struct Probe {
        #[serde(default)]
        streams: Vec<Stream>,
        #[serde(default)]
        format: Format,
    }
    #[derive(Deserialize)]
    struct Stream {
        codec_type: Option<String>,
    }
    #[derive(Deserialize, Default)]
    struct Format {
        duration: Option<String>,
    }
    let probe: Probe = serde_json::from_slice(data).ok()?;
    Some(Info {
        duration: probe
            .format
            .duration
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.),
        audio: probe
            .streams
            .iter()
            .any(|s| s.codec_type.as_deref() == Some("audio")),
    })
}

/// Reads a file's duration and whether it has sound; `None` when FFprobe cannot tell.
pub fn probe(path: &Path, cancel: &AtomicBool) -> Option<Info> {
    let output = process::Helper::new(
        Command::new(crate::tools::path("ffprobe"))
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file",
                "-show_entries",
                "format=duration:stream=codec_type",
                "-of",
                "json",
                "-i",
            ])
            .arg(path),
        cancel,
    )
    .time_limit(Duration::from_secs(15))
    .output_limit(65_536)
    .ignore_stderr()
    .output()
    .ok()
    .filter(|output| output.status.success())?;
    parse_info(&output.stdout)
}

/// One picture, as premultiplied RGBA (always opaque).
pub struct Frame {
    /// Its moment in the video.
    pub seconds: f64,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub enum Step {
    Frame(Frame),
    /// The video ended, or decoding failed with a message for the person.
    End(Result<(), String>),
}

/// One run of FFmpeg from a moment of the video to its end. Dropping it stops the helper, so
/// seeking and switching videos start a new one.
pub struct Decoder {
    steps: Receiver<Step>,
    running: Arc<process::Running>,
}

impl Decoder {
    /// Starts decoding at `seconds`, at `rate` frames a second at most. Nothing here waits: a
    /// worker starts FFmpeg and reads. `wake` is called from that worker whenever a step is
    /// ready.
    pub fn start(path: &Path, seconds: f64, rate: f64, wake: impl Fn() + Send + 'static) -> Self {
        let (tx, steps) = mpsc::sync_channel(AHEAD);
        let running = Arc::new(process::Running::default());
        let (path, worker) = (path.to_owned(), running.clone());
        std::thread::spawn(move || {
            let result = decode(&path, seconds, rate, &worker, &tx, &wake);
            if !worker.stopped() {
                let _ = tx.send(Step::End(result));
                wake();
            }
            worker.stop();
        });
        Self { steps, running }
    }

    /// The next step when one is ready. After `Step::End` there is nothing more.
    pub fn next(&self) -> Option<Step> {
        self.steps.try_recv().ok()
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        self.running.stop();
    }
}

fn command(path: &Path, seconds: f64, rate: f64) -> Command {
    let mut command = Command::new(crate::tools::path("ffmpeg"));
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-protocol_whitelist",
        "file",
    ]);
    if seconds > 0. {
        command.args(["-ss", &format!("{seconds:.3}")]);
    }
    // Never upscaled; even sides for 4:2:0; one color convention whatever the source uses.
    let filter = format!(
        "scale=w='min({MAX_SIDE},iw)':h='min({MAX_SIDE},ih)':force_original_aspect_ratio=decrease:force_divisible_by=2:flags=bilinear:out_color_matrix=bt709:out_range=tv,setsar=1,format=yuv420p"
    );
    command.arg("-i").arg(path).args([
        "-map",
        "0:V:0",
        "-an",
        "-sn",
        "-dn",
        "-vf",
        &filter,
        "-fpsmax",
        &rate.clamp(1., MAX_RATE).to_string(),
        "-f",
        "yuv4mpegpipe",
        "pipe:1",
    ]);
    command
}

fn decode(
    path: &Path,
    seconds: f64,
    rate: f64,
    running: &process::Running,
    tx: &SyncSender<Step>,
    wake: &impl Fn(),
) -> Result<(), String> {
    let stdout = running
        .start(&mut command(path, seconds, rate))
        .map_err(|error| match error {
            process::Error::Start(_) => NO_TOOLS.to_string(),
            _ => NOT_PLAYABLE.to_string(),
        })?;
    let mut stdout = BufReader::with_capacity(1 << 16, stdout);
    let header = read_line(&mut stdout)
        .and_then(|line| Header::parse(&line))
        .ok_or(NOT_PLAYABLE)?;
    let mut yuv = vec![0; header.frame_bytes()];
    let mut count = 0_u64;
    loop {
        // Each frame follows a `FRAME` line; none means the video ended.
        if read_line(&mut stdout).is_none() || stdout.read_exact(&mut yuv).is_err() {
            return if count > 0 || running.stopped() {
                Ok(())
            } else {
                Err(NOT_PLAYABLE.into())
            };
        }
        let mut rgba = vec![0; header.width * header.height * 4];
        to_rgba(header.width, header.height, &yuv, &mut rgba);
        let frame = Frame {
            seconds: seconds + count as f64 / header.rate,
            width: header.width,
            height: header.height,
            rgba,
        };
        count += 1;
        // Waits while the player has frames ahead; fails once the player dropped this run.
        if tx.send(Step::Frame(frame)).is_err() {
            return Ok(());
        }
        wake();
    }
}

/// One header or `FRAME` line, without its newline.
fn read_line(source: &mut impl Read) -> Option<String> {
    let mut line = Vec::new();
    let mut byte = [0];
    loop {
        source.read_exact(&mut byte).ok()?;
        if byte[0] == b'\n' {
            return String::from_utf8(line).ok();
        }
        if line.len() == 256 {
            return None;
        }
        line.push(byte[0]);
    }
}

/// The stream header: `YUV4MPEG2 W640 H360 F30:1 Ip A1:1 C420jpeg`.
#[derive(Debug, PartialEq)]
struct Header {
    width: usize,
    height: usize,
    /// Frames per second.
    rate: f64,
}

impl Header {
    fn parse(line: &str) -> Option<Self> {
        let mut fields = line.split(' ');
        if fields.next() != Some("YUV4MPEG2") {
            return None;
        }
        let (mut width, mut height, mut rate) = (None, None, None);
        for field in fields {
            let (tag, value) = field.split_at_checked(1)?;
            match tag {
                "W" => width = value.parse().ok(),
                "H" => height = value.parse().ok(),
                "F" => {
                    let (n, d) = value.split_once(':')?;
                    let (n, d) = (n.parse::<f64>().ok()?, d.parse::<f64>().ok()?);
                    rate = Some(n / d);
                }
                // 8-bit 4:2:0 in any chroma siting; anything else is not what was asked for.
                "C" if !matches!(value, "420" | "420jpeg" | "420mpeg2" | "420paldv") => {
                    return None;
                }
                _ => {}
            }
        }
        let (width, height) = (width?, height?);
        if width == 0 || height == 0 || width > 4 * MAX_SIDE || height > 4 * MAX_SIDE {
            return None;
        }
        Some(Self {
            width,
            height,
            rate: rate
                .filter(|rate| rate.is_finite() && (1. ..=240.).contains(rate))
                .unwrap_or(30.),
        })
    }

    fn chroma(&self) -> (usize, usize) {
        (self.width.div_ceil(2), self.height.div_ceil(2))
    }

    fn frame_bytes(&self) -> usize {
        let (width, height) = self.chroma();
        self.width * self.height + 2 * width * height
    }
}

/// Planar 4:2:0 in BT.709 limited range (what the scale filter is asked for) to opaque RGBA.
fn to_rgba(width: usize, height: usize, yuv: &[u8], rgba: &mut [u8]) {
    let chroma_width = width.div_ceil(2);
    let (luma, chroma) = yuv.split_at(width * height);
    let (blue, red) = chroma.split_at(chroma.len() / 2);
    let byte = |value: i32| (value >> 8).clamp(0, 255) as u8;
    for (row, out) in rgba.chunks_exact_mut(width * 4).enumerate() {
        let luma = &luma[row * width..][..width];
        let blue = &blue[row / 2 * chroma_width..][..chroma_width];
        let red = &red[row / 2 * chroma_width..][..chroma_width];
        for (x, pixel) in out.chunks_exact_mut(4).enumerate() {
            let y = 298 * (i32::from(luma[x]) - 16) + 128;
            let u = i32::from(blue[x / 2]) - 128;
            let v = i32::from(red[x / 2]) - 128;
            pixel[0] = byte(y + 459 * v);
            pixel[1] = byte(y - 55 * u - 136 * v);
            pixel[2] = byte(y + 541 * u);
            pixel[3] = 255;
        }
    }
}

/// A short synthetic clip with a moving picture and a tone, for the playback tests.
#[cfg(test)]
pub(crate) fn test_clip(folder: &Path, name: &str, size: &str, sound: bool) -> std::path::PathBuf {
    let path = folder.join(name);
    let mut command = Command::new(crate::tools::path("ffmpeg"));
    command.args(["-v", "error", "-f", "lavfi", "-i"]);
    command.arg(format!("testsrc2=size={size}:rate=20"));
    if sound {
        command.args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"]);
    }
    command.args(["-t", "2", "-pix_fmt", "yuv420p", "-g", "10"]);
    let status = process::isolate(command.arg(&path)).status().unwrap();
    assert!(status.success());
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn the_stream_header_gives_the_size_and_rate() {
        assert_eq!(
            Header::parse("YUV4MPEG2 W640 H360 F30000:1001 Ip A1:1 C420mpeg2 XCOLORRANGE=LIMITED"),
            Some(Header {
                width: 640,
                height: 360,
                rate: 30000. / 1001.,
            })
        );
        let odd = Header::parse("YUV4MPEG2 W3 H5 F0:0 C420jpeg").unwrap();
        assert_eq!(odd.rate, 30., "An unusable rate falls back");
        assert_eq!(odd.frame_bytes(), 15 + 2 * 6);
        for bad in [
            "",
            "RIFF W640 H360",
            "YUV4MPEG2 W640",
            "YUV4MPEG2 W0 H360 F30:1",
            "YUV4MPEG2 W640 H360 F30:1 C444",
            "YUV4MPEG2 W640 H360 F30:1 C420p10",
            "YUV4MPEG2 W99999 H360 F30:1",
        ] {
            assert_eq!(Header::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn limited_range_pixels_become_full_range_rgb() {
        let pixel = |y: u8, u: u8, v: u8| {
            let mut rgba = [0; 16];
            to_rgba(2, 2, &[y, y, y, y, u, v], &mut rgba);
            assert_eq!(rgba[..4], rgba[12..], "One chroma sample covers the block");
            [rgba[0], rgba[1], rgba[2], rgba[3]]
        };
        assert_eq!(pixel(16, 128, 128), [0, 0, 0, 255]);
        assert_eq!(pixel(235, 128, 128), [255, 255, 255, 255]);
        assert_eq!(pixel(0, 128, 128), [0, 0, 0, 255], "Below black is clamped");
        // BT.709 red, green and blue.
        for (yuv, rgb) in [
            ((63, 102, 240), [255, 0, 0]),
            ((173, 42, 26), [0, 255, 0]),
            ((32, 240, 118), [0, 0, 255]),
        ] {
            let [r, g, b, _] = pixel(yuv.0, yuv.1, yuv.2);
            for (got, want) in [r, g, b].into_iter().zip(rgb) {
                assert!(
                    (i32::from(got) - want).abs() <= 3,
                    "{yuv:?} gave {r} {g} {b}"
                );
            }
        }
    }

    #[test]
    fn the_probe_reads_duration_and_sound() {
        let info = parse_info(
            br#"{"streams":[{"codec_type":"video"},{"codec_type":"audio"}],"format":{"duration":"12.5"}}"#,
        )
        .unwrap();
        assert_eq!(
            info,
            Info {
                duration: Some(12.5),
                audio: true
            }
        );
        let silent =
            parse_info(br#"{"streams":[{"codec_type":"video"}],"format":{"duration":"N/A"}}"#);
        assert_eq!(
            silent,
            Some(Info {
                duration: None,
                audio: false
            })
        );
        assert_eq!(parse_info(b"not json"), None);
    }

    /// Takes steps until the end, as fast as they come.
    fn drain(decoder: &Decoder) -> (Vec<Frame>, Result<(), String>) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut frames = Vec::new();
        loop {
            match decoder.next() {
                Some(Step::Frame(frame)) => frames.push(frame),
                Some(Step::End(result)) => return (frames, result),
                None => {
                    assert!(Instant::now() < deadline, "Decoding timed out");
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }

    #[test]
    #[ignore = "requires installed FFmpeg and FFprobe"]
    fn local_playback_decodes_seeks_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        // Portrait, larger than a decoded frame may be, with sound and an awkward name.
        let portrait = test_clip(dir.path(), "portrait clip #1 🐦.mp4", "1080x1920", true);
        assert_eq!(
            probe(&portrait, &cancel),
            Some(Info {
                duration: Some(2.),
                audio: true
            })
        );
        let (frames, result) = drain(&Decoder::start(&portrait, 0., MAX_RATE, || {}));
        assert_eq!(result, Ok(()));
        assert_eq!(frames.len(), 40, "Two seconds at 20 frames a second");
        assert_eq!((frames[0].width, frames[0].height), (540, 960));
        assert_eq!(frames[0].rgba.len(), 540 * 960 * 4);
        assert_eq!(frames[0].seconds, 0.);
        assert!((frames[39].seconds - 1.95).abs() < 1e-9);
        assert_ne!(frames[0].rgba, frames[20].rgba, "The picture moves");
        assert!(
            frames[0]
                .rgba
                .chunks_exact(4)
                .any(|p| p[0] > 200 && p[1] < 80),
            "The test pattern keeps its colors"
        );

        // A seek starts a new run at that moment and shows what the first run showed there.
        let (later, result) = drain(&Decoder::start(&portrait, 1., MAX_RATE, || {}));
        assert_eq!(result, Ok(()));
        assert_eq!(later.len(), 20);
        assert_eq!(later[0].seconds, 1.);
        let differing = later[0]
            .rgba
            .iter()
            .zip(&frames[20].rgba)
            .filter(|(a, b)| a.abs_diff(**b) > 8)
            .count();
        assert!(
            differing < later[0].rgba.len() / 100,
            "{differing} bytes differ"
        );

        // A player that fell behind asks for fewer frames a second.
        let (fewer, result) = drain(&Decoder::start(&portrait, 0., 10., || {}));
        assert_eq!(result, Ok(()));
        assert!((20..=22).contains(&fewer.len()), "{} frames", fewer.len());
        assert!((fewer[1].seconds - 0.1).abs() < 1e-9);

        // Landscape without sound, in another container and codec; never upscaled.
        let landscape = test_clip(dir.path(), "small.webm", "320x180", false);
        assert_eq!(
            probe(&landscape, &cancel).map(|info| info.audio),
            Some(false)
        );
        let (frames, result) = drain(&Decoder::start(&landscape, 0., MAX_RATE, || {}));
        assert_eq!(result, Ok(()));
        assert_eq!((frames[0].width, frames[0].height), (320, 180));

        // An untaken video waits instead of decoding on, and dropping it stops FFmpeg.
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = woken.clone();
        let decoder = Decoder::start(&portrait, 0., MAX_RATE, move || {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        std::thread::sleep(Duration::from_millis(1500));
        let waiting = woken.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(waiting, AHEAD, "Only the frames ahead were decoded");
        let running = decoder.running.clone();
        drop(decoder);
        assert!(running.stopped());

        // What is not a video ends with a message instead of frames.
        let broken = dir.path().join("broken.mp4");
        std::fs::write(&broken, b"not video").unwrap();
        let (frames, result) = drain(&Decoder::start(&broken, 0., MAX_RATE, || {}));
        assert!(frames.is_empty());
        assert_eq!(result, Err(NOT_PLAYABLE.into()));
        assert_eq!(probe(&broken, &cancel), None);
    }
}
