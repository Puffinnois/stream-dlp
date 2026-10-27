use crate::session::SessionCookies;
use serde::Deserialize;
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    YouTube,
    TikTok,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::YouTube => "YouTube",
            Self::TikTok => "TikTok",
        }
    }
}

pub fn source(link: &str) -> Option<Source> {
    let normalized = normalize_url(link).ok()?;
    let url = url::Url::parse(&normalized).ok()?;
    Some(
        if url.host_str().is_some_and(|host| {
            matches!(host, "www.tiktok.com" | "vm.tiktok.com" | "vt.tiktok.com")
        }) {
            Source::TikTok
        } else {
            Source::YouTube
        },
    )
}

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaIdentity {
    pub source: Source,
    pub source_url: String,
    pub id: Option<String>,
    pub extractor_key: Option<String>,
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

pub fn parse_video(json: &str) -> Result<Video, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Could not read the video details: {e}"))?;
    if value.get("entries").is_some()
        || value
            .get("_type")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t != "video")
    {
        return Err("Choose one video; profiles and collections are not supported.".into());
    }
    let mut video: Video =
        serde_json::from_str(json).map_err(|e| format!("Could not read the video details: {e}"))?;
    if video.is_live {
        return Err("This is a live stream. Try again after the broadcast has ended.".into());
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
        return Err("No downloadable formats were returned for this video.".into());
    }
    Ok(video)
}

fn parse_video_for(json: &str, link: &str) -> Result<Video, String> {
    let mut video = parse_video(json)?;
    video.source_url = link.to_owned();
    if source(link) == Some(Source::TikTok) {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if value["extractor_key"].as_str() != Some("TikTok") {
            return Err("This link did not return an individual TikTok video.".into());
        }
        video.source = Source::TikTok;
        // First TikTok release supports muxed, playable video only. Do not treat
        // an audio-only slideshow or an untested video-only stream as a video.
        video.formats.retain(|f| {
            f.audio() && matches!(f.vcodec.as_deref(), Some("h264" | "h265" | "avc1" | "hevc"))
        });
        if video.formats.is_empty() {
            return Err("TikTok returned no supported video with audio. Photo posts, silent clips, and video-only formats are not supported yet.".into());
        }
    }
    Ok(video)
}

