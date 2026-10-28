//! Experimental website automation; never uses private Streamable API endpoints.
use crate::process;
use eframe::egui;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const DRIVER: &str = include_str!("../scripts/streamable-upload.js");

#[derive(Debug, Default)]
pub struct Status {
    pub active: bool,
    pub message: String,
    pub percent: Option<f32>,
    pub url: Option<String>,
    pub candidate: Option<String>,
    pub failed: bool,
}
#[derive(Deserialize)]
struct Wire {
    kind: String,
    stage: Option<String>,
    percent: Option<f32>,
    url: Option<String>,
}
enum Event {
    Wire(Wire),
    Finished(Result<(), String>),
}
pub struct Uploader {
    pub status: Status,
    cancel: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
}
impl Default for Uploader {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            status: Status::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
        }
    }
}
impl Drop for Uploader {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Uploader {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn start(&mut self, path: PathBuf, ctx: &egui::Context) {
        if self.status.active {
            return;
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        self.status = Status {
            active: true,
            message: "Opening a private upload browser…".into(),
            ..Default::default()
        };
        let (tx, cancel, ctx) = (self.tx.clone(), self.cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = run(&path, &cancel, |wire| {
                let _ = tx.send(Event::Wire(wire));
                ctx.request_repaint();
            });
            let _ = tx.send(Event::Finished(result));
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Wire(wire) => self.status.apply(wire),
                Event::Finished(result) => {
                    self.status.active = false;
                    if let Err(error) = result {
                        if !self.status.failed {
                            self.status.failed = true;
                            self.status.message = error;
                        }
                    } else if self.status.url.is_none() {
                        self.status.failed = true;
                        self.status.message = "The browser stopped before playback could be verified. The video may already be uploaded; no retry was made.".into();
                    }
                }
            }
        }
    }
}
impl Status {
    fn apply(&mut self, wire: Wire) {
        match wire.kind.as_str() {
            "failed" => {
                self.failed = true;
                self.percent = None;
                self.message = match wire.stage.as_deref() {
                    Some("upload_control_missing" | "upload_page_missing") => "Streamable’s upload page could not be recognized. Use the manual website option.",
                    Some("upload_page_timeout") => "The upload page was not ready before the time limit. Complete any browser verification and try again manually.",
                    Some("website_upload_failed") => "Streamable reported an upload error. Check its file limits or use the manual website option.",
                    Some("playback_failed" | "playback_verification_timeout") => "A share link was created, but playable video could not be verified. Check the link before uploading again.",
                    _ => "The automatic upload stopped before completion was verified. The video may already exist on Streamable; no retry was made.",
                }.into();
            }
            "candidate" => self.candidate = wire.url.and_then(|u| share_url(&u)),
            "complete" => {
                if let Some(url) = wire.url.and_then(|u| share_url(&u)) {
                    self.url = Some(url);
                    self.percent = Some(1.);
                    self.message = "Uploaded. Streamable playback verified.".into();
                }
            }
            "progress" => {
                self.percent = wire
                    .percent
                    .filter(|v| v.is_finite() && (0. ..=100.).contains(v))
                    .map(|v| v / 100.);
                self.message = match wire.stage.as_deref() {
                    Some("uploading") => "Uploading your video…",
                    Some("processing") => "Streamable is processing the video…",
                    Some("verifying") => "Checking the share link and video playback…",
                    Some("attention") => "Complete sign-in or verification in the upload browser. The app will continue automatically.",
                    _ => "Preparing Streamable…",
                }.into();
            }
            _ => {}
        }
    }
}
fn share_url(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    let id = url.path().strip_prefix('/')?;
    (url.scheme() == "https"
        && url.host_str() == Some("streamable.com")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (3..=16).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_alphanumeric()))
    .then(|| url.to_string())
}

