//! Runs yt-dlp to inspect a link and to download one of its formats.
use super::{CompletedDownload, EngineError, Format, Link, MediaIdentity, Video, media::parse_for};
use crate::{process, session::SessionCookies};
use std::{
    collections::VecDeque,
    path::PathBuf,
    process::Command,
    sync::{Arc, atomic::AtomicBool, mpsc::Sender},
    time::Duration,
};

pub enum Event {
    Loaded(Video),
    Line(String),
    Complete(CompletedDownload),
    Failed(EngineError),
    Cancelled,
}

fn command(
    session: Option<&SessionCookies>,
) -> Result<(Command, Option<tempfile::NamedTempFile>), EngineError> {
    let mut cmd = Command::new(crate::tools::path("yt-dlp"));
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
    .arg(crate::tools::dir())
    .arg("--js-runtimes")
    .arg(format!("deno:{}", crate::tools::path("deno").display()));
    let session_file = session.map(SessionCookies::temporary_file).transpose()?;
    if let Some(file) = &session_file {
        cmd.arg("--cookies").arg(file.path());
    }
    Ok((cmd, session_file))
}

fn command_for(
    link: &Link,
    session: Option<&SessionCookies>,
) -> Result<(Command, Option<tempfile::NamedTempFile>), EngineError> {
    let (mut cmd, file) = command(session.filter(|_| link.takes_session()))?;
    cmd.args(link.source.arguments());
    Ok((cmd, file))
}

pub fn inspect(
    link: Link,
    session: Option<SessionCookies>,
    tx: Sender<Event>,
    cancel: Arc<AtomicBool>,
) {
    let result = (|| {
        let Some(link) = link.resolve(&cancel)? else {
            return Ok(None);
        };
        let (mut cmd, _session_file) = command_for(&link, session.as_ref())?;
        cmd.args(["--dump-single-json", "--skip-download", "--", &link.url]);
        let output = match process::Helper::new(&mut cmd, &cancel)
            .time_limit(Duration::from_secs(150))
            .output()
        {
            Ok(output) => output,
            Err(process::Error::Cancelled) => return Ok(None),
            Err(process::Error::TimedOut) => return Err(EngineError::TimedOut),
            Err(process::Error::Start(e)) => {
                return Err(EngineError::Helper(format!("Could not start yt-dlp: {e}")));
            }
            Err(error) => return Err(EngineError::Helper(error.to_string())),
        };
        if !output.status.success() {
            return Err(link
                .source
                .explain(&String::from_utf8_lossy(&output.stderr)));
        }
        parse_for(&String::from_utf8_lossy(&output.stdout), &link).map(Some)
    })();
    let _ = tx.send(match result {
        Ok(Some(v)) => Event::Loaded(v),
        Ok(None) => Event::Cancelled,
        Err(e) => Event::Failed(e),
    });
}