pub fn normalize_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    let full = if input.starts_with("https://") || input.starts_with("http://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = url::Url::parse(&full)
        .map_err(|_| "Paste a valid YouTube or TikTok video link.".to_owned())?;
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err("Paste a standard video link without credentials or a custom port.".into());
    }
    let host = url.host_str().unwrap_or("");
    if matches!(
        host,
        "www.tiktok.com" | "tiktok.com" | "m.tiktok.com" | "vm.tiktok.com" | "vt.tiktok.com"
    ) {
        let parts: Vec<_> = url
            .path()
            .trim_end_matches('/')
            .split('/')
            .skip(1)
            .collect();
        let short_token = if matches!(host, "vm.tiktok.com" | "vt.tiktok.com") && parts.len() == 1 {
            Some(parts[0])
        } else if !matches!(host, "vm.tiktok.com" | "vt.tiktok.com")
            && parts.len() == 2
            && parts[0] == "t"
        {
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
        if matches!(host, "vm.tiktok.com" | "vt.tiktok.com") {
            return Err("Paste a TikTok video or share link.".into());
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
        return Err(if parts.contains(&"photo") {
            "TikTok photo slideshows are not supported. Choose an individual video post."
        } else if parts.contains(&"live") {
            "TikTok live streams are not supported. Choose a recorded video post."
        } else {
            "Paste an individual TikTok video or share link. Profiles and collections are not supported."
        }.into());
    }
    let id = if host == "youtu.be" || host == "www.youtu.be" {
        url.path_segments()
            .and_then(|mut s| s.next())
            .map(str::to_owned)
    } else if matches!(
        host,
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com"
    ) {
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

pub fn is_tiktok_short(link: &str) -> bool {
    url::Url::parse(link).is_ok_and(|u| {
        matches!(u.host_str(), Some("vm.tiktok.com" | "vt.tiktok.com"))
            || (u.host_str() == Some("www.tiktok.com") && u.path().starts_with("/t/"))
    })
}

pub fn tools_dir() -> PathBuf {
    crate::platform::tools_dir()
}
pub fn tools_ready() -> bool {
    ["yt-dlp", "ffmpeg", "ffprobe", "deno"].iter().all(|name| {
        tools_dir()
            .join(crate::platform::executable(name))
            .is_file()
    })
}
pub fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Give each helper its own process group so cancellation also stops FFmpeg.
        cmd.process_group(0);
    }
    cmd
}
fn command(
    session: Option<&SessionCookies>,
) -> Result<(Command, Option<tempfile::NamedTempFile>), String> {
    let tools = tools_dir();
    let mut cmd = Command::new(tools.join(crate::platform::executable("yt-dlp")));
    hidden(&mut cmd);
    cmd.args([
        "--ignore-config",
        "--no-playlist",
        "--no-colors",
        "--encoding",
        "utf-8",
        "--socket-timeout",
        "20",
        "--retries",
        "3",
        "--ffmpeg-location",
    ])
    .arg(&tools)
    .arg("--js-runtimes")
    .arg(format!(
        "deno:{}",
        tools.join(crate::platform::executable("deno")).display()
    ));
    let session_file = session.map(SessionCookies::temporary_file).transpose()?;
    if let Some(file) = &session_file {
        cmd.arg("--cookies").arg(file.path());
    }
    Ok((cmd, session_file))
}

fn command_for(
    link: &str,
    session: Option<&SessionCookies>,
) -> Result<(Command, Option<tempfile::NamedTempFile>), String> {
    let site = source(link);
    let (mut cmd, file) = command(session.filter(|_| site == Some(Source::YouTube)))?;
    if site == Some(Source::TikTok) {
        cmd.args(["--use-extractors", "TikTok"]);
    }
    Ok((cmd, file))
}

pub enum Event {
    Loaded(Video),
    Line(String),
    Complete(CompletedDownload),
    Failed(String),
    Cancelled,
}

pub(crate) fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // SAFETY: hidden() assigns the spawned helper a process group equal to
        // its PID. This targets that group, never our application's group.
        unsafe {
            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ =
            hidden(Command::new("taskkill").args(["/PID", &child.id().to_string(), "/T", "/F"]))
                .output();
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn inspect(
    link: String,
    session: Option<SessionCookies>,
    tx: Sender<Event>,
    cancel: Arc<AtomicBool>,
) {
    let result = (|| {
        let link = if is_tiktok_short(&link) {
            match crate::tiktok::resolve(&link, &cancel)? {
                Some(resolved) => resolved,
                None => return Ok(None),
            }
        } else {
            link
        };
        let (mut cmd, _session_file) = command_for(&link, session.as_ref())?;
        cmd.args(["--dump-single-json", "--skip-download", "--", &link]);
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start yt-dlp: {e}"))?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let out = thread::spawn(move || {
            let mut text = String::new();
            let result = BufReader::new(stdout).read_to_string(&mut text);
            result.map(|_| text)
        });
        let err = thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut text);
            text
        });
        let start = Instant::now();
        let status = loop {
            if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(150) {
                stop(&mut child);
                if cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                return Err(
                    "The video service took too long to respond. Check your connection and try again.".into(),
                );
            }
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            thread::sleep(Duration::from_millis(100));
        };
        let output = out
            .join()
            .map_err(|_| "Could not read video data.")?
            .map_err(|e| e.to_string())?;
        let errors = err.join().unwrap_or_default();
        if !status.success() {
            return Err(error_for(&errors, &link));
        }
        parse_video_for(&output, &link).map(Some)
    })();
    let _ = tx.send(match result {
        Ok(Some(v)) => Event::Loaded(v),
        Ok(None) => Event::Cancelled,
        Err(e) => Event::Failed(e),
    });
}

