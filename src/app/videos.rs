//! The My videos page: a folder of local videos with previews, and Streamable sharing.
use crate::media::local::{self, EXTENSIONS, Entry, Video};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

enum ResultData {
    Folder(PathBuf, Vec<Entry>, bool),
    Video(Video),
    Cancelled,
}
type JobResult = (u64, Result<ResultData, String>);

pub struct Library {
    previews: super::preview::Previews,
    pub uploader: crate::streamable::Uploader,
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
            previews: super::preview::Previews::default(),
            uploader: crate::streamable::Uploader::default(),
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
            local::probe(&path, cancel).map(ResultData::Video)
        });
    }
    fn scan(&mut self, ctx: &egui::Context, folder: PathBuf) {
        self.job(ctx, move |cancel| {
            local::scan(&folder, cancel)
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
        super::theme::page_header(
            ui,
            "My videos",
            "Your local videos, ready to play or share.",
        );
        super::theme::card(ui, |ui| {
            ui.label(
                RichText::new("Video folder")
                    .size(14.)
                    .color(super::theme::INK)
                    .strong(),
            );
            ui.label("Choose a folder to see your videos here.");
            ui.horizontal_wrapped(|ui| {
                if super::theme::primary(ui, "Add video folder…", !busy).clicked() {
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
                        local::scan(&folder, cancel).map(|(entries, truncated)| {
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
                        local::probe(&path, cancel).map(ResultData::Video)
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
                        .color(super::theme::MUTED),
                );
                ui.small(
                    "Folder selection lasts for this app session. Subfolders are not scanned.",
                );
            }
        });
        ui.add_space(18.);
        super::theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Your videos")
                        .size(14.)
                        .color(super::theme::INK)
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("{} videos", self.entries.len()))
                        .size(12.)
                        .color(super::theme::MUTED),
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
                        .color(super::theme::MUTED),
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
                                    super::theme::BLUE
                                } else {
                                    super::theme::BORDER
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
            super::theme::card(ui, |ui| {
                ui.label(
                    RichText::new("Selected video")
                        .size(14.)
                        .color(super::theme::INK)
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
                    self.error = crate::platform::open_target(video.path.as_os_str()).err();
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
                        self.error = crate::platform::open_target(video.path.as_os_str()).err();
                    }
                    if ui.button("Show file").clicked() {
                        self.error = crate::platform::reveal(&video.path).err();
                    }
                });
                ui.separator();
                ui.strong("Upload on Streamable");
                ui.label("Automatic upload uses a separate private Edge or Chrome window. The app selects your video and returns the share link. Complete any sign-in or verification in that window.");
                ui.label(RichText::new("Uploading sends this video to Streamable. Anyone with its link may be able to watch it. Guest links are temporary.").size(12.).color(super::theme::MUTED));
                ui.horizontal_wrapped(|ui| {
                    ui.small("By choosing Upload automatically, you accept Streamable’s");
                    ui.hyperlink_to("Terms of Service", "https://streamable-support.zendesk.com/hc/en-us/articles/44623209368468-Terms-of-Service");
                });
                if super::theme::primary(ui, "Upload automatically (experimental)", !busy).clicked()
                {
                    self.uploader.start(video.path.clone(), ctx);
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Use website manually"))
                    .clicked()
                {
                    match crate::streamable::handoff(&video.path, crate::platform::reveal, || {
                        crate::platform::open_target(std::ffi::OsStr::new(
                            crate::streamable::STREAMABLE,
                        ))
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
            super::theme::card(ui, |ui| {
                ui.label(
                    RichText::new("Streamable upload")
                        .size(14.)
                        .color(super::theme::INK)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_jobs_cannot_replace_selection() {
        let mut library = Library::default();
        library.busy = true;
        library
            .tx
            .send((
                0,
                Ok(ResultData::Video(local::Video {
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
}
