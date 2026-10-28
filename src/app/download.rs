//! The Download page: find a video, choose a format and folder, and follow the download.
use super::{
    App, Page, Phase,
    theme::{BLUE, BORDER, INK, MUTED, card, icon_tile, page_header, primary},
};
use crate::{engine, platform, tools};
use eframe::egui::{self, Color32, RichText, Vec2};
use engine::Event;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Which formats the list shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FormatFilter {
    All,
    Video,
    Audio,
}

impl FormatFilter {
    fn shows(self, format: &engine::Format) -> bool {
        match self {
            Self::All => true,
            Self::Video => format.video(),
            Self::Audio => !format.video(),
        }
    }
}

impl App {
    pub(super) fn inspect(&mut self) {
        match engine::parse_link(&self.link) {
            Err(error) => self.error = Some(error.user_message()),
            Ok(link) => {
                self.refresh_used = false;
                self.retry_download = false;
                self.loaded_link = Some(link.clone());
                self.error = None;
                self.video = None;
                self.saved = None;
                // The last video's "ready" or "cancelled" line must not label the next one.
                self.status.clear();
                self.progress = 0.;
                self.log.clear();
                self.phase = Phase::Inspecting;
                self.download_cancel = Arc::new(AtomicBool::new(false));
                let (tx, cancel) = (self.tx.clone(), self.download_cancel.clone());
                let session = self.session.clone();
                std::thread::spawn(move || engine::inspect(link, session, tx, cancel));
            }
        }
    }
    pub(super) fn poll_download(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Loaded(video) => {
                    self.video = Some(video);
                    self.selected = 0;
                    self.filter = FormatFilter::All;
                    self.phase = Phase::Choosing;
                }
                Event::Line(line) => {
                    if let Some((fraction, status)) = engine::progress(&line) {
                        self.progress = fraction;
                        self.status = status;
                    } else if line.contains("[Merger]") || line.contains("[Fixup") {
                        self.status = "Finishing your file · combining video and audio…".into();
                    }
                    if self.log.len() >= 40 {
                        self.log.pop_front();
                    }
                    self.log.push_back(line);
                }
                Event::Complete(path) => {
                    self.saved = Some(path);
                    self.progress = 1.;
                    self.phase = Phase::Done;
                    self.status = "Your download is ready.".into();
                }
                Event::Failed(error) => {
                    let needs_session = error.needs_session();
                    self.retry_download = self.phase == Phase::Downloading;
                    self.error = Some(error.user_message());
                    self.phase = if self.video.is_some() {
                        Phase::Choosing
                    } else {
                        Phase::Idle
                    };
                    if needs_session
                        && self.auto_refresh
                        && !self.refresh_used
                        && !self.closing
                        && let Some(profile) = self.browser_source.clone()
                    {
                        self.import_browser(profile, true);
                        self.error = None;
                    }
                }
                Event::Cancelled => {
                    if self.phase == Phase::Downloading {
                        self.status =
                            "Cancelled. Partial downloads can be resumed by choosing the same folder."
                                .into();
                    } else {
                        self.status.clear();
                    }
                    self.phase = if self.video.is_some() {
                        Phase::Choosing
                    } else {
                        Phase::Idle
                    };
                }
            }
        }
        if let Ok(folder) = self.folder_rx.try_recv() {
            if let Some(folder) = folder {
                self.start_download(folder);
            } else {
                self.phase = Phase::Choosing;
            }
        }
    }
    pub(super) fn start_download(&mut self, folder: PathBuf) {
        self.refresh_used = false;
        self.destination = Some(folder.clone());
        self.error = None;
        self.progress = 0.;
        self.status = format!(
            "Connecting to {}…",
            self.video.as_ref().unwrap().source.label()
        );
        self.log.clear();
        self.phase = Phase::Downloading;
        self.download_cancel = Arc::new(AtomicBool::new(false));
        let (media, format, tx, cancel) = (
            self.video.as_ref().unwrap().identity(),
            self.video.as_ref().unwrap().formats[self.selected].clone(),
            self.tx.clone(),
            self.download_cancel.clone(),
        );
        let session = self.session.clone();
        std::thread::spawn(move || engine::download(media, format, folder, session, tx, cancel));
    }
    /// Switches the format list; a hidden selection moves to the first shown format so the
    /// download always matches a visible row.
    pub(super) fn set_filter(&mut self, filter: FormatFilter) {
        self.filter = filter;
        if let Some(video) = &self.video
            && !video
                .formats
                .get(self.selected)
                .is_some_and(|f| filter.shows(f))
            && let Some(first) = video.formats.iter().position(|f| filter.shows(f))
        {
            self.selected = first;
        }
    }
    fn choose_folder(&mut self) {
        self.phase = Phase::Picking;
        let tx = self.folder_tx.clone();
        let folder = self.destination.clone();
        std::thread::spawn(move || {
            let mut dialog =
                rfd::FileDialog::new().set_title("Where should we save your download?");
            if let Some(folder) = folder {
                dialog = dialog.set_directory(folder);
            }
            let _ = tx.send(dialog.pick_folder());
        });
    }
    pub(super) fn content(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Download",
            "Save a YouTube or TikTok video in the quality you choose.",
        );
        card(ui, |ui| {
            ui.label(RichText::new("Video link").size(14.).color(INK).strong());
            ui.add_space(2.);
            ui.horizontal(|ui| {
                let busy = self.download_busy();
                let width = (ui.available_width() - 156.).max(100.);
                let response = ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.link)
                        .hint_text("Paste a YouTube or TikTok video/share link")
                        .desired_width(width)
                        .margin(Vec2::new(12., 12.)),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if primary(
                    ui,
                    "Find video",
                    !busy && !self.link.trim().is_empty() && tools::ready(),
                )
                .clicked()
                    || (enter && !busy && tools::ready())
                {
                    self.inspect();
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(
                        "YouTube videos and Shorts · TikTok public video links (experimental)",
                    )
                    .size(12.)
                    .color(MUTED),
                );
                if ui
                    .add_enabled(
                        !self.download_busy(),
                        egui::Button::new(if self.session.is_some() {
                            "YouTube session loaded · Manage"
                        } else {
                            "Connect YouTube browser…"
                        })
                        .frame(false),
                    )
                    .clicked()
                {
                    self.session_expanded = true;
                }
            });
        });
        self.session_panel(ui);
        if !tools::ready() {
            ui.add_space(12.);
            ui.colored_label(
                Color32::from_rgb(174, 100, 20),
                format!("Download tools are missing. {}", tools::help()),
            );
        }
        if self.browser_importing {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Importing your YouTube session from Firefox…");
                    if ui.button("Cancel import").clicked() {
                        self.import_cancel.store(true, Ordering::Relaxed);
                    }
                });
            });
        }
        if let Some(error) = self.error.clone() {
            ui.add_space(12.);
            egui::Frame::new()
                .fill(Color32::from_rgb(255, 240, 240))
                .corner_radius(8)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.colored_label(Color32::from_rgb(175, 46, 57), error);
                    if self
                        .loaded_link
                        .as_ref()
                        .is_some_and(|link| link.source == engine::Source::YouTube)
                        && ui
                            .add_enabled(
                                !self.download_busy(),
                                egui::Button::new("Connect browser / retry help"),
                            )
                            .clicked()
                    {
                        self.session_expanded = true;
                    }
                });
        }
        ui.add_space(18.);
        if self.phase == Phase::Inspecting {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Finding your video and available formats…");
                    if ui.small_button("Cancel").clicked() {
                        self.download_cancel.store(true, Ordering::Relaxed);
                    }
                });
            });
        } else if self.video.is_some() {
            self.video_card(ui);
        } else {
            ui.add_space(36.);
            ui.vertical_centered(|ui| {
                icon_tile(ui, 72., Color32::from_rgb(239, 238, 255), BLUE);
                ui.add_space(14.);
                ui.label(RichText::new("A link is all you need").size(22.).strong());
                ui.label(
                    RichText::new("Paste a video or Shorts link above to get started.")
                        .color(MUTED),
                );
                ui.add_space(24.);
                ui.label(
                    RichText::new("Add a link   /   Pick a quality   /   Save to your folder")
                        .size(12.)
                        .color(MUTED),
                );
            });
        }
    }
    fn video_card(&mut self, ui: &mut egui::Ui) {
        let video = self.video.as_ref().unwrap().clone();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(239, 238, 255))
                    .corner_radius(12)
                    .inner_margin(18)
                    .show(ui, |ui| {
                        ui.label(RichText::new("▶").size(27.).color(BLUE));
                    });
                ui.vertical(|ui| {
                    ui.label(RichText::new(&video.title).size(18.).strong());
                    ui.label(
                        RichText::new(format!(
                            "{}   ·   {}   ·   {}",
                            video.creator_label(),
                            video.duration_label(),
                            video.source.label()
                        ))
                        .size(12.)
                        .color(MUTED),
                    );
                });
            });
            ui.add_space(14.);
            ui.separator();
            ui.add_space(10.);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Quality & format")
                        .size(14.)
                        .color(MUTED)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(!self.download_busy(), |ui| {
                        for (filter, name) in [
                            (FormatFilter::Audio, "Audio"),
                            (FormatFilter::Video, "Video"),
                            (FormatFilter::All, "All"),
                        ] {
                            if ui.selectable_label(self.filter == filter, name).clicked() {
                                self.set_filter(filter);
                            }
                        }
                    });
                });
            });
            ui.label(
                RichText::new(if video.source == engine::Source::TikTok {
                    "Experimental TikTok support · these video formats include audio."
                } else {
                    "Available streams · video-only choices are combined with matching audio."
                })
                .size(11.)
                .color(MUTED),
            );
            ui.add_space(4.);
            egui::ScrollArea::vertical()
                .id_salt("formats")
                .max_height(244.)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_enabled_ui(!self.download_busy(), |ui| {
                        for (index, format) in video.formats.iter().enumerate() {
                            if !self.filter.shows(format) {
                                continue;
                            }
                            let selected = self.selected == index;
                            let mut picked = false;
                            let row = egui::Frame::new()
                                .fill(if selected {
                                    Color32::from_rgb(239, 238, 255)
                                } else {
                                    Color32::WHITE
                                })
                                .stroke(egui::Stroke::new(1., if selected { BLUE } else { BORDER }))
                                .corner_radius(8)
                                .inner_margin(10)
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        picked |= ui.radio(selected, "").clicked();
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}   {}",
                                                    format.quality(),
                                                    format.container_label()
                                                ))
                                                .strong(),
                                            );
                                            ui.label(
                                                RichText::new(format.details())
                                                    .size(11.)
                                                    .color(MUTED),
                                            );
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(format.size())
                                                        .size(12.)
                                                        .color(MUTED),
                                                );
                                            },
                                        );
                                    });
                                });
                            if picked
                                || ui
                                    .interact(
                                        row.response.rect,
                                        ui.id().with(index),
                                        egui::Sense::click(),
                                    )
                                    .clicked()
                            {
                                self.selected = index;
                            }
                            ui.add_space(2.);
                        }
                    });
                });
            ui.add_space(8.);
            let selected = &video.formats[self.selected];
            ui.label(
                RichText::new(format!(
                    "Selected: {} · {}{}",
                    selected.quality(),
                    selected.container_label(),
                    if selected.video() && !selected.audio() {
                        " + audio (MKV if needed)"
                    } else {
                        ""
                    }
                ))
                .size(12.)
                .color(MUTED),
            );
            ui.add_space(8.);
            match self.phase {
                Phase::Downloading => {
                    ui.label(
                        RichText::new("Saving your video")
                            .size(15.)
                            .color(INK)
                            .strong(),
                    );
                    ui.add(
                        egui::ProgressBar::new(self.progress)
                            .desired_width(ui.available_width())
                            .desired_height(32.)
                            .corner_radius(8)
                            .fill(BLUE)
                            .text(
                                RichText::new(format!("{:.0}%", self.progress * 100.))
                                    .size(18.)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .animate(true),
                    );
                    ui.horizontal(|ui| {
                        ui.label(&self.status);
                        if ui.small_button("Cancel").clicked() {
                            self.download_cancel.store(true, Ordering::Relaxed);
                            self.status = "Cancelling…".into();
                        }
                    });
                    ui.label(
                        RichText::new(
                            "Progress is for the current stream; audio and merging follow.",
                        )
                        .size(11.)
                        .color(MUTED),
                    );
                }
                Phase::Done => {
                    ui.colored_label(
                        Color32::from_rgb(31, 139, 99),
                        RichText::new("✓   Download complete").strong(),
                    );
                    if let Some(saved) = &self.saved {
                        ui.label(format!("Saved from {}", saved.media.source.label()));
                        ui.label(
                            RichText::new(saved.path.display().to_string())
                                .size(12.)
                                .color(MUTED),
                        );
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(
                                self.saved.is_some() && !self.gif.busy,
                                egui::Button::new("Make GIF"),
                            )
                            .clicked()
                            && let Some(path) = self.saved.as_ref().map(|saved| saved.path.clone())
                        {
                            self.gif.select_file(ui.ctx(), path);
                            self.page = Page::Gif;
                        }
                        if primary(ui, "Open folder", true).clicked()
                            && let Some(folder) = &self.destination
                        {
                            let _ = platform::open_folder(folder);
                        }
                        if ui.button("Download again").clicked() {
                            self.choose_folder();
                        }
                        if ui
                            .add_enabled(
                                self.saved.is_some(),
                                egui::Button::new("Upload on Streamable"),
                            )
                            .clicked()
                            && let Some(path) = self.saved.as_ref().map(|saved| saved.path.clone())
                        {
                            self.library.select_file(ui.ctx(), path);
                            self.page = Page::Videos;
                        }
                    });
                }
                Phase::Picking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Choose your destination folder…");
                    });
                }
                _ => {
                    ui.horizontal(|ui| {
                        if primary(ui, "Choose folder & download", true).clicked() {
                            self.choose_folder();
                        }
                        ui.label(
                            RichText::new("You choose where it goes.")
                                .size(12.)
                                .color(MUTED),
                        );
                    });
                    if !self.status.is_empty() {
                        ui.label(RichText::new(&self.status).size(12.).color(MUTED));
                    }
                }
            }
            if let Some(folder) = &self.destination
                && self.phase != Phase::Done
            {
                ui.label(
                    RichText::new(format!("Save to: {}", folder.display()))
                        .size(11.)
                        .color(MUTED),
                );
            }
        });
        if !self.log.is_empty() {
            egui::CollapsingHeader::new("Download details").show(ui, |ui| {
                for line in &self.log {
                    ui.label(RichText::new(line).monospace().size(11.).color(MUTED));
                }
            });
        }
    }
}