fn friendly_error(raw: &str) -> String {
    let tail = raw
        .lines()
        .rev()
        .find(|l| l.contains("ERROR:"))
        .unwrap_or(raw.trim());
    let tail: String = tail.chars().take(1200).collect();
    if tail.is_empty() {
        "The download engine stopped unexpectedly. Try again or update the tools.".into()
    } else if tail.contains("Sign in")
        || tail.contains("confirm you’re not a bot")
        || tail.contains("confirm you're not a bot")
    {
        "YouTube requires sign-in or a verification check. Open YouTube session to import from Firefox automatically, or choose a fresh cookies.txt export, then retry. YouTube may still restrict this connection or video.".into()
    } else {
        tail.trim_start_matches("ERROR: ").to_owned()
    }
}

fn error_for(raw: &str, link: &str) -> String {
    if source(link) != Some(Source::TikTok) {
        return friendly_error(raw);
    }
    let lower = raw.to_lowercase();
    if lower.contains("log in")
        || lower.contains("login")
        || lower.contains("sign in")
        || lower.contains("private")
    {
        "TikTok requires access or verification for this video. Only public videos are supported; YouTube browser sessions cannot unlock TikTok videos.".into()
    } else if lower.contains("resolve")
        || lower.contains("timed out")
        || lower.contains("connection")
    {
        "Could not connect to TikTok. Check your connection and try again.".into()
    } else if lower.contains("429") || lower.contains("rate limit") {
        "TikTok is limiting requests. Wait a while before trying again.".into()
    } else if lower.contains("100002")
        || lower.contains("not available")
        || lower.contains("unavailable")
        || lower.contains("removed")
        || lower.contains("404")
    {
        "This TikTok video is unavailable or has been removed. Check that it still plays in your browser.".into()
    } else if lower.contains("blocked") || lower.contains("403") {
        "TikTok restricted this request. Try again later or check the video in your browser.".into()
    } else {
        "TikTok could not provide this video. It may be unavailable or unsupported; check the link in your browser or update the download tools.".into()
    }
}

pub fn download(
    media: MediaIdentity,
    format: Format,
    folder: PathBuf,
    session: Option<SessionCookies>,
    tx: Sender<Event>,
    cancel: Arc<AtomicBool>,
) {
    let link = media.source_url.as_str();
    let result = (|| {
        let (mut cmd, _session_file) = command_for(link, session.as_ref())?;
        let template = if source(link) == Some(Source::TikTok) {
            "TikTok %(title).140s [%(id)s] [%(format_id)s].%(ext)s"
        } else {
            "%(title).150s [%(id)s] [%(format_id)s].%(ext)s"
        };
        cmd.args(["--newline", "--progress", "--no-simulate", "--no-overwrites", "--windows-filenames", "--trim-filenames", "180", "--progress-template", "download:__PROGRESS__%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s", "--print", "after_move:__FILE__%(filepath)s", "--format"])
            .arg(format.selector()).arg("--paths").arg(&folder)
            .args(["--output", template, "--", link]);
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start download: {e}"))?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (line_tx, line_rx) = std::sync::mpsc::channel();
        let reader = |stream: Box<dyn Read + Send>, line_tx: std::sync::mpsc::Sender<String>| {
            thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    if line_tx.send(line).is_err() {
                        break;
                    }
                }
            })
        };
        let out = reader(Box::new(stdout), line_tx.clone());
        let err = reader(Box::new(stderr), line_tx);
        let mut tail = VecDeque::new();
        let mut final_path = None;
        let mut process_line = |line: String| {
            if let Some(path) = line.strip_prefix("__FILE__") {
                final_path = Some(PathBuf::from(path));
            } else {
                if tail.len() >= 20 {
                    tail.pop_front();
                }
                tail.push_back(line.clone());
                // TikTok diagnostics can contain signed media URLs. Only send
                // structured progress to the UI; errors are classified below.
                if source(link) != Some(Source::TikTok) || line.starts_with("__PROGRESS__") {
                    let _ = tx.send(Event::Line(line));
                }
            }
        };
        let status = loop {
            if cancel.load(Ordering::Relaxed) {
                stop(&mut child);
                let _ = out.join();
                let _ = err.join();
                return Ok(None);
            }
            while let Ok(line) = line_rx.try_recv() {
                process_line(line);
            }
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            thread::sleep(Duration::from_millis(80));
        };
        let _ = out.join();
        let _ = err.join();
        for line in line_rx {
            process_line(line);
        }
        if !status.success() {
            return Err(error_for(
                &tail.into_iter().collect::<Vec<_>>().join("\n"),
                link,
            ));
        }
        let path = final_path.filter(|p| p.is_file()).ok_or(
            "The engine finished but did not return a saved file. Check the destination folder.",
        )?;
        Ok(Some(path))
    })();
    let _ = tx.send(match result {
        Ok(Some(path)) => Event::Complete(CompletedDownload { path, media }),
        Ok(None) => Event::Cancelled,
        Err(e) => Event::Failed(e),
    });
}

