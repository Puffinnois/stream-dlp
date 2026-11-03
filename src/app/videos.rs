//! The My videos page: a folder of local videos with previews, and Streamable sharing.
use super::theme::{
    ACCENT, ACCENT_TINT, DANGER, RADIUS_M, SPACE_L, SPACE_M, caption, card, page_header, primary,
    progress, section_title, semibold,
};
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
    /// Whether the page shows an error; public feedback sends only this, never its text.
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }
    /// What a dev note records about this page: summary parts and details.
    pub fn note_context(&self) -> (Vec<String>, serde_json::Value) {
        let file = self
            .selected
            .as_ref()
            .and_then(|video| video.path.file_name());
        let parts = file
            .map(|name| name.to_string_lossy().into_owned())
            .into_iter()
            .chain(self.uploader.status.active.then(|| "uploading".to_string()))
            .chain(self.error.as_ref().map(|_| "error".to_string()))
            .collect();
        let details = serde_json::json!({
            "folder": self.folder,
            "videos": self.entries.len(),
            "selected": self.selected.as_ref().map(|video| &video.path),
            "message": self.message,
            "error": self.error,
            "upload": self.uploader.status.active.then_some(&self.uploader.status.message),
        });
        (parts, details)
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
                Err(error) => {
                    crate::dev::errors::record("videos", &error);
                    self.error = Some(error);
                }
            }
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        let busy = self.busy || self.uploader.status.active;
        page_header(
            ui,
            "My videos",
            "Your local videos, ready to play or share.",
        );
        card(ui, |ui| {
            section_title(ui, "Video folder");
            ui.label("Choose a folder to see your videos here.");
            ui.horizontal_wrapped(|ui| {
                if primary(ui, "Add video folder…", !busy).clicked() {
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
                ui.label(caption(folder.display().to_string()));
                ui.label(caption(
                    "Folder selection lasts for this app session. Subfolders are not scanned.",
                ));
            }
        });
        ui.add_space(SPACE_L);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, "Your videos");
                ui.label(caption(video_count(self.entries.len())));
            });
            if self.entries.is_empty() {
                ui.add_space(SPACE_M);
                ui.label(if self.folder.is_some() {
                    "No videos found in this folder."
                } else {
                    "Your videos will appear here."
                });
                ui.label(caption(
                    "Add a video folder or choose an existing video to get started.",
                ));
                ui.add_space(SPACE_M);
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
                            let mut button = egui::Button::new(format!(
                                "{name}  ·  {:.1} MB",
                                entry.bytes as f64 / 1_000_000.
                            ))
                            .corner_radius(RADIUS_M)
                            .min_size(egui::vec2(ui.available_width(), 76.5))
                            .truncate();
                            // Only the selected row sets a fill: one would also hide hovering.
                            if selected {
                                button = button
                                    .fill(ACCENT_TINT)
                                    .stroke(egui::Stroke::new(1., ACCENT));
                            }
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
            ui.add_space(SPACE_L);
            card(ui, |ui| {
                section_title(ui, "Selected video");
                ui.label(semibold(
                    video.path.file_name().unwrap_or_default().to_string_lossy(),
                ));
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
                    video.container_label(),
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
                section_title(ui, "Upload on Streamable");
                ui.label("Automatic upload uses a separate private Edge or Chrome window. The app selects your video and returns the share link. Complete any sign-in or verification in that window.");
                ui.label(caption("Uploading sends this video to Streamable. Anyone with its link may be able to watch it. Guest links are temporary."));
                ui.horizontal_wrapped(|ui| {
                    ui.label(caption("By choosing Upload automatically, you accept Streamable’s"));
                    ui.hyperlink_to("Terms of Service", "https://streamable-support.zendesk.com/hc/en-us/articles/44623209368468-Terms-of-Service");
                });
                if primary(ui, "Upload automatically (experimental)", !busy).clicked() {
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
            ui.add_space(SPACE_L);
            card(ui, |ui| {
                section_title(ui, "Streamable upload");
                let status = &self.uploader.status;
                if status.failed {
                    ui.colored_label(DANGER, &status.message);
                } else {
                    ui.label(&status.message);
                }
                if status.active {
                    progress(ui, "Uploading", status.percent);
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
        ui.add_space(SPACE_M);
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if let Some(error) = &self.error {
            ui.colored_label(DANGER, error);
        }
    }
    fn cancel_job(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.generation += 1;
        self.busy = false;
    }
}

fn video_count(count: usize) -> String {
    match count {
        1 => "1 video".into(),
        count => format!("{count} videos"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_video_count_reads_correctly() {
        assert_eq!(video_count(0), "0 videos");
        assert_eq!(video_count(1), "1 video");
        assert_eq!(video_count(2), "2 videos");
    }
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
