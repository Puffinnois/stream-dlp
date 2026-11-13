//! Runs yt-dlp to inspect a link and to download one of its formats.
use super::{
    CompletedDownload, EngineError, Format, Link, MediaIdentity, Source, Video,
    media::{Sound, parse_for},
    sources::other,
};
use crate::{process, session::SessionCookies};
use std::{
    collections::VecDeque,
    path::PathBuf,
    process::Command,
    sync::{Arc, atomic::AtomicBool, mpsc::Sender},
    time::Duration,
};

/// The most metadata yt-dlp may print while inspecting a link (each stream).
const METADATA_LIMIT: u64 = 32 * 1024 * 1024;

pub enum Event {
    Loaded(Video),
    Entries(Vec<Video>),
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
    let (mut cmd, file) =
        command(session.filter(|jar| jar.source == link.source && link.takes_session()))?;
    cmd.args(link.source.arguments());
    Ok((cmd, file))
}

pub fn inspect(
    link: Link,
    session: Option<SessionCookies>,
    tx: Sender<Event>,
    cancel: Arc<AtomicBool>,
) {
    let result = link.resolve(&cancel).and_then(|link| match link {
        Some(link) => inspect_resolved(&link, session.as_ref(), &cancel),
        None => Ok(None),
    });
    let _ = tx.send(match result {
        Ok(Some(event)) => event,
        Ok(None) => Event::Cancelled,
        Err(e) => Event::Failed(e),
    });
}

/// The yt-dlp command that inspects `link`, with the session file it reads.
fn inspect_command(
    link: &Link,
    session: Option<&SessionCookies>,
) -> Result<(Command, Option<tempfile::NamedTempFile>), EngineError> {
    let (mut cmd, session_file) = command_for(link, session)?;
    if link.source == Source::Instagram {
        // Keep photo entries without formats from aborting inspection of a mixed carousel.
        cmd.arg("--ignore-no-formats-error");
    }
    if link.source == Source::Other {
        // A page with many videos: read only the first ones.
        cmd.arg("--playlist-items")
            .arg(format!("1:{}", other::MAX_ENTRIES));
    }
    cmd.args(["--dump-single-json", "--skip-download", "--", &link.url]);
    Ok((cmd, session_file))
}

/// Inspects a link whose share link or redirects are already resolved. `None` when cancelled.
fn inspect_resolved(
    link: &Link,
    session: Option<&SessionCookies>,
    cancel: &AtomicBool,
) -> Result<Option<Event>, EngineError> {
    let (mut cmd, _session_file) = inspect_command(link, session)?;
    let output = match process::Helper::new(&mut cmd, cancel)
        .time_limit(Duration::from_secs(150))
        .output_limit(METADATA_LIMIT)
        .output()
    {
        Ok(output) => output,
        Err(process::Error::Cancelled) => return Ok(None),
        Err(process::Error::TimedOut) => return Err(EngineError::TimedOut(link.source)),
        Err(process::Error::Start(e)) => {
            return Err(EngineError::Helper(format!("Could not start yt-dlp: {e}")));
        }
        Err(process::Error::TooMuchOutput) => {
            return Err(EngineError::Unsupported(
                "This page lists too much to read. Paste the link to a single video.".into(),
            ));
        }
        Err(error) => return Err(EngineError::Helper(error.to_string())),
    };
    if !output.status.success() {
        return Err(link
            .source
            .explain(&String::from_utf8_lossy(&output.stderr)));
    }
    inspection_event(&String::from_utf8_lossy(&output.stdout), link).map(Some)
}

