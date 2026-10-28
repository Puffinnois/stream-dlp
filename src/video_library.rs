//! User-selected local videos and a browser-assisted Streamable handoff.
use crate::process;
use eframe::egui;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

const STREAMABLE: &str = "https://streamable.com/";
const EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "m4v", "avi", "mpeg", "mpg", "mts", "m2ts", "3gp", "ogv", "flv",
    "wmv",
];

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    bytes: u64,
}
#[derive(Debug)]
pub(crate) struct Video {
    path: PathBuf,
    bytes: u64,
    container: String,
    pub(crate) duration: Option<f64>,
}
enum ResultData {
    Folder(PathBuf, Vec<Entry>, bool),
    Video(Video),
    Cancelled,
}
type JobResult = (u64, Result<ResultData, String>);

pub struct Library {
    previews: crate::video_preview::Previews,
    pub uploader: crate::streamable_upload::Uploader,
    folder: Option<PathBuf>,
    entries: Vec<Entry>,
    truncated: bool,
    selected: Option<Video>,
    busy: bool,
    message: String,
    error: Option<String>,
    generation: u64,
    cancel: Arc<AtomicBool>,
    tx: mpsc::Sender<JobResult>,
    rx: mpsc::Receiver<JobResult>,
}
impl Default for Library {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            previews: crate::video_preview::Previews::default(),
            uploader: crate::streamable_upload::Uploader::default(),
            folder: None,
            entries: vec![],
            truncated: false,
            selected: None,
            busy: false,
            message: String::new(),
            error: None,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
        }
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Library {
    fn job(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(&AtomicBool) -> Result<ResultData, String> + Send + 'static,
    ) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.generation += 1;
        self.busy = true;
        self.error = None;
        self.message.clear();
        let (id, tx, cancel, ctx) = (
            self.generation,
            self.tx.clone(),
            self.cancel.clone(),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let _ = tx.send((id, work(&cancel)));
            ctx.request_repaint();
        });
    }
    pub fn select_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.uploader.status.active {
            return;
        }
        self.selected = None;
        self.job(ctx, move |cancel| {
            probe(&path, cancel).map(ResultData::Video)
        });
    }
    fn scan(&mut self, ctx: &egui::Context, folder: PathBuf) {
        self.job(ctx, move |cancel| {
            scan(&folder, cancel)
                .map(|(entries, truncated)| ResultData::Folder(folder, entries, truncated))
        });
    }
    pub fn poll(&mut self) {
        self.uploader.poll();
        while let Ok((id, result)) = self.rx.try_recv() {
            if id != self.generation {
                continue;
            }
            self.busy = false;
            match result {
                Ok(ResultData::Folder(folder, entries, truncated)) => {
                    self.previews.clear();
                    self.folder = Some(folder);
                    self.entries = entries;
                    self.truncated = truncated;
                    self.selected = None;
                }
                Ok(ResultData::Video(video)) => self.selected = Some(video),
                Ok(ResultData::Cancelled) => {}
                Err(error) => self.error = Some(error),
            }
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        use egui::RichText;
        let ctx = &ui.ctx().clone();
        let busy = self.busy || self.uploader.status.active;
        crate::app::theme::page_header(
            ui,
            "My videos",
            "Your local videos, ready to play or share.",
        );
        crate::app::theme::card(ui, |ui| {
            ui.label(
                RichText::new("Video folder")
                    .size(14.)
                    .color(crate::app::theme::INK)
                    .strong(),
            );
            ui.label("Choose a folder to see your videos here.");
            ui.horizontal_wrapped(|ui| {
                if crate::app::theme::primary(ui, "Add video folder…", !busy).clicked() {
                    let start = self
                        .folder
                        .clone()
                        .unwrap_or_else(crate::session::export::downloads_folder);
                    self.job(ctx, move |cancel| {
                        let Some(folder) =
                            rfd::FileDialog::new().set_directory(start).pick_folder()
                        else {
                            return Ok(ResultData::Cancelled);
                        };
                        scan(&folder, cancel).map(|(entries, truncated)| {
                            ResultData::Folder(folder, entries, truncated)
                        })
                    });
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Choose existing video…"))
                    .clicked()
                {
                    let start = self
                        .folder
                        .clone()
                        .unwrap_or_else(crate::session::export::downloads_folder);
                    self.job(ctx, move |cancel| {
                        let Some(path) = rfd::FileDialog::new()
                            .set_directory(start)
                            .add_filter("Videos", EXTENSIONS)
                            .pick_file()
                        else {
                            return Ok(ResultData::Cancelled);
                        };
                        probe(&path, cancel).map(ResultData::Video)
                    });
                }
                if ui
                    .add_enabled(!busy && self.folder.is_some(), egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.scan(ctx, self.folder.clone().unwrap());
                }
            });
            if let Some(folder) = &self.folder {
                ui.label(
                    RichText::new(folder.display().to_string())
                        .size(12.)
                        .color(crate::app::theme::MUTED),
                );
                ui.small(
                    "Folder selection lasts for this app session. Subfolders are not scanned.",
                );
            }
        });
        ui.add_space(18.);
        crate::app::theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Your videos")
                        .size(14.)
                        .color(crate::app::theme::INK)
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("{} videos", self.entries.len()))
                        .size(12.)
                        .color(crate::app::theme::MUTED),
                );
            });
            if self.entries.is_empty() {
                ui.add_space(12.);
                ui.label(if self.folder.is_some() {
                    "No videos found in this folder."
                } else {
                    "Your videos will appear here."
                });
                ui.label(
                    RichText::new("Add a video folder or choose an existing video to get started.")
                        .size(13.)
                        .color(crate::app::theme::MUTED),
                );
                ui.add_space(12.);
            }
            if self.truncated {
                ui.label("Showing the first 1,000 videos found. Choose a smaller folder to see other files.");
            }
            let mut chosen = None;
            egui::ScrollArea::vertical()
                .id_salt("local_videos")
                .max_height(330.)
                .show_rows(ui, 76.5, self.entries.len(), |ui, rows| {
                    for index in rows {
                        let entry = &self.entries[index];
                        let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
                        let selected = self.selected.as_ref().is_some_and(|v| v.path == entry.path);
                        ui.horizontal(|ui| {
                            let preview =
                                self.previews
                                    .show(ui, &entry.path, None, egui::vec2(136., 76.5));
                            if preview.on_hover_text("Select video").clicked() && !busy {
                                chosen = Some(entry.path.clone());
                            }
                            let button = egui::Button::new(format!(
                                "{name}  ·  {:.1} MB",
                                entry.bytes as f64 / 1_000_000.
                            ))
                            .fill(if selected {
                                egui::Color32::from_rgb(239, 238, 255)
                            } else {
                                egui::Color32::WHITE
                            })
                            .stroke(egui::Stroke::new(
                                1.,
                                if selected {
                                    crate::app::theme::BLUE
                                } else {
                                    crate::app::theme::BORDER
                                },
                            ))
                            .corner_radius(8.)
                            .min_size(egui::vec2(ui.available_width(), 76.5))
                            .truncate();
                            if ui.add_enabled(!busy, button).clicked() {
                                chosen = Some(entry.path.clone());
                            }
                        });
                    }
                });
            if let Some(path) = chosen {
                self.select_file(ctx, path);
            }
            if self.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Choosing or checking video files…");
                    if ui.button("Cancel").clicked() {
                        self.cancel_job();
                    }
                });
                ctx.request_repaint_after(Duration::from_millis(80));
            }
        });
        if let Some(video) = &self.selected {
            ui.add_space(18.);
            crate::app::theme::card(ui, |ui| {
                ui.label(
                    RichText::new("Selected video")
                        .size(14.)
                        .color(crate::app::theme::INK)
                        .strong(),
                );
                ui.strong(video.path.file_name().unwrap_or_default().to_string_lossy());
                let width = ui.available_width().min(480.);
                if self
                    .previews
                    .show(ui, &video.path, None, egui::vec2(width, width * 9. / 16.))
                    .on_hover_text("Play video")
                    .clicked()
                {
                    self.error = open_target(video.path.as_os_str()).err();
                }
                let duration = video
                    .duration
                    .map(|s| format!("{s:.1} seconds"))
                    .unwrap_or_else(|| "Duration unavailable".into());
                ui.label(format!(
                    "{} · {duration} · {:.1} MB",
                    video.container,
                    video.bytes as f64 / 1_000_000.
                ));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Play video").clicked() {
                        self.error = open_target(video.path.as_os_str()).err();
                    }
                    if ui.button("Show file").clicked() {
                        self.error = reveal(&video.path).err();
                    }
                });
                ui.separator();
                ui.strong("Upload on Streamable");
                ui.label("Automatic upload uses a separate private Edge or Chrome window. The app selects your video and returns the share link. Complete any sign-in or verification in that window.");
                ui.label(RichText::new("Uploading sends this video to Streamable. Anyone with its link may be able to watch it. Guest links are temporary.").size(12.).color(crate::app::theme::MUTED));
                ui.horizontal_wrapped(|ui| {
                    ui.small("By choosing Upload automatically, you accept Streamable’s");
                    ui.hyperlink_to("Terms of Service", "https://streamable-support.zendesk.com/hc/en-us/articles/44623209368468-Terms-of-Service");
                });
                if crate::app::theme::primary(ui, "Upload automatically (experimental)", !busy)
                    .clicked()
                {
                    self.uploader.start(video.path.clone(), ctx);
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Use website manually"))
                    .clicked()
                {
                    match handoff(&video.path, reveal, || {
                        open_target(std::ffi::OsStr::new(STREAMABLE))
                    }) {
                        Ok(()) => {
                            self.error = None;
                            self.message = "Streamable opened and the file was revealed. Select or drag it into the website to upload.".into();
                        }
                        Err(error) => {
                            self.message.clear();
                            self.error = Some(error);
                        }
                    }
                }
            });
        }
        if self.uploader.status.active || !self.uploader.status.message.is_empty() {
            ui.add_space(18.);
            crate::app::theme::card(ui, |ui| {
                ui.label(
                    RichText::new("Streamable upload")
                        .size(14.)
                        .color(crate::app::theme::INK)
                        .strong(),
                );
                let status = &self.uploader.status;
                if status.failed {
                    ui.colored_label(egui::Color32::from_rgb(180, 45, 45), &status.message);
                } else {
                    ui.label(&status.message);
                }
                if status.active {
                    if let Some(percent) = status.percent {
                        ui.add(egui::ProgressBar::new(percent).show_percentage());
                    } else {
                        ui.spinner();
                    }
                    if ui.button("Cancel upload").clicked() {
                        self.uploader.cancel();
                    }
                    ctx.request_repaint_after(Duration::from_millis(80));
                }
                if let Some(url) = &self.uploader.status.url {
                    ui.hyperlink(url);
                    if ui.button("Copy share link").clicked() {
                        ui.ctx().copy_text(url.clone());
                    }
                } else if !self.uploader.status.active
                    && let Some(url) = &self.uploader.status.candidate
                {
                    ui.label("A link was created, but playback was not verified. Check it before uploading again.");
                    ui.hyperlink(url);
                }
            });
        }
        ui.add_space(12.);
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(180, 45, 45), error);
        }
    }
    fn cancel_job(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.generation += 1;
        self.busy = false;
    }
}