pub fn download(
    media: MediaIdentity,
    format: Format,
    folder: PathBuf,
    session: Option<SessionCookies>,
    tx: Sender<Event>,
    cancel: Arc<AtomicBool>,
) {
    let link = media.link();
    let result = (|| {
        let (mut cmd, _session_file) = command_for(&link, session.as_ref())?;
        cmd.args(["--newline", "--progress", "--no-simulate", "--no-overwrites", "--windows-filenames", "--trim-filenames", "180", "--progress-template", "download:__PROGRESS__%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s", "--print", "after_move:__FILE__%(filepath)s", "--format"])
            .arg(format.selector()).arg("--paths").arg(&folder)
            .args(["--output", link.source.output_template(), "--", &link.url]);
        // Errors are explained below; only lines the site allows reach the UI.
        let shows_output = link.source.shows_helper_output();
        let mut tail = VecDeque::new();
        let mut final_path = None;
        let status = match process::Helper::new(&mut cmd, &cancel).lines(|_, line| {
            if let Some(path) = line.strip_prefix("__FILE__") {
                final_path = Some(PathBuf::from(path));
            } else {
                if tail.len() >= 20 {
                    tail.pop_front();
                }
                tail.push_back(line.to_owned());
                if shows_output || line.starts_with("__PROGRESS__") {
                    let _ = tx.send(Event::Line(line.to_owned()));
                }
            }
        }) {
            Ok(status) => status,
            Err(process::Error::Cancelled) => return Ok(None),
            Err(process::Error::Start(e)) => {
                return Err(EngineError::Helper(format!(
                    "Could not start download: {e}"
                )));
            }
            Err(error) => return Err(EngineError::Helper(error.to_string())),
        };
        if !status.success() {
            return Err(link
                .source
                .explain(&tail.into_iter().collect::<Vec<_>>().join("\n")));
        }
        let path = final_path.filter(|p| p.is_file()).ok_or_else(|| {
            EngineError::Helper(
                "The engine finished but did not return a saved file. Check the destination folder."
                    .into(),
            )
        })?;
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
    use crate::{engine::Source, process::isolate};
    use std::{
        io::Read,
        sync::atomic::Ordering,
        thread,
        time::{Duration, Instant},
    };

    fn arguments(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_session_goes_only_to_youtube_and_tiktok_uses_its_extractor() {
        for (link, youtube) in [
            ("https://www.youtube.com/watch?v=abcdefghijk", true),
            ("https://vm.tiktok.com/ABC123/", false),
            ("https://vt.tiktok.com/ABC123/", false),
            ("https://www.tiktok.com/t/ABC123?lang=en", false),
            ("https://m.tiktok.com/@creator/video/123", false),
            ("https://m.tiktok.com/v/123.html", false),
        ] {
            let link = crate::engine::parse_link(link).unwrap();
            let (cmd, session) = command_for(&link, Some(&crate::session::fake_session())).unwrap();
            let args = arguments(&cmd);
            assert_eq!(session.is_some(), youtube, "{}", link.url);
            assert_eq!(args.iter().any(|a| a == "--cookies"), youtube);
            assert_eq!(
                args.windows(2)
                    .any(|a| a[0] == "--use-extractors" && a[1] == "TikTok"),
                !youtube
            );
        }
    }

    #[test]
    fn cookie_file_is_passed_to_the_engine_and_removed_after_use() {
        let (command, file) = command(Some(&crate::session::fake_session())).unwrap();
        let path = file.as_ref().unwrap().path().to_owned();
        let args = arguments(&command);
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
    fn parses_progress_and_ignores_noise() {
        assert_eq!(
            progress("__PROGRESS__ 42.5%|2 MiB/s|00:03").unwrap().0,
            0.425
        );
        assert!(progress("[Merger] merging formats").is_none());
        assert!(progress("__PROGRESS__NA|NA|NA").is_none());
    }

    #[test]
    #[ignore = "requires explicitly supplied permitted TikTok URL, internet and tools"]
    fn tiktok_authorized_production_pipeline() {
        let link = crate::engine::parse_link(
            &std::env::var("STREAM_DLP_TIKTOK_TEST_URL")
                .expect("Supply an owned/permitted TikTok video URL"),
        )
        .unwrap();
        assert_eq!(link.source, Source::TikTok);
        let (tx, rx) = std::sync::mpsc::channel();
        inspect(
            link,
            Some(crate::session::fake_session()),
            tx,
            Arc::new(AtomicBool::new(false)),
        );
        let video = match rx.recv().unwrap() {
            Event::Loaded(v) => v,
            Event::Failed(e) => panic!("{}", e.user_message()),
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
                    Event::Failed(e) => panic!("{}", e.user_message()),
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
        let decoded = isolate(
            Command::new(crate::tools::path("ffmpeg"))
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

    #[test]
    #[ignore = "requires installed FFmpeg and yt-dlp, plus localhost sockets"]
    fn local_download_pipeline_saves_playable_file() {
        use std::{io::Write, net::TcpListener};
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-output")
            .join(format!("local-{}", std::process::id()));
        std::fs::create_dir_all(&output).unwrap();
        let fixture = output.join("manifest.mpd");
        let generated = isolate(
            Command::new(crate::tools::path("ffmpeg"))
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
        // A YouTube-labelled local link: the session must still not reach it.
        let link = Link {
            source: Source::YouTube,
            url: format!("http://{address}/manifest.mpd"),
        };
        inspect(
            link,
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
        let probe = isolate(
            Command::new(crate::tools::path("ffprobe"))
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
        inspect(
            crate::engine::parse_link(&link).unwrap(),
            None,
            tx.clone(),
            cancel.clone(),
        );
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
        let probe = isolate(
            Command::new(crate::tools::path("ffprobe"))
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
}