fn inspection_event(json: &str, link: &Link) -> Result<Event, EngineError> {
    if link.source == Source::Instagram {
        let post: serde_json::Value = serde_json::from_str(json)
            .map_err(|_| EngineError::Helper("Could not read Instagram post metadata.".into()))?;
        if let Some(entries) = post["entries"].as_array() {
            if post["extractor_key"] != "Instagram" || post["_type"] != "playlist" {
                return Err(EngineError::Unsupported(
                    "Choose an individual Instagram post.".into(),
                ));
            }
            let mut videos = Vec::new();
            for (index, entry) in entries.iter().enumerate() {
                if entry.is_null() {
                    continue;
                }
                match parse_for(&entry.to_string(), link) {
                    Ok(mut video) => {
                        video.entry_index = Some(index + 1);
                        videos.push(video);
                    }
                    Err(EngineError::MissingVideo(_)) => continue,
                    Err(error) => return Err(error),
                }
            }
            if videos.is_empty() {
                return Err(EngineError::MissingVideo(link.source));
            }
            return Ok(Event::Entries(videos));
        }
    }
    if link.source == super::Source::X {
        let post: serde_json::Value = serde_json::from_str(json)
            .map_err(|_| EngineError::Helper("Could not read X post metadata.".into()))?;
        if let Some(entries) = post["entries"].as_array() {
            if post["extractor_key"] != "Twitter"
                || post["_type"] != "playlist"
                || entries.is_empty()
            {
                return Err(EngineError::Unsupported(
                    "No supported X video entries were returned.".into(),
                ));
            }
            let videos = entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let mut video = parse_for(&entry.to_string(), link)?;
                    video.entry_index = Some(index + 1);
                    Ok(video)
                })
                .collect::<Result<Vec<_>, EngineError>>()?;
            return Ok(Event::Entries(videos));
        }
    }
    if link.source == Source::Other {
        let page: serde_json::Value =
            serde_json::from_str(json).map_err(|_| EngineError::Helper(other::HELPER.into()))?;
        if let Some(entries) = page["entries"].as_array() {
            // Entries that are not a downloadable video are skipped; when none is left, the
            // first reason is shown.
            let mut videos = Vec::new();
            let mut first_error = None;
            for (index, entry) in entries.iter().enumerate().take(other::MAX_ENTRIES) {
                if entry.is_null() {
                    continue;
                }
                match parse_for(&entry.to_string(), link) {
                    Ok(mut video) => {
                        video.entry_index = Some(index + 1);
                        videos.push(video);
                    }
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            return match videos.len() {
                0 => Err(first_error.unwrap_or(EngineError::MissingVideo(Source::Other))),
                1 => Ok(Event::Loaded(videos.remove(0))),
                _ => Ok(Event::Entries(videos)),
            };
        }
    }
    parse_for(json, link).map(Event::Loaded)
}

pub fn download(
    media: MediaIdentity,
    format: Format,
    folder: PathBuf,
    session: Option<SessionCookies>,
    tx: Sender<Event>,
    cancel: Arc<AtomicBool>,
) {
    // DNS answers can change after inspection: check another site's destination again.
    if media.source == Source::Other
        && let Err(error) = other::recheck(&media.source_url)
    {
        let _ = tx.send(Event::Failed(error));
        return;
    }
    run_download(media, format, folder, session, tx, cancel);
}

fn run_download(
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
        if matches!(
            link.source,
            super::Source::X | super::Source::Facebook | super::Source::Instagram
        ) {
            let id = media
                .id
                .as_deref()
                .filter(|id| {
                    if link.source == super::Source::Instagram {
                        super::sources::instagram::valid_id(id)
                    } else {
                        !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
                    }
                })
                .ok_or_else(|| {
                    EngineError::Unsupported(format!(
                        "{} returned no usable video ID.",
                        link.source.label()
                    ))
                })?;
            cmd.arg("--playlist-items")
                .arg(media.entry_index.unwrap_or(1).to_string())
                .arg("--match-filter")
                .arg(format!("id = '{id}'"));
        }
        if link.source == Source::Other {
            // Never a whole collection: only the chosen entry, by position and, when its ID is
            // safe to quote, by ID.
            cmd.arg("--playlist-items")
                .arg(media.entry_index.unwrap_or(1).to_string());
            if media.entry_index.is_some()
                && let Some(id) = media.id.as_deref().filter(|id| other::filter_safe_id(id))
            {
                cmd.arg("--match-filter").arg(format!("id = '{id}'"));
            }
        }
        cmd.args(["--newline", "--progress", "--no-simulate", "--no-overwrites", "--windows-filenames", "--trim-filenames", "180", "--progress-template", "download:__PROGRESS__%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s", "--print", "after_move:__FILE__%(filepath)s", "--format"])
            .arg(format.selector()).arg("--paths").arg(&folder)
            .args(["--output", link.source.output_template(), "--", &link.url]);
        // Raw helper output can include signed URLs, local paths and account details.
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
                if progress(line).is_some() {
                    let _ = tx.send(Event::Line(line.to_owned()));
                } else if line.starts_with("[Merger]") || line.starts_with("[Fixup") {
                    let _ = tx.send(Event::Line("[Merger]".into()));
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
            return Err(explain_download(
                link.source,
                format.sound(),
                &tail.into_iter().collect::<Vec<_>>().join("\n"),
            ));
        }
        let path = final_path.filter(|p| p.is_file()).ok_or_else(|| {
            EngineError::Helper(
                "The engine finished but did not return a saved file. Check the destination folder."
                    .into(),
            )
        })?;
        if format.sound().expected() {
            check_sound(
                format.sound(),
                crate::media::local::has_audio(&path, &cancel),
            )?;
        }
        Ok(Some(path))
    })();
    let _ = tx.send(match result {
        Ok(Some(path)) => Event::Complete(CompletedDownload { path, media }),
        Ok(None) => Event::Cancelled,
        Err(e) => Event::Failed(e),
    });
}