fn scan(folder: &Path, cancel: &AtomicBool) -> Result<(Vec<Entry>, bool), String> {
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
pub(crate) fn probe(path: &Path, cancel: &AtomicBool) -> Result<Video, String> {
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

pub(crate) fn open_target(target: &std::ffi::OsStr) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    process::isolate(command.arg(target))
        .spawn()
        .map(|_| ())
        .map_err(|_| {
            "Could not open the browser or video player. Open the file or streamable.com manually."
                .into()
        })
}
fn reveal(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("This video was moved or deleted. Choose it again.".into());
    }
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("explorer.exe");
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        c.arg(arg);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = Command::new("/usr/bin/open");
        c.arg("-R").arg(path);
        c
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    return crate::platform::open_folder(path.parent().unwrap_or(Path::new(".")))
        .map_err(|_| "Could not open the video folder.".into());
    #[cfg(any(windows, target_os = "macos"))]
    process::isolate(&mut command)
        .spawn()
        .map(|_| ())
        .map_err(|_| "Could not reveal the video. Open its folder manually.".into())
}
fn handoff(
    path: &Path,
    reveal_file: impl FnOnce(&Path) -> Result<(), String>,
    open_browser: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if !path.is_file() {
        return Err("This video was moved or deleted. Choose it again.".into());
    }
    reveal_file(path)?;
    open_browser()
        .map_err(|e| format!("The file was revealed, but Streamable could not be opened. {e}"))
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
    fn failed_handoff_preserves_file_and_never_claims_upload() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"original").unwrap();
        assert!(
            handoff(
                file.path(),
                |_| Err("reveal failed".into()),
                || panic!("must not open browser")
            )
            .is_err()
        );
        assert!(
            handoff(file.path(), |_| Ok(()), || Err("browser failed".into()))
                .unwrap_err()
                .contains("file was revealed")
        );
        assert_eq!(std::fs::read(file.path()).unwrap(), b"original");
        assert!(
            handoff(
                &file.path().with_extension("missing"),
                |_| panic!(),
                || panic!()
            )
            .is_err()
        );
    }
    #[test]
    fn cancelled_jobs_cannot_replace_selection() {
        let mut library = Library::default();
        library.busy = true;
        library
            .tx
            .send((
                0,
                Ok(ResultData::Video(Video {
                    path: "old.mp4".into(),
                    bytes: 10,
                    container: "mp4".into(),
                    duration: None,
                })),
            ))
            .unwrap();
        library.cancel_job();
        library.poll();
        assert!(library.selected.is_none());
        assert!(!library.busy);
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
