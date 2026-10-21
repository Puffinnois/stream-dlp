use crate::session::SessionCookies;
use serde::Deserialize;
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Format {
    pub format_id: String,
    #[serde(default)]
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
            format!(
                "{}p{}",
                self.height.unwrap_or(0),
                if self.fps.unwrap_or(0.) > 30. {
                    format!(" · {} fps", self.fps.unwrap_or(0.).round())
                } else {
                    String::new()
                }
            )
        } else {
            format!("Audio · {} kbps", self.abr.unwrap_or(0.).round())
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
        if let Some(note) = &self.format_note
            && !note.is_empty()
        {
            parts.push(note.clone());
        }
        if let Some(lang) = &self.language {
            parts.push(lang.clone());
        }
        parts.push(format!("ID {}", self.format_id));
        if let (Some(w), Some(h)) = (self.width, self.height) {
            parts.push(format!("{w} × {h}"));
        }
        parts.join(" · ")
    }
    pub fn size(&self) -> String {
        self.filesize
            .or(self.filesize_approx)
            .map(|n| {
                format!(
                    "{}{} MB",
                    if self.filesize.is_none() { "~" } else { "" },
                    (n / 1_048_576.).round()
                )
            })
            .unwrap_or_else(|| "Size varies".into())
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
    pub title: String,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    #[serde(default)]
    pub formats: Vec<Format>,
    #[serde(default)]
    pub is_live: bool,
}

pub fn parse_video(json: &str) -> Result<Video, String> {
    let mut video: Video =
        serde_json::from_str(json).map_err(|e| format!("Could not read the video details: {e}"))?;
    if video.is_live {
        return Err("This is a live stream. Try again after the broadcast has ended.".into());
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

pub fn normalize_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    let full = if input.starts_with("https://") || input.starts_with("http://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = url::Url::parse(&full).map_err(|_| "Paste a valid YouTube video link.".to_owned())?;
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err("Paste a standard YouTube video link.".into());
    }
    let host = url.host_str().unwrap_or("");
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
        .ok_or("Paste a YouTube video link, such as youtube.com/watch?v=… or youtu.be/… .")?;
    Ok(format!("https://www.youtube.com/watch?v={id}"))
}

pub fn root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}
pub fn tools_dir() -> PathBuf {
    let beside = root().join("tools");
    if beside.join("yt-dlp.exe").exists() {
        beside
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools")
    }
}
pub fn tools_ready() -> bool {
    ["yt-dlp.exe", "ffmpeg.exe", "ffprobe.exe", "deno.exe"]
        .iter()
        .all(|name| tools_dir().join(name).is_file())
}
pub fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}
fn command(
    session: Option<&SessionCookies>,
) -> Result<(Command, Option<tempfile::NamedTempFile>), String> {
    let tools = tools_dir();
    let mut cmd = Command::new(tools.join("yt-dlp.exe"));
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
    .arg(format!("deno:{}", tools.join("deno.exe").display()));
    let session_file = session.map(SessionCookies::temporary_file).transpose()?;
    if let Some(file) = &session_file {
        cmd.arg("--cookies").arg(file.path());
    }
    Ok((cmd, session_file))
}

pub enum Event {
    Loaded(Video),
    Line(String),
    Complete(PathBuf),
    Failed(String),
    Cancelled,
}

fn stop(child: &mut std::process::Child) {
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
        let (mut cmd, _session_file) = command(session.as_ref())?;
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
                    "YouTube took too long to respond. Check your connection and try again.".into(),
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
            return Err(friendly_error(&errors));
        }
        parse_video(&output).map(Some)
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

pub fn download(
    link: String,
    format: Format,
    folder: PathBuf,
    session: Option<SessionCookies>,
    tx: Sender<Event>,
    cancel: Arc<AtomicBool>,
) {
    let result = (|| {
        let (mut cmd, _session_file) = command(session.as_ref())?;
        cmd.args(["--newline", "--progress", "--no-simulate", "--no-overwrites", "--windows-filenames", "--trim-filenames", "180", "--progress-template", "download:__PROGRESS__%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s", "--print", "after_move:__FILE__%(filepath)s", "--format"])
            .arg(format.selector()).arg("--paths").arg(&folder)
            .args(["--output", "%(title).150s [%(id)s] [%(format_id)s].%(ext)s", "--", &link]);
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
                let _ = tx.send(Event::Line(line));
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
            return Err(friendly_error(
                &tail.into_iter().collect::<Vec<_>>().join("\n"),
            ));
        }
        let path = final_path.filter(|p| p.is_file()).ok_or(
            "The engine finished but did not return a saved file. Check the destination folder.",
        )?;
        Ok(Some(path))
    })();
    let _ = tx.send(match result {
        Ok(Some(path)) => Event::Complete(path),
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
    #[ignore = "requires installed FFmpeg and yt-dlp, plus localhost sockets"]
    fn local_download_pipeline_saves_playable_file() {
        use std::{io::Write, net::TcpListener};
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-output")
            .join(format!("local-{}", std::process::id()));
        std::fs::create_dir_all(&output).unwrap();
        let fixture = output.join("manifest.mpd");
        let generated = hidden(
            Command::new(tools_dir().join("ffmpeg.exe"))
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
                    "libx264",
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
                            let _ = stream.write_all(&bytes);
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
            link,
            format,
            output,
            Some(crate::session::fake_session()),
            tx,
            Arc::new(AtomicBool::new(false)),
        );
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        let mut saved = None;
        let mut had_progress = false;
        for event in rx {
            match event {
                Event::Complete(path) => saved = Some(path),
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
            Command::new(tools_dir().join("ffprobe.exe"))
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
            format!("http://{address}/manifest.mpd"),
            video.formats[0].clone(),
            saved.parent().unwrap().to_owned(),
            Some(crate::session::fake_session()),
            cancel_tx,
            Arc::new(AtomicBool::new(true)),
        );
        assert!(matches!(cancel_rx.recv().unwrap(), Event::Cancelled));
        println!("Verified cancellation stops the child process.");
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
        download(link, format, output, None, tx, cancel);
        let path = loop {
            match rx.recv().unwrap() {
                Event::Complete(path) => break path,
                Event::Failed(error) => panic!("Download failed: {error}"),
                Event::Line(line) => println!("{line}"),
                _ => panic!("Unexpected download event"),
            }
        };
        let probe = hidden(
            Command::new(tools_dir().join("ffprobe.exe"))
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