fn browser() -> Option<PathBuf> {
    #[cfg(windows)]
    let candidates = [
        PathBuf::from("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"),
        PathBuf::from("C:/Program Files/Microsoft/Edge/Application/msedge.exe"),
        PathBuf::from("C:/Program Files/Google/Chrome/Application/chrome.exe"),
        dirs::data_local_dir()
            .unwrap_or_default()
            .join("Google/Chrome/Application/chrome.exe"),
    ];
    #[cfg(target_os = "macos")]
    let candidates = [
        PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
    ];
    #[cfg(not(any(windows, target_os = "macos")))]
    let candidates = [
        PathBuf::from("/usr/bin/google-chrome"),
        PathBuf::from("/usr/bin/chromium"),
        PathBuf::from("/usr/bin/microsoft-edge"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}
fn run(path: &Path, cancel: &AtomicBool, mut emit: impl FnMut(Wire)) -> Result<(), String> {
    let path = std::path::absolute(path).map_err(|_| "Cannot locate the selected video.")?;
    if !path.is_file() {
        return Err("The selected video was moved or deleted.".into());
    }
    let browser = browser().ok_or("Install Microsoft Edge or Google Chrome to use automatic uploading, or use the manual website option.")?;
    let work = tempfile::Builder::new()
        .prefix("stream-dlp-upload-")
        .tempdir()
        .map_err(|_| "Cannot create the temporary upload session.")?;
    let profile = work.path().join("profile");
    let script = work.path().join("upload.js");
    std::fs::write(&script, DRIVER).map_err(|_| "Cannot prepare the upload helper.")?;
    let private = if browser
        .to_string_lossy()
        .to_ascii_lowercase()
        .contains("edge")
    {
        "--inprivate"
    } else {
        "--incognito"
    };
    let mut command = Command::new(browser);
    command
        .args([
            "--remote-debugging-port=0",
            "--remote-debugging-address=127.0.0.1",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-sync",
            private,
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(["--new-window", "https://streamable.com/"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut browser = process::Owned(
        process::isolate(&mut command)
            .spawn()
            .map_err(|_| "Could not start the private upload browser.")?,
    );
    let start = Instant::now();
    let port = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(stopped());
        }
        if start.elapsed() > Duration::from_secs(30) {
            return Err("The upload browser did not start in time. Browser policy may prevent automation. Use the manual website option.".into());
        }
        if let Ok(text) = std::fs::read_to_string(profile.join("DevToolsActivePort"))
            && let Some(port) = text
                .lines()
                .next()
                .and_then(|p| p.parse::<u16>().ok())
                .filter(|p| *p > 0)
        {
            break port;
        }
        if matches!(browser.0.try_wait(), Ok(Some(_))) {
            return Err("The upload browser closed before connecting.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let result = process::Helper::new(
        Command::new(crate::tools::path("deno"))
            .args(["run", "--no-prompt", "--no-config", "--no-remote"])
            .arg(format!("--allow-net=127.0.0.1:{port}"))
            .arg(script)
            .arg(format!("http://127.0.0.1:{port}"))
            .arg(path),
        cancel,
    )
    .time_limit(Duration::from_secs(20 * 60).saturating_sub(start.elapsed()))
    .ignore_stderr()
    .lines(|_, line| {
        if line.len() <= 2048
            && let Ok(wire) = serde_json::from_str::<Wire>(line)
        {
            emit(wire);
        }
    });
    drop(browser);
    // Owned processes have stopped before the temporary profile is removed.
    drop(work);
    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err("Automatic uploading could not finish. Check the reported stage or use the manual website option. The video may already be uploaded; no retry was made.".into()),
        Err(process::Error::Cancelled) => Err(stopped()),
        Err(process::Error::TimedOut) => Err("Upload timed out. The video may already exist on Streamable; no automatic retry was made.".into()),
        Err(process::Error::Start(_)) => Err("Cannot start Deno. Install the download tools and try again.".into()),
        Err(_) => Err("The upload helper stopped unexpectedly.".into()),
    }
}
fn stopped() -> String {
    "Upload stopped. Streamable may already have received the video. No automatic retry was made."
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_preserves_specific_reason_and_unverified_link() {
        let mut uploader = Uploader::default();
        uploader.status.active = true;
        for wire in [
            Wire {
                kind: "candidate".into(),
                stage: None,
                percent: None,
                url: Some("https://streamable.com/abc123".into()),
            },
            Wire {
                kind: "failed".into(),
                stage: Some("playback_failed".into()),
                percent: None,
                url: None,
            },
        ] {
            uploader.tx.send(Event::Wire(wire)).unwrap();
        }
        uploader
            .tx
            .send(Event::Finished(Err("generic process error".into())))
            .unwrap();
        uploader.poll();
        assert!(!uploader.status.active);
        assert!(uploader.status.failed && uploader.status.url.is_none());
        assert!(uploader.status.message.contains("playable video"));
        assert_eq!(
            uploader.status.candidate.as_deref(),
            Some("https://streamable.com/abc123")
        );
    }
    #[test]
    #[ignore = "publishes a video to Streamable; requires explicit user authorization and STREAM_DLP_UPLOAD_TEST_FILE"]
    fn authorized_streamable_upload() {
        let file = std::env::var_os("STREAM_DLP_UPLOAD_TEST_FILE")
            .expect("Set only to the specifically authorized video file");
        let mut verified = None;
        run(Path::new(&file), &AtomicBool::new(false), |wire| {
            if wire.kind == "progress" {
                eprintln!("Upload stage: {:?}", wire.stage);
            }
            if wire.kind == "candidate" {
                eprintln!("Allocated test link: {:?}", wire.url);
            }
            if wire.kind == "complete" {
                verified = wire.url.and_then(|u| share_url(&u));
            } else if wire.kind == "failed" {
                eprintln!("Upload driver reported: {:?}", wire.stage);
            }
        })
        .unwrap();
        println!(
            "Verified uploaded video: {}",
            verified.expect("No playable share link returned")
        );
    }
    #[test]
    fn only_scoped_share_links_are_accepted() {
        assert!(share_url("https://streamable.com/abc123").is_some());
        for bad in [
            "https://streamable.com.evil.test/abc123",
            "https://evil.test/abc123",
            "http://streamable.com/abc123",
            "https://user@streamable.com/abc123",
            "https://streamable.com/abc123?token=secret",
            "https://streamable.com/videos/abc123",
            "https://streamable.com/abc123#token",
        ] {
            assert!(share_url(bad).is_none());
        }
    }
    #[test]
    fn allocated_link_is_not_a_completed_upload() {
        let mut status = Status::default();
        status.apply(Wire {
            kind: "candidate".into(),
            url: Some("https://streamable.com/abc123".into()),
            stage: None,
            percent: None,
        });
        assert!(status.url.is_none());
        assert!(status.candidate.is_some());
        status.apply(Wire {
            kind: "complete".into(),
            url: Some("https://evil.test/abc123".into()),
            stage: None,
            percent: None,
        });
        assert!(status.url.is_none());
    }
    #[test]
    fn website_progress_does_not_invent_percentages() {
        let mut status = Status::default();
        for percent in [None, Some(-1.), Some(101.), Some(f32::NAN)] {
            status.apply(Wire {
                kind: "progress".into(),
                stage: Some("processing".into()),
                percent,
                url: None,
            });
            assert!(status.percent.is_none());
        }
    }
}
