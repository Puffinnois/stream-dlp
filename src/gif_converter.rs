//! Local, bounded two-pass GIF encoding. Workers own all temporary files.
use crate::process::{self, Stream};
use eframe::egui::{self, RichText};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
struct Settings {
    start: f64,
    length: f64,
    width: u32,
    fps: u32,
    colors: u32,
    smooth: bool,
    repeat: bool,
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
    fn validate(&self, duration: f64) -> Result<(), String> {
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
enum Event {
    Dismissed,
    Source(PathBuf, f64),
    Progress(f32),
    Saved(PathBuf, u64),
    Finished(Result<(), String>),
}
pub struct Converter {
    previews: crate::video_preview::Previews,
    preview_time: f64,
    preview_change: (f64, Instant),
    source: Option<(PathBuf, f64)>,
    settings: Settings,
    pub busy: bool,
    progress: f32,
    message: String,
    error: Option<String>,
    saved: Option<(PathBuf, u64)>,
    cancel: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
}
impl Default for Converter {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            previews: crate::video_preview::Previews::default(),
            preview_time: 0.,
            preview_change: (0., Instant::now()),
            source: None,
            settings: Settings::default(),
            busy: false,
            progress: 0.,
            message: String::new(),
            error: None,
            saved: None,
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
        }
    }
}
impl Drop for Converter {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Converter {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    fn job(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(&mpsc::Sender<Event>, &AtomicBool) -> Result<(), String> + Send + 'static,
    ) {
        self.busy = true;
        self.error = None;
        self.progress = 0.;
        self.cancel = Arc::new(AtomicBool::new(false));
        let (tx, cancel, ctx) = (self.tx.clone(), self.cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = work(&tx, &cancel);
            let _ = tx.send(Event::Finished(result));
            ctx.request_repaint();
        });
    }
    pub fn select_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.busy {
            return;
        }
        self.message = "Reading video…".into();
        self.job(ctx, move |tx, cancel| load_source(tx, cancel, path));
    }
    pub fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Dismissed => self.message = "Ready when you are".into(),
                Event::Source(path, duration) => {
                    self.previews.clear();
                    self.preview_time = 0.;
                    self.preview_change = (0., Instant::now());
                    self.source = Some((path, duration));
                    self.settings.start = 0.;
                    self.settings.length = duration.min(5.);
                    self.saved = None;
                    self.message = "Ready to convert".into();
                }
                Event::Progress(p) => {
                    self.progress = p;
                    self.message = if p < 0.35 {
                        "Pass 1 of 2 · Building color palette"
                    } else {
                        "Pass 2 of 2 · Encoding GIF"
                    }
                    .into();
                }
                Event::Saved(path, size) => {
                    self.saved = Some((path, size));
                    self.progress = 1.;
                    self.message = "Your GIF is ready".into();
                }
                Event::Finished(result) => {
                    self.busy = false;
                    if self.cancel.load(Ordering::Relaxed) {
                        self.message = "Cancelled".into();
                    } else if let Err(error) = result {
                        self.error = Some(error);
                        self.message = "Could not complete conversion".into();
                    }
                }
            }
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        crate::page_header(
            ui,
            "GIF converter",
            "Turn a favorite moment into a GIF. Made locally, on your device.",
        );
        crate::card(ui, |ui| {
            ui.label(
                RichText::new("Source video")
                    .size(14.)
                    .color(crate::INK)
                    .strong(),
            );
            if let Some((path, duration)) = &self.source {
                let width = ui.available_width().min(560.);
                self.previews.show(
                    ui,
                    path,
                    Some(self.preview_time),
                    egui::vec2(width, width * 9. / 16.),
                );
                ui.add_enabled_ui(!self.busy, |ui| {
                    ui.add(
                        egui::Slider::new(
                            &mut self.settings.start,
                            0.0..=(*duration - self.settings.length).max(0.),
                        )
                        .text("Clip start")
                        .suffix(" s")
                        .fixed_decimals(1),
                    );
                });
                if self.preview_change.0 != self.settings.start {
                    self.preview_change = (self.settings.start, Instant::now());
                }
                if self.preview_time != self.settings.start {
                    if self.preview_change.1.elapsed() >= Duration::from_millis(180) {
                        self.preview_time = self.settings.start;
                    }
                    ui.ctx().request_repaint_after(Duration::from_millis(180));
                }
                ui.small(format!(
                    "Frame at {:.1} s · Move Clip start to preview your moment",
                    self.preview_time
                ));
                if ui.button("Play source video").clicked() {
                    self.error = crate::video_library::open_target(path.as_os_str()).err();
                }
                ui.label(
                    RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).strong(),
                );
                ui.small(format!("{duration:.2} seconds · {}", path.display()));
            } else {
                ui.label("Choose a downloaded clip or any local video.");
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Choose video…"))
                .clicked()
            {
                self.message = "Choose a video…".into();
                self.job(ui.ctx(), |tx, cancel| {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(
                            "Videos",
                            &["mp4", "mov", "mkv", "webm", "avi", "m4v", "gif"],
                        )
                        .pick_file()
                    {
                        load_source(tx, cancel, path)?;
                    } else {
                        let _ = tx.send(Event::Dismissed);
                    }
                    Ok(())
                });
            }
        });
        ui.add_space(16.);
        crate::card(ui, |ui| {
            ui.label(
                RichText::new("Clip & quality")
                    .size(14.)
                    .color(crate::INK)
                    .strong(),
            );
            ui.add_enabled_ui(!self.busy, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (label, width, fps, colors, smooth) in [
                        ("Small file", 320, 10, 64, false),
                        ("Balanced", 480, 15, 128, false),
                        ("High quality", 720, 24, 256, true),
                    ] {
                        let selected = self.settings.width == width && self.settings.fps == fps
                            && self.settings.colors == colors && self.settings.smooth == smooth;
                        if ui.add(egui::Button::new(label).selected(selected)).clicked() {
                            self.settings.width = width;
                            self.settings.fps = fps;
                            self.settings.colors = colors;
                            self.settings.smooth = smooth;
                        }
                    }
                });
                ui.separator();
                egui::Grid::new("gif_settings").num_columns(2).spacing([24., 14.]).show(ui, |ui| {
                    ui.label("Start (seconds)"); ui.add(egui::DragValue::new(&mut self.settings.start).speed(0.1).range(0.0..=self.source.as_ref().map_or(0., |s| s.1))); ui.end_row();
                    ui.label("Duration (seconds)"); ui.add(egui::DragValue::new(&mut self.settings.length).speed(0.1).range(0.1..=60.)); ui.end_row();
                });
                egui::CollapsingHeader::new("Advanced settings").show(ui, |ui| {
                  egui::Grid::new("gif_advanced").num_columns(2).spacing([24., 14.]).show(ui, |ui| {
                    ui.label("Maximum dimension"); ui.add(egui::Slider::new(&mut self.settings.width, 160..=960).suffix(" px")); ui.end_row();
                    ui.label("Frame rate"); ui.add(egui::Slider::new(&mut self.settings.fps, 5..=30).suffix(" fps")); ui.end_row();
                    ui.label("Palette colors"); ui.add(egui::Slider::new(&mut self.settings.colors, 32..=256)); ui.end_row();
                });
                ui.horizontal_wrapped(|ui| { ui.checkbox(&mut self.settings.repeat, "Loop forever"); ui.checkbox(&mut self.settings.smooth, "Smooth color dithering"); });
                });
                ui.small("Aspect ratio is preserved. Smaller dimensions, fewer frames and shorter clips produce smaller files. GIFs have no audio.");
            });
        });
        ui.add_space(16.);
        crate::card(ui, |ui| {
            ui.label(RichText::new("Export").size(14.).color(crate::INK).strong());
            let valid = self
                .source
                .as_ref()
                .map(|(_, duration)| self.settings.validate(*duration));
            if let Some(Err(error)) = &valid {
                ui.colored_label(crate::MUTED, error);
            }
            if crate::primary(ui, "Save GIF…", !self.busy && matches!(valid, Some(Ok(()))))
                .clicked()
            {
                let (source, duration) = self.source.clone().unwrap();
                let settings = self.settings.clone();
                self.saved = None;
                self.message = "Choose where to save your GIF…".into();
                self.job(ui.ctx(), move |tx, cancel| {
                    let name = format!(
                        "{}.gif",
                        source.file_stem().unwrap_or_default().to_string_lossy()
                    );
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name(name)
                        .add_filter("GIF image", &["gif"])
                        .save_file()
                    {
                        let path = path.with_extension("gif");
                        let size = convert(&source, &path, &settings, duration, cancel, tx)?;
                        let _ = tx.send(Event::Saved(path, size));
                    } else {
                        let _ = tx.send(Event::Dismissed);
                    }
                    Ok(())
                });
            }
            if self.busy {
                ui.add(
                    egui::ProgressBar::new(self.progress)
                        .show_percentage()
                        .animate(true),
                );
                if ui
                    .add_enabled(
                        !self.cancel.load(Ordering::Relaxed),
                        egui::Button::new("Cancel"),
                    )
                    .clicked()
                {
                    self.cancel();
                    self.message = "Cancelling…".into();
                }
            }
            ui.label(&self.message);
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(180, 45, 45), error);
            }
            if let Some((path, size)) = &self.saved {
                if ui.button("Preview GIF").clicked()
                    && let Err(error) = crate::video_library::open_target(path.as_os_str())
                {
                    self.error = Some(error);
                }
                ui.label(format!(
                    "{:.2} MB · {}",
                    *size as f64 / 1_000_000.,
                    path.display()
                ));
                if ui.button("Open output folder").clicked()
                    && let Some(folder) = path.parent()
                    && let Err(error) = crate::platform::open_folder(folder)
                {
                    self.error = Some(error.to_string());
                }
            }
        });
    }
}
fn load_source(tx: &mpsc::Sender<Event>, cancel: &AtomicBool, path: PathBuf) -> Result<(), String> {
    let video = crate::video_library::probe(&path, cancel)?;
    let duration = video
        .duration
        .filter(|d| *d >= 0.1)
        .ok_or("This video has no usable duration. Choose a different file.")?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    let _ = tx.send(Event::Source(path, duration));
    Ok(())
}
fn convert(
    source: &Path,
    destination: &Path,
    settings: &Settings,
    duration: f64,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<Event>,
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
        let mut command =
            Command::new(crate::platform::tools_dir().join(crate::platform::executable("ffmpeg")));
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
            let progress = if pass == 0 {
                fraction * 0.35
            } else {
                0.35 + fraction * 0.64
            };
            let _ = tx.send(Event::Progress(progress));
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
            process::Error::Start(e) => format!(
                "Cannot start FFmpeg: {e}. {}",
                crate::platform::tools_help()
            ),
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
        let exe = crate::platform::tools_dir().join(crate::platform::executable("ffmpeg"));
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
        let (tx, _) = mpsc::channel();
        let cancel = AtomicBool::new(false);
        let settings = Settings {
            length: 0.8,
            start: 0.1,
            ..Settings::default()
        };
        assert!(convert(&source, &destination, &settings, 1., &cancel, &tx).unwrap() > 0);
        let original = std::fs::read(&destination).unwrap();
        assert!(original.starts_with(b"GIF89a"));
        assert!(convert(&source, &destination, &settings, 1., &cancel, &tx).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), original);
        cancel.store(true, Ordering::Relaxed);
        let cancelled = dir.path().join("cancelled.gif");
        assert!(convert(&source, &cancelled, &settings, 1., &cancel, &tx).is_err());
        assert!(!cancelled.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