pub fn progress(line: &str) -> Option<(f32, String)> {
    let data = line.strip_prefix("__PROGRESS__")?;
    let parts: Vec<_> = data.split('|').collect();
    let percent = parts
        .first()?
        .trim()
        .trim_end_matches('%')
        .parse::<f32>()
        .ok()?;
    Some((
        (percent / 100.).clamp(0., 1.),
        format!(
            "{} · {} · {} remaining",
            parts[0].trim(),
            parts.get(1).unwrap_or(&"—").trim(),
            parts.get(2).unwrap_or(&"—").trim()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiktok_share_and_mobile_routes_keep_session_isolation() {
        for link in [
            "https://vm.tiktok.com/ABC123/",
            "https://vt.tiktok.com/ABC123/",
            "https://www.tiktok.com/t/ABC123?lang=en",
            "https://m.tiktok.com/@creator/video/123",
            "https://m.tiktok.com/v/123.html",
        ] {
            let normalized = normalize_url(link).unwrap();
            assert_eq!(source(&normalized), Some(Source::TikTok));
            let (cmd, session) =
                command_for(&normalized, Some(&crate::session::fake_session())).unwrap();
            assert!(session.is_none());
            assert!(!cmd.get_args().any(|a| a == "--cookies"));
        }
        assert_eq!(
            normalize_url("https://m.tiktok.com/v/123.html?lang=en").unwrap(),
            "https://www.tiktok.com/share/video/123?lang=en"
        );
        for (url, explanation) in [
            ("https://www.tiktok.com/@creator/photo/123", "photo"),
            ("https://www.tiktok.com/@creator/live", "live"),
            ("https://www.tiktok.com/@creator", "Profiles"),
        ] {
            assert!(normalize_url(url).unwrap_err().contains(explanation));
        }
        let link = "https://www.tiktok.com/@creator/video/123";
        for (raw, expected) in [
            ("HTTP 429 token=secret", "limiting"),
            ("status 100002 token=secret", "unavailable"),
            ("Private video token=secret", "access"),
            ("HTTP 403 token=secret", "restricted"),
            ("Failed to resolve token=secret", "connect"),
        ] {
            let message = error_for(raw, link);
            assert!(message.contains(expected), "{message}");
            assert!(!message.contains("secret"));
        }
    }

    #[test]
    fn missing_metadata_has_honest_labels_for_both_platforms() {
        for (link, extractor) in [
            ("https://www.youtube.com/watch?v=abcdefghijk", "Youtube"),
            ("https://www.tiktok.com/@creator/video/123", "TikTok"),
        ] {
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
                let video = parse_video_for(&fixture.to_string(), link).unwrap();
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
        for (link, extractor, expected_source) in [
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
            let video = parse_video_for(&json.to_string(), link).unwrap();
            assert_eq!(
                video.identity(),
                MediaIdentity {
                    source: expected_source,
                    source_url: link.into(),
                    id: Some("123".into()),
                    extractor_key: Some(extractor.into()),
                }
            );
        }
        let video = parse_video_for(
            r#"{"formats":[{"format_id":"muxed","vcodec":"h264","acodec":"aac"}]}"#,
            "https://www.youtube.com/watch?v=abcdefghijk",
        )
        .unwrap();
        assert_eq!(video.identity().id, None);
        assert_eq!(video.identity().extractor_key, None);
    }

    #[test]
    fn tiktok_routing_rejects_unverified_content_and_deceptive_hosts() {
        for link in [
            "https://www.tiktok.com/@creator/video/7622412096440929566",
            "tiktok.com/share/video/7622412096440929566/?lang=en",
        ] {
            assert_eq!(source(link), Some(Source::TikTok));
            assert!(
                normalize_url(link)
                    .unwrap()
                    .starts_with("https://www.tiktok.com/")
            );
        }
        for link in [
            "https://www.tiktok.com.evil.com/@c/video/123",
            "https://www.tiktok.com@evil.com/@c/video/123",
            "https://www.tiktok.com:8080/@c/video/123",
            "https://vm.tiktok.com/abc/extra",
            "https://www.tiktok.com/t/abc/extra",
            "https://www.tiktok.com/@c/photo/123",
            "https://www.tiktok.com/@c/live",
            "https://www.tiktok.com/@c",
            "https://www.tiktok.com/@c/video/123/extra",
            "https://www.tiktok.com/@c/video/nope",
        ] {
            assert!(normalize_url(link).is_err(), "{link}");
        }
    }

    #[test]
    fn tiktok_session_and_metadata_are_isolated() {
        let link = "https://www.tiktok.com/@creator/video/123";
        let (cmd, cookie_file) = command_for(link, Some(&crate::session::fake_session())).unwrap();
        assert!(cookie_file.is_none());
        assert!(!cmd.get_args().any(|a| a == "--cookies"));
        let json = r#"{"id":"123","extractor_key":"TikTok","title":null,"formats":[{"format_id":"ok","width":540,"height":958,"vcodec":"h264","acodec":"aac"},{"format_id":"unknown","vcodec":"h264","acodec":"aac"},{"format_id":"bad","vcodec":"bytevc2","acodec":"aac"},{"format_id":"silent","vcodec":"h264","acodec":"none"}]}"#;
        let video = parse_video_for(json, link).unwrap();
        assert_eq!(video.source, Source::TikTok);
        assert_eq!(video.source_url, link);
        assert_eq!(video.title, "Video 123");
        assert_eq!(video.formats.len(), 2);
        assert_eq!(video.formats[0].quality(), "540 × 958");
        assert!(!video.formats[1].quality().contains("0p"));
        assert!(parse_video_for(&json.replace("TikTok", "Generic"), link).is_err());
        assert!(parse_video_for(r#"{"_type":"playlist","entries":[]}"#, link).is_err());
        assert!(parse_video_for(r#"{"extractor_key":"TikTok","formats":[{"format_id":"audio","vcodec":"none","acodec":"aac"}]}"#, link).is_err());
        let error = error_for("ERROR: Sign in https://cdn.example/?secret=token", link);
        assert!(!error.starts_with("YouTube"));
        assert!(!error.contains("secret"));
    }

    #[test]
    #[ignore = "requires explicitly supplied permitted TikTok URL, internet and tools"]
    fn tiktok_authorized_production_pipeline() {
        let link = normalize_url(
            &std::env::var("STREAM_DLP_TIKTOK_TEST_URL")
                .expect("Supply an owned/permitted TikTok video URL"),
        )
        .unwrap();
        assert_eq!(source(&link), Some(Source::TikTok));
        let (tx, rx) = std::sync::mpsc::channel();
        inspect(
            link.clone(),
            Some(crate::session::fake_session()),
            tx,
            Arc::new(AtomicBool::new(false)),
        );
        let video = match rx.recv().unwrap() {
            Event::Loaded(v) => v,
            Event::Failed(e) => panic!("{e}"),
            _ => panic!("No metadata"),
        };
        let format = video
            .formats
            .iter()
            .find(|f| f.vcodec.as_deref() == Some("h264"))
            .unwrap()
            .clone();
        let folder =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-output/tiktok-production");
        std::fs::create_dir_all(&folder).unwrap();
        let run_download = || {
            let (tx, rx) = std::sync::mpsc::channel();
            download(
                video.identity(),
                format.clone(),
                folder.clone(),
                Some(crate::session::fake_session()),
                tx,
                Arc::new(AtomicBool::new(false)),
            );
            rx.into_iter()
                .find_map(|e| match e {
                    Event::Complete(result) => {
                        assert_eq!(result.media, video.identity());
                        Some(result.path)
                    }
                    Event::Failed(e) => panic!("{e}"),
                    _ => None,
                })
                .expect("No output file")
        };
        let path = run_download();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("TikTok ")
        );
        let before = std::fs::read(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(run_download(), path);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        let decoded = hidden(
            Command::new(tools_dir().join(crate::platform::executable("ffmpeg")))
                .args(["-v", "error", "-xerror", "-i"])
                .arg(&path)
                .args(["-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-"]),
        )
        .output()
        .unwrap();
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = thread::spawn(move || {
            download(video.identity(), format, folder, None, tx, worker_cancel)
        });
        thread::sleep(Duration::from_millis(100));
        cancel.store(true, Ordering::Relaxed);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("Cancellation timed out")
            {
                Event::Cancelled => break,
                Event::Line(_) => {}
                _ => panic!("Expected cancellation during helper startup"),
            }
        }
        worker.join().unwrap();
        println!(
            "Production TikTok inspect/download/decode/no-overwrite/cancellation passed: {}",
            path.display()
        );
    }
    #[cfg(unix)]
    #[test]
    fn cancellation_stops_descendants_in_the_helper_process_group() {
        let directory = tempfile::tempdir().unwrap();
        let survived = directory.path().join("survived");
        let ready = directory.path().join("ready");
        let mut child = hidden(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "(sleep 1; printf survived > \"$1\") & printf ready > \"$2\"; wait",
                    "test",
                ])
                .arg(&survived)
                .arg(&ready),
        )
        .spawn()
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let started = ready.exists();
        stop(&mut child);
        assert!(started, "The synthetic helper did not start");
        thread::sleep(Duration::from_millis(1200));
        assert!(!survived.exists(), "A descendant survived cancellation");
    }

    #[test]
    #[ignore = "requires installed FFmpeg and yt-dlp, plus localhost sockets"]
    fn local_download_pipeline_saves_playable_file() {
        use std::{io::Write, net::TcpListener};
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-output")
            .join(format!("local-{}", std::process::id()));
        std::fs::create_dir_all(&output).unwrap();
        let fixture = output.join("manifest.mpd");
        let generated = hidden(
            Command::new(tools_dir().join(crate::platform::executable("ffmpeg")))
                .current_dir(&output)
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=blue:s=160x90:r=10:d=1",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=1",
                    "-c:v",
                    "mpeg4",
                    "-pix_fmt",
                    "yuv420p",
                    "-c:a",
                    "aac",
                    "-shortest",
                    "-f",
                    "dash",
                ])
                .arg(&fixture),
        )
        .output()
        .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let server_done = done.clone();
        let slow = Arc::new(AtomicBool::new(false));
        let server_slow = slow.clone();
        let server_root = output.clone();
        let server = thread::spawn(move || {
            while !server_done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Windows accepted sockets inherit the listener's nonblocking mode.
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut request = Vec::new();
                        while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 16384
                        {
                            let mut chunk = [0; 4096];
                            let n = stream.read(&mut chunk).unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            request.extend_from_slice(&chunk[..n]);
                        }
                        if request.is_empty() {
                            continue;
                        }
                        let head = request.starts_with(b"HEAD ");
                        let request_text = String::from_utf8_lossy(&request);
                        assert!(
                            !request_text.contains("fake-test-value"),
                            "YouTube cookies must not be sent to localhost"
                        );
                        let name = request_text
                            .split_whitespace()
                            .nth(1)
                            .unwrap_or("/")
                            .trim_start_matches('/');
                        assert!(
                            !name.contains("..") && !name.contains('/') && !name.contains('\\')
                        );
                        let bytes = std::fs::read(server_root.join(name))
                            .unwrap_or_else(|e| panic!("Fixture request {name} failed: {e}"));
                        let mime = if name.ends_with(".mpd") {
                            "application/dash+xml"
                        } else {
                            "video/mp4"
                        };
                        let headers = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            bytes.len()
                        );
                        let _ = stream.write_all(headers.as_bytes());
                        if !head {
                            if server_slow.load(Ordering::Relaxed) && !name.ends_with(".mpd") {
                                for chunk in bytes.chunks(256) {
                                    if server_done.load(Ordering::Relaxed)
                                        || stream.write_all(chunk).is_err()
                                    {
                                        break;
                                    }
                                    thread::sleep(Duration::from_millis(20));
                                }
                            } else {
                                let _ = stream.write_all(&bytes);
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let link = format!("http://{address}/manifest.mpd");
        inspect(
            link.clone(),
            Some(crate::session::fake_session()),
            tx.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        let video = match rx.recv().unwrap() {
            Event::Loaded(video) => video,
            Event::Failed(error) => {
                done.store(true, Ordering::Relaxed);
                server.join().unwrap();
                panic!("Local inspection failed: {error}");
            }
            _ => panic!("Unexpected inspection event"),
        };
        let format = video
            .formats
            .iter()
            .find(|f| f.video() && !f.audio())
            .expect("DASH video-only format was not detected")
            .clone();
        println!(
            "Detected {} streams; selecting {}",
            video.formats.len(),
            format.selector()
        );
        download(
            video.identity(),
            format,
            output,
            Some(crate::session::fake_session()),
            tx,
            Arc::new(AtomicBool::new(false)),
        );
        let mut saved = None;
        let mut had_progress = false;
        for event in rx {
            match event {
                Event::Complete(result) => {
                    assert_eq!(result.media, video.identity());
                    saved = Some(result.path);
                }
                Event::Line(line) => {
                    had_progress |= progress(&line).is_some();
                    println!("{line}");
                }
                Event::Failed(error) => panic!("Local download failed: {error}"),
                _ => panic!("Unexpected event"),
            }
        }
        let saved = saved.expect("No completion event");
        assert!(had_progress, "No progress events were parsed");
        let probe = hidden(
            Command::new(tools_dir().join(crate::platform::executable("ffprobe")))
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_type",
                    "-of",
                    "json",
                ])
                .arg(&saved),
        )
        .output()
        .unwrap();
        assert!(probe.status.success());
        let json: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let streams = json["streams"].as_array().unwrap();
        assert!(streams.iter().any(|s| s["codec_type"] == "video"));
        assert!(streams.iter().any(|s| s["codec_type"] == "audio"));
        println!(
            "Verified saved file with video + audio: {}",
            saved.display()
        );
        let (cancel_tx, cancel_rx) = std::sync::mpsc::channel();
        download(
            video.identity(),
            video.formats[0].clone(),
            saved.parent().unwrap().to_owned(),
            Some(crate::session::fake_session()),
            cancel_tx,
            Arc::new(AtomicBool::new(true)),
        );
        assert!(matches!(cancel_rx.recv().unwrap(), Event::Cancelled));
        println!("Verified cancellation stops the child process.");
        slow.store(true, Ordering::Relaxed);
        let transfer_folder = saved.parent().unwrap().join("cancel-active");
        std::fs::create_dir_all(&transfer_folder).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = thread::spawn(move || {
            download(
                video.identity(),
                video.formats[0].clone(),
                transfer_folder,
                None,
                tx,
                worker_cancel,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut transferred = false;
        let mut cancelled = false;
        while let Ok(event) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            match event {
                Event::Line(line) if progress(&line).is_some() => {
                    transferred = true;
                    cancel.store(true, Ordering::Relaxed);
                }
                Event::Cancelled => {
                    cancelled = true;
                    break;
                }
                Event::Failed(_) | Event::Complete(_) => break,
                _ => {}
            }
        }
        cancel.store(true, Ordering::Relaxed);
        done.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        server.join().unwrap();
        assert!(
            transferred && cancelled,
            "Expected cancellation after transfer progress"
        );
        println!("Verified cancellation during an active throttled transfer.");
    }

    #[test]
    #[ignore = "requires internet access and installed download tools"]
    fn youtube_download_has_video_and_audio() {
        let link = std::env::var("STREAM_DLP_TEST_URL")
            .unwrap_or_else(|_| "https://www.youtube.com/watch?v=jNQXAC9IVRw".to_string());
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        inspect(link.clone(), None, tx.clone(), cancel.clone());
        let video = match rx.recv().unwrap() {
            Event::Loaded(video) => video,
            Event::Failed(error) => panic!("YouTube inspection failed: {error}"),
            _ => panic!("Unexpected inspection event"),
        };
        println!(
            "Detected {} with {} formats",
            video.title,
            video.formats.len()
        );
        let format = video
            .formats
            .iter()
            .find(|f| f.video() && !f.audio() && f.height.unwrap_or(9999) <= 360)
            .expect("No low-resolution video-only test stream")
            .clone();
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-output");
        std::fs::create_dir_all(&output).unwrap();
        download(video.identity(), format, output, None, tx, cancel);
        let path = loop {
            match rx.recv().unwrap() {
                Event::Complete(result) => {
                    assert_eq!(result.media, video.identity());
                    break result.path;
                }
                Event::Failed(error) => panic!("Download failed: {error}"),
                Event::Line(line) => println!("{line}"),
                _ => panic!("Unexpected download event"),
            }
        };
        let probe = hidden(
            Command::new(tools_dir().join(crate::platform::executable("ffprobe")))
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_type",
                    "-of",
                    "json",
                ])
                .arg(&path),
        )
        .output()
        .unwrap();
        assert!(probe.status.success());
        let json: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let streams = json["streams"].as_array().unwrap();
        assert!(streams.iter().any(|s| s["codec_type"] == "video"));
        assert!(streams.iter().any(|s| s["codec_type"] == "audio"));
        println!("Verified video + audio in {}", path.display());
    }
    #[test]
    fn explains_youtube_verification_errors() {
        let error = friendly_error(
            "WARNING: No title found\nERROR: [youtube] id: Sign in to confirm you’re not a bot. Use --cookies-from-browser...",
        );
        assert!(error.contains("verification check"));
        assert!(!error.contains("--cookies-from-browser"));
        assert!(friendly_error("ERROR: Video unavailable").contains("Video unavailable"));
    }
    #[test]
    fn cookie_file_is_passed_to_the_engine_and_removed_after_use() {
        let (command, file) = command(Some(&crate::session::fake_session())).unwrap();
        let path = file.as_ref().unwrap().path().to_owned();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.windows(2)
                .any(|args| args[0] == "--cookies" && args[1] == path.to_string_lossy())
        );
        assert!(!args.iter().any(|arg| arg.contains("fake-test-value")));
        assert!(path.exists());
        drop(file);
        assert!(!path.exists());
        let (anonymous, file) = super::command(None).unwrap();
        assert!(file.is_none());
        assert!(!anonymous.get_args().any(|arg| arg == "--cookies"));
    }
    #[test]
    fn canonicalizes_video_links_without_playlist_or_tracking() {
        for link in [
            "https://youtu.be/BaW_jenozKc?t=5",
            "www.youtube.com/watch?v=BaW_jenozKc&list=123",
            "https://youtube.com/shorts/BaW_jenozKc",
            "https://www.youtube.com/shorts/BaW_jenozKc?feature=share",
            "https://m.youtube.com/shorts/BaW_jenozKc",
        ] {
            assert_eq!(
                normalize_url(link).unwrap(),
                "https://www.youtube.com/watch?v=BaW_jenozKc"
            );
        }
    }
    #[test]
    fn rejects_other_hosts_and_invalid_ids() {
        for link in [
            "https://youtube.com.evil.com/watch?v=BaW_jenozKc",
            "https://youtube.com@evil.com/watch?v=BaW_jenozKc",
            "file:///tmp/a",
            "--exec calc",
            "https://youtube.com/playlist?list=abc",
            "https://youtu.be/a",
            "https://youtube.com:8080/watch?v=BaW_jenozKc",
        ] {
            assert!(normalize_url(link).is_err(), "{link}");
        }
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
    #[test]
    fn parses_progress_and_ignores_noise() {
        assert_eq!(
            progress("__PROGRESS__ 42.5%|2 MiB/s|00:03").unwrap().0,
            0.425
        );
        assert!(progress("[Merger] merging formats").is_none());
        assert!(progress("__PROGRESS__NA|NA|NA").is_none());
    }
}
