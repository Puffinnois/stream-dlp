//! The GIF converter page. Workers own all temporary files.
use crate::media::gif::{self, Settings};
use eframe::egui::{self, RichText};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

enum Event {
    Dismissed,
    Source(PathBuf, f64),
    Progress(f32),
    Saved(PathBuf, u64),
    Finished(Result<(), String>),
}
pub struct Converter {
    previews: super::preview::Previews,
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
            previews: super::preview::Previews::default(),
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
        super::theme::page_header(
            ui,
            "GIF converter",
            "Turn a favorite moment into a GIF. Made locally, on your device.",
        );
        super::theme::card(ui, |ui| {
            ui.label(
                RichText::new("Source video")
                    .size(14.)
                    .color(super::theme::INK)
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
                    self.error = crate::platform::open_target(path.as_os_str()).err();
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
        super::theme::card(ui, |ui| {
            ui.label(
                RichText::new("Clip & quality")
                    .size(14.)
                    .color(super::theme::INK)
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
        super::theme::card(ui, |ui| {
            ui.label(
                RichText::new("Export")
                    .size(14.)
                    .color(super::theme::INK)
                    .strong(),
            );
            let valid = self
                .source
                .as_ref()
                .map(|(_, duration)| self.settings.validate(*duration));
            if let Some(Err(error)) = &valid {
                ui.colored_label(super::theme::MUTED, error);
            }
            if super::theme::primary(ui, "Save GIF…", !self.busy && matches!(valid, Some(Ok(()))))
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
                        let size = gif::convert(
                            &source,
                            &path,
                            &settings,
                            duration,
                            cancel,
                            |progress| {
                                let _ = tx.send(Event::Progress(progress));
                            },
                        )?;
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
                    && let Err(error) = crate::platform::open_target(path.as_os_str())
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
    let video = crate::media::local::probe(&path, cancel)?;
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