/// Explains a failed download. Audio that could not be combined with a video-only format has
/// its own message; everything else is explained by the site's rules.
fn explain_download(source: Source, sound: Sound, errors: &str) -> EngineError {
    let lower = errors.to_lowercase();
    if sound == Sound::Merged
        && (lower.contains("postprocessing:") || lower.contains("merging of multiple formats"))
    {
        EngineError::MissingAudio
    } else {
        source.explain(errors)
    }
}

/// A saved file that should have sound but has no audio stream is a failed download, never a
/// silent success. When FFprobe cannot tell, the download stands.
fn check_sound(sound: Sound, has_audio: Option<bool>) -> Result<(), EngineError> {
    if sound.expected() && has_audio == Some(false) {
        Err(EngineError::MissingAudio)
    } else {
        Ok(())
    }
}

pub fn progress(line: &str) -> Option<(f32, String)> {
    let data = line.strip_prefix("__PROGRESS__")?;
    let [percent, speed, eta]: [&str; 3] = data
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let percent = percent.strip_suffix('%')?.parse::<f32>().ok()?;
    let speed_valid = ["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s"]
        .iter()
        .any(|unit| {
            speed.strip_suffix(unit).is_some_and(|n| {
                n.trim()
                    .parse::<f64>()
                    .is_ok_and(|v| v.is_finite() && v >= 0.)
            })
        });
    let eta_valid = !eta.is_empty()
        && eta
            .split(':')
            .all(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()));
    if !percent.is_finite()
        || !(speed_valid || matches!(speed, "NA" | "Unknown"))
        || !(eta_valid || matches!(eta, "NA" | "Unknown"))
    {
        return None;
    }
    Some((
        (percent / 100.).clamp(0., 1.),
        // The percentage is the fraction, which the progress bar shows.
        format!("{speed} · {eta} remaining"),
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
    fn other_links_never_receive_any_platform_session() {
        let link = crate::engine::parse_link("https://videos.example.test/watch?id=7").unwrap();
        assert_eq!(link.source, Source::Other);
        assert!(!link.takes_session());
        for (source, domain) in [
            (Source::YouTube, ".youtube.com"),
            (Source::TikTok, ".tiktok.com"),
            (Source::X, ".x.com"),
            (Source::Facebook, ".facebook.com"),
            (Source::Instagram, ".instagram.com"),
        ] {
            let jar = SessionCookies::parse_for(
                &format!(
                    "# Netscape HTTP Cookie File
{domain}	TRUE	/	TRUE	0	TEST	fake-test-value
"
                ),
                source,
            )
            .unwrap();
            for (cmd, file) in [
                command_for(&link, Some(&jar)).unwrap(),
                inspect_command(&link, Some(&jar)).unwrap(),
            ] {
                let args = arguments(&cmd);
                assert!(file.is_none(), "{source:?}");
                assert!(!args.iter().any(|a| a == "--cookies"), "{source:?}");
                assert!(!args.iter().any(|a| a.contains("fake-test-value")));
                assert!(args.iter().any(|a| a == "--no-cookies"));
                assert!(args.iter().any(|a| a == "--no-cookies-from-browser"));
            }
        }
        // No cookie can be imported for other sites in the first place.
        assert!(
            SessionCookies::parse_for(
                "# Netscape HTTP Cookie File
.example.test	TRUE	/	TRUE	0	TEST	fake
",
                Source::Other,
            )
            .is_err()
        );
    }

    #[test]
    fn other_inspection_is_unrestricted_but_capped_and_local_files_stay_closed() {
        let link = crate::engine::parse_link("https://videos.example.test/watch?id=7").unwrap();
        let (cmd, _) = inspect_command(&link, None).unwrap();
        let args = arguments(&cmd);
        assert!(!args.iter().any(|a| a == "--use-extractors"));
        assert!(!args.iter().any(|a| a == "--enable-file-urls"));
        assert!(
            args.windows(2)
                .any(|a| a[0] == "--playlist-items" && a[1] == "1:25")
        );
        assert!(args.windows(2).any(|a| a[0] == "--downloader-args"
            && a[1] == "ffmpeg_i:-protocol_whitelist http,https,tcp,tls,crypto"));
        assert_eq!(&args[args.len() - 2..], ["--", link.url.as_str()]);
    }

    #[test]
    fn other_pages_offer_at_most_25_videos_and_skip_what_is_not_one() {
        let link = crate::engine::parse_link("https://videos.example.test/gallery").unwrap();
        let clip = |id: usize| {
            serde_json::json!({"id": format!("clip{id}"), "extractor_key": "Generic",
                "formats": [{"format_id": "0", "protocol": "https", "ext": "mp4"}]})
        };
        let mut entries: Vec<_> = (1..=30).map(clip).collect();
        entries[1]["formats"] =
            serde_json::json!([{"format_id": "a", "protocol": "https", "ext": "mp3"}]);
        entries[2]["live_status"] = "is_live".into();
        entries[3] = serde_json::Value::Null;
        let page = serde_json::json!({"_type": "playlist", "extractor_key": "Generic", "entries": entries.clone()});
        let Event::Entries(videos) = inspection_event(&page.to_string(), &link).unwrap() else {
            panic!("expected a chooser");
        };
        assert_eq!(videos.len(), 22);
        assert_eq!(videos[0].identity().entry_index, Some(1));
        assert_eq!(videos[1].identity().entry_index, Some(5));
        assert_eq!(videos.last().unwrap().identity().entry_index, Some(25));
        assert!(videos.iter().all(|v| v.source == Source::Other));
        // One video on the page is offered directly, keeping its position.
        let single = serde_json::json!({"_type": "playlist", "entries": [null, clip(9)]});
        let Event::Loaded(video) = inspection_event(&single.to_string(), &link).unwrap() else {
            panic!("expected one video");
        };
        assert_eq!(video.identity().entry_index, Some(2));
        // Nothing usable: the first reason is shown.
        let audio = serde_json::json!({"_type": "playlist", "entries": [entries[1]]});
        assert!(matches!(
            inspection_event(&audio.to_string(), &link),
            Err(EngineError::Unsupported(message)) if message.contains("audio only")
        ));
        let empty = serde_json::json!({"_type": "playlist", "entries": []});
        assert!(matches!(
            inspection_event(&empty.to_string(), &link),
            Err(EngineError::MissingVideo(Source::Other))
        ));
        // Nested collections are not opened.
        let nested = serde_json::json!({"_type": "playlist", "entries": [
            {"_type": "playlist", "entries": [clip(1)]}
        ]});
        assert!(inspection_event(&nested.to_string(), &link).is_err());
    }

    #[test]
    fn other_file_names_start_with_the_site_and_keep_the_id() {
        let template = Source::Other.output_template();
        assert!(template.starts_with("%(extractor_key).30s %(webpage_url_domain).60s "));
        assert!(template.ends_with("[%(id)s] [%(format_id)s].%(ext)s"));
        for source in [
            Source::YouTube,
            Source::TikTok,
            Source::X,
            Source::Facebook,
            Source::Instagram,
        ] {
            assert_ne!(source.output_template(), template);
        }
    }

    /// Serves the files in `root` on localhost until `done` is set: the address and the server.
    fn serve_folder(
        root: PathBuf,
        done: Arc<AtomicBool>,
    ) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
        use std::{io::Write, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
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
                        let text = String::from_utf8_lossy(&request);
                        assert!(
                            !text.contains("fake-test-value"),
                            "a session reached another site"
                        );
                        let name = text
                            .split_whitespace()
                            .nth(1)
                            .unwrap_or("/")
                            .trim_start_matches('/')
                            .split('?')
                            .next()
                            .unwrap_or("")
                            .to_owned();
                        let safe =
                            !name.contains("..") && !name.contains('/') && !name.contains('\\');
                        let Some(bytes) =
                            safe.then(|| std::fs::read(root.join(&name)).ok()).flatten()
                        else {
                            let _ = stream.write_all(
                                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            );
                            continue;
                        };
                        let mime = match name.rsplit('.').next() {
                            Some("html") => "text/html",
                            Some("m3u8") => "application/vnd.apple.mpegurl",
                            Some("ts") => "video/mp2t",
                            _ => "video/mp4",
                        };
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            bytes.len()
                        );
                        let _ = stream.write_all(head.as_bytes());
                        if !text.starts_with("HEAD ") {
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
        (address, server)
    }

    /// Other sites through the real helpers: a direct video file, a page with an embedded
    /// `<video>` and an HLS manifest, served from localhost. The pasted-link checks, which refuse
    /// localhost, are skipped by starting after them, as the YouTube local test does.
    #[test]
    #[ignore = "requires installed FFmpeg and yt-dlp, plus localhost sockets"]
    fn other_sites_local_fixtures_inspect_and_download() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-output")
            .join(format!("other-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ffmpeg = |extra: &[&str]| {
            let made = isolate(
                Command::new(crate::tools::path("ffmpeg"))
                    .current_dir(&root)
                    .args([
                        "-y",
                        "-v",
                        "error",
                        "-f",
                        "lavfi",
                        "-i",
                        "color=c=blue:s=160x90:r=10:d=2",
                        "-f",
                        "lavfi",
                        "-i",
                        "sine=frequency=440:duration=2",
                        "-c:v",
                        "mpeg4",
                        "-pix_fmt",
                        "yuv420p",
                        "-c:a",
                        "aac",
                        "-shortest",
                    ])
                    .args(extra),
            )
            .output()
            .unwrap();
            assert!(
                made.status.success(),
                "{}",
                String::from_utf8_lossy(&made.stderr)
            );
        };
        ffmpeg(&["clip.mp4"]);
        // MPEG-4 Part 2 in MPEG-TS does not survive yt-dlp's MP4 fixup; H.264 does.
        ffmpeg(&[
            "-c:v",
            "libx264",
            "-f",
            "hls",
            "-hls_time",
            "1",
            "-hls_list_size",
            "0",
            "stream.m3u8",
        ]);
        std::fs::write(
            root.join("page.html"),
            "<!doctype html><html><head><title>Fixture page</title></head><body>\
             <video controls src=\"clip.mp4\"></video></body></html>",
        )
        .unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let (address, server) = serve_folder(root.clone(), done.clone());
        let mut saved = Vec::new();
        for name in ["clip.mp4", "page.html", "stream.m3u8"] {
            let link = Link {
                source: Source::Other,
                url: format!("http://{address}/{name}?fixture=1"),
            };
            let video = match inspect_resolved(
                &link,
                Some(&crate::session::fake_session()),
                &AtomicBool::new(false),
            ) {
                Ok(Some(Event::Loaded(video))) => video,
                Ok(Some(Event::Entries(mut videos))) => videos.remove(0),
                Ok(_) => panic!("{name}: no video"),
                Err(error) => {
                    done.store(true, Ordering::Relaxed);
                    panic!("{name}: {error:?}");
                }
            };
            assert_eq!(video.source, Source::Other);
            let format = video.formats.iter().find(|f| f.video()).unwrap().clone();
            let folder = root.join(name.replace('.', "-"));
            std::fs::create_dir_all(&folder).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            run_download(
                video.identity(),
                format,
                folder,
                Some(crate::session::fake_session()),
                tx,
                Arc::new(AtomicBool::new(false)),
            );
            let path = rx
                .into_iter()
                .find_map(|event| match event {
                    Event::Complete(done) => Some(done.path),
                    Event::Failed(error) => panic!("{name}: {error:?}"),
                    _ => None,
                })
                .expect("no file");
            let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
            // The extractor (Generic, or HTML5MediaEmbed for the page), then the page's domain.
            assert!(file_name.contains(" 127.0.0.1"), "{file_name}");
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
            let json: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            assert!(
                json["streams"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["codec_type"] == "video")
            );
            saved.push(file_name);
        }
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        println!("Other-site fixtures saved: {saved:?}");
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
    #[ignore = "requires the bundled yt-dlp, uses offline metadata only"]
    fn scoped_cookies_follow_helper_request_rules() {
        let jar = SessionCookies::parse_for(
            concat!(
                "# Netscape HTTP Cookie File\n",
                ".x.com\tTRUE\t/media\tTRUE\t4102444800\tMATCH\tfake\n",
                ".x.com\tTRUE\t/media\tTRUE\t1\tEXPIRED\tfake\n",
                ".youtube.com\tTRUE\t/\tFALSE\t4102444800\tFOREIGN\tfake\n",
            ),
            Source::X,
        )
        .unwrap();
        let link = crate::engine::parse_link("https://x.com/example/status/123").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let info = dir.path().join("fixture.json");
        for (url, matches) in [
            ("https://sub.x.com/media/clip.mp4", true),
            ("http://sub.x.com/media/clip.mp4", false),
            ("https://x.com/media-other/clip.mp4", false),
            ("https://x.com.example/media/clip.mp4", false),
            ("https://www.youtube.com/media/clip.mp4", false),
        ] {
            std::fs::write(&info, serde_json::json!({"id":"fixture","title":"fixture","url":url,"ext":"mp4","extractor":"generic","extractor_key":"Generic","webpage_url":url}).to_string()).unwrap();
            let (mut cmd, _file) = command_for(&link, Some(&jar)).unwrap();
            cmd.arg("--load-info-json")
                .arg(&info)
                .args(["--simulate", "--print", "%(cookies)s"]);
            let output = process::Helper::new(&mut cmd, &AtomicBool::new(false))
                .time_limit(Duration::from_secs(30))
                .output()
                .unwrap();
            assert!(output.status.success());
            let cookies = String::from_utf8(output.stdout).unwrap();
            assert_eq!(cookies.contains("MATCH="), matches);
            assert!(!cookies.contains("EXPIRED") && !cookies.contains("FOREIGN"));
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
    fn instagram_mixed_carousel_preserves_each_video_position_and_identity() {
        let link = crate::engine::parse_link("instagram.com/p/Example/").unwrap();
        let photo = serde_json::json!({"extractor_key":"Instagram","formats":[{"format_id":"image","ext":"jpg","vcodec":"mjpeg","protocol":"https"}]});
        let clip = serde_json::json!({"extractor_key":"Instagram","id":"Example_2","formats":[{"format_id":"progressive","ext":"mp4","protocol":"https"}]});
        let mut post = serde_json::json!({"extractor_key":"Instagram","_type":"playlist","entries":[photo,clip]});
        let Event::Entries(videos) = inspection_event(&post.to_string(), &link).unwrap() else {
            panic!("expected a selector")
        };
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].identity().entry_index, Some(2));
        assert_eq!(videos[0].identity().id.as_deref(), Some("Example_2"));
        assert_eq!(videos[0].formats[0].selector(), "progressive");
        post["entries"] = serde_json::json!([photo]);
        assert!(matches!(
            inspection_event(&post.to_string(), &link),
            Err(EngineError::MissingVideo(Source::Instagram))
        ));
    }

    #[test]
    fn instagram_dash_audio_is_merged_unless_the_source_is_explicitly_silent() {
        let link = crate::engine::parse_link("instagram.com/reel/Example").unwrap();
        let mut post = serde_json::json!({"id":"Example","extractor_key":"Instagram","formats":[
            {"format_id":"dash-video","ext":"mp4","protocol":"https","vcodec":"avc1","acodec":"none"},
            {"format_id":"dash-audio","ext":"m4a","protocol":"https","vcodec":"none","acodec":"aac"}
        ]});
        let video = parse_for(&post.to_string(), &link).unwrap();
        assert!(video.formats[0].selector().starts_with("dash-video+"));
        post["formats"].as_array_mut().unwrap().pop();
        let silent = parse_for(&post.to_string(), &link).unwrap();
        assert_eq!(silent.formats[0].selector(), "dash-video");
    }

    #[test]
    fn instagram_cookie_jar_only_reaches_instagram_posts() {
        let jar = SessionCookies::parse_for(
            "# Netscape HTTP Cookie File\n.instagram.com\tTRUE\t/\tTRUE\t0\tTEST\tfake\n",
            Source::Instagram,
        )
        .unwrap();
        for (url, expected) in [
            ("instagram.com/p/Example", true),
            ("facebook.com/reel/123", false),
            ("x.com/example/status/123", false),
        ] {
            let link = crate::engine::parse_link(url).unwrap();
            let (cmd, file) = command_for(&link, Some(&jar)).unwrap();
            assert_eq!(file.is_some(), expected);
            assert_eq!(cmd.get_args().any(|arg| arg == "--cookies"), expected);
        }
    }

    #[test]
    fn facebook_cookie_jar_only_reaches_individual_facebook_links() {
        let jar = SessionCookies::parse_for(
            "# Netscape HTTP Cookie File\n.facebook.com\tTRUE\t/\tTRUE\t0\tTEST\tfake\n",
            Source::Facebook,
        )
        .unwrap();
        for (url, expected) in [
            ("facebook.com/reel/123", true),
            ("facebook.com/share/v/Example", false),
            ("x.com/example/status/123", false),
        ] {
            let link = crate::engine::parse_link(url).unwrap();
            let (cmd, file) = command_for(&link, Some(&jar)).unwrap();
            assert_eq!(file.is_some(), expected);
            assert_eq!(cmd.get_args().any(|arg| arg == "--cookies"), expected);
        }
    }

    #[test]
    fn lost_audio_is_a_failure_and_a_silent_source_is_not() {
        // A video-only format whose audio could not be combined fails with its own message.
        for errors in [
            "ERROR: Postprocessing: Conversion failed!",
            "ERROR: You have requested merging of multiple formats but ffmpeg is not installed",
        ] {
            assert_eq!(
                explain_download(Source::YouTube, Sound::Merged, errors),
                EngineError::MissingAudio
            );
            // A format that needed no merging keeps the site's explanation.
            for sound in [Sound::Included, Sound::None, Sound::Unknown] {
                assert_ne!(
                    explain_download(Source::YouTube, sound, errors),
                    EngineError::MissingAudio
                );
            }
        }
        // A failed audio request is still the site's network error.
        assert_eq!(
            explain_download(
                Source::YouTube,
                Sound::Merged,
                "ERROR: unable to download video data: <urlopen error timed out>"
            ),
            EngineError::TimedOut(Source::YouTube)
        );
        // A saved file without sound fails split and combined formats, never a silent source
        // or a direct file of unknown sound; an unreadable file does not fail the download.
        for sound in [Sound::Merged, Sound::Included] {
            assert_eq!(
                check_sound(sound, Some(false)),
                Err(EngineError::MissingAudio)
            );
            assert_eq!(check_sound(sound, Some(true)), Ok(()));
            assert_eq!(check_sound(sound, None), Ok(()));
        }
        for sound in [Sound::None, Sound::Unknown] {
            assert_eq!(check_sound(sound, Some(false)), Ok(()));
        }
        assert!(EngineError::MissingAudio.user_message().contains("sound"));
    }

    #[test]
    fn parses_progress_and_ignores_noise() {
        assert_eq!(
            progress("__PROGRESS__ 42.5%|2 MiB/s|00:03").unwrap(),
            (0.425, "2 MiB/s · 00:03 remaining".to_string())
        );
        assert!(progress("[Merger] merging formats").is_none());
        assert!(progress("__PROGRESS__NA|NA|NA").is_none());
        assert!(progress("__PROGRESS__42%|Cookie: session=secret|00:03").is_none());
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
    fn x_links_and_collection_entries_are_scoped_to_one_post() {
        let link = crate::engine::parse_link("https://mobile.twitter.com/example/status/123?s=20")
            .unwrap();
        assert_eq!(link.url, "https://x.com/example/status/123");
        assert!(link.takes_session());
        for bad in [
            "https://x.com.example/status/1",
            "https://x.com/example",
            "https://x.com/example/status/123/video/2",
            "http://x.com/example/status/1",
        ] {
            assert!(crate::engine::parse_link(bad).is_err());
        }
        let entry = serde_json::json!({"id":"101","extractor_key":"Twitter","formats":[{"format_id":"http-128","ext":"mp4","protocol":"https"}]});
        let mut other = entry.clone();
        other["id"] = "102".into();
        let mut post = serde_json::json!({"_type":"playlist","extractor_key":"Twitter","entries":[entry,other]});
        let Event::Entries(entries) = inspection_event(&post.to_string(), &link).unwrap() else {
            panic!("expected entry chooser")
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].identity().entry_index, Some(2));
        assert_eq!(entries[1].identity().id.as_deref(), Some("102"));
        assert_eq!(entries[1].formats[0].selector(), "http-128");
        let mut animated = post["entries"][0].clone();
        animated["formats"][0]["format_id"] = "http".into();
        animated["formats"][0]["acodec"] = "none".into();
        let Event::Loaded(clip) = inspection_event(&animated.to_string(), &link).unwrap() else {
            panic!("expected a native MP4");
        };
        assert_eq!(clip.formats[0].selector(), "http");
        assert_eq!(
            Source::X.explain("Requires authentication"),
            EngineError::SignInNeeded(Source::X)
        );
        assert!(!Source::X.explain("Requires authentication").needs_session());
        post["entries"][1]["extractor_key"] = "Generic".into();
        assert!(inspection_event(&post.to_string(), &link).is_err());
    }

    #[test]
    #[ignore = "requires a permitted X post and installed download tools"]
    fn x_authorized_production_pipeline() {
        let link =
            crate::engine::parse_link(&std::env::var("STREAM_DLP_X_TEST_URL").unwrap()).unwrap();
        assert_eq!(link.source, Source::X);
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        inspect(
            link,
            Some(crate::session::fake_session()),
            tx,
            cancel.clone(),
        );
        let event = rx.recv().unwrap();
        let mode = std::env::var("STREAM_DLP_X_TEST_CASE").unwrap();
        if mode == "login" {
            let Event::Failed(error) = event else {
                panic!("Expected inspection to stop at the X sign-in gate");
            };
            assert_eq!(error, EngineError::SignInNeeded(Source::X));
            assert!(!error.needs_session());
            return;
        }
        if mode == "photo" {
            assert!(matches!(
                event,
                Event::Failed(EngineError::MissingVideo(Source::X))
                    | Event::Failed(EngineError::Unsupported(_))
            ));
            return;
        }
        let video = match (mode.as_str(), event) {
            ("single" | "animated", Event::Loaded(v)) => v,
            ("multi", Event::Entries(mut entries)) if entries.len() >= 2 => entries.remove(1),
            (_, Event::Failed(e)) => panic!("{e}"),
            _ => panic!("Unexpected post response"),
        };
        verify_saved_post(video, mode.as_str(), cancel);
    }

    #[test]
    #[ignore = "requires a permitted Facebook video with sound and installed tools"]
    fn facebook_authorized_production_pipeline() {
        let link =
            crate::engine::parse_link(&std::env::var("STREAM_DLP_FACEBOOK_TEST_URL").unwrap())
                .unwrap();
        assert_eq!(link.source, Source::Facebook);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        inspect(link, None, tx, cancel.clone());
        match rx.recv().unwrap() {
            Event::Loaded(video) => verify_saved_post(video, "facebook", cancel),
            Event::Failed(error) => panic!("{error}"),
            _ => panic!("Expected an individual Facebook video"),
        }
    }

    #[test]
    #[ignore = "requires a permitted Instagram sample and installed download tools"]
    fn instagram_authorized_production_pipeline() {
        let link =
            crate::engine::parse_link(&std::env::var("STREAM_DLP_INSTAGRAM_TEST_URL").unwrap())
                .unwrap();
        assert_eq!(link.source, Source::Instagram);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        inspect(link, None, tx, cancel.clone());
        let mode =
            std::env::var("STREAM_DLP_INSTAGRAM_TEST_CASE").unwrap_or_else(|_| "single".into());
        match (mode.as_str(), rx.recv().unwrap()) {
            ("single", Event::Loaded(video)) => verify_saved_post(video, "instagram", cancel),
            ("carousel", Event::Entries(mut videos)) if videos.len() >= 2 => {
                verify_saved_post(videos.remove(1), "instagram-carousel", cancel)
            }
            ("photo", Event::Failed(EngineError::MissingVideo(Source::Instagram))) => (),
            (_, Event::Failed(error)) => panic!("{error}"),
            _ => panic!("Unexpected Instagram inspection result"),
        }
    }

    fn verify_saved_post(video: Video, mode: &str, cancel: Arc<AtomicBool>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-output");
        std::fs::create_dir_all(&root).unwrap();
        let folder = tempfile::tempdir_in(root).unwrap();
        download(
            video.identity(),
            video.formats[0].clone(),
            folder.path().into(),
            Some(crate::session::fake_session()),
            tx,
            cancel,
        );
        let file = rx
            .into_iter()
            .find_map(|event| match event {
                Event::Complete(done) => {
                    assert_eq!(done.media, video.identity());
                    Some(done.path)
                }
                Event::Failed(e) => panic!("{e}"),
                _ => None,
            })
            .expect("No completed file");
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
        assert!(
            file.file_name()
                .unwrap()
                .to_string_lossy()
                .contains(&format!("[{}]", video.id.unwrap()))
        );
        if mode == "animated"
            || (mode == "instagram-carousel"
                && video
                    .formats
                    .iter()
                    .all(|f| f.acodec.as_deref() == Some("none")))
        {
            assert_eq!(file.extension().and_then(|ext| ext.to_str()), Some("mp4"));
            let probe = isolate(
                Command::new(crate::tools::path("ffprobe"))
                    .args([
                        "-v",
                        "error",
                        "-count_frames",
                        "-show_entries",
                        "stream=codec_type,nb_read_frames",
                        "-of",
                        "json",
                    ])
                    .arg(&file),
            )
            .output()
            .unwrap();
            assert!(probe.status.success());
            let metadata: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            let streams = metadata["streams"].as_array().unwrap();
            assert_eq!(streams.len(), 1);
            assert_eq!(streams[0]["codec_type"], "video");
            assert!(
                streams[0]["nb_read_frames"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    > 1
            );
        }
        let decoded = isolate(
            Command::new(crate::tools::path("ffmpeg"))
                .args(["-nostdin", "-v", "error", "-xerror", "-i"])
                .arg(file)
                .args([
                    "-map",
                    "0:v:0",
                    "-map",
                    if matches!(mode, "facebook" | "instagram") {
                        "0:a:0"
                    } else {
                        "0:a?"
                    },
                    "-f",
                    "null",
                    "-",
                ]),
        )
        .output()
        .unwrap();
        assert!(decoded.status.success());
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
