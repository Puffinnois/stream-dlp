//! The Download page: find a video, choose a format and folder, and follow the download.
use super::{
    App, Page, Phase,
    theme::{
        ACCENT, ACCENT_TINT, BORDER, DANGER, HOVER_BORDER, HOVER_FILL, MUTED, RADIUS_M, SPACE_L,
        SPACE_M, SPACE_S, SPACE_XL, SPACE_XS, TEXT_CAPTION, TEXT_HEADING, TEXT_TITLE, WARNING,
        caption, card, clamped_title, error_banner, focus_ring, icon_tile, page_header, primary,
        progress, radio_mark, section_title, semibold, success_line,
    },
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
            Err(error) => {
                self.error_kind = Some((&error).into());
                self.error = Some(error.user_message());
            }
            Ok(link) => {
                self.refresh_used = false;
                self.retry_download = false;
                self.loaded_link = Some(link.clone());
                self.error = None;
                self.video = None;
                self.entries.clear();
                self.saved = None;
                // The last video's "ready" or "cancelled" line must not label the next one.
                self.status.clear();
                self.progress = 0.;
                self.log.clear();
                self.phase = Phase::Inspecting;
                self.download_cancel = Arc::new(AtomicBool::new(false));
                let (tx, cancel) = (self.tx.clone(), self.download_cancel.clone());
                let session = self.sessions.get(&link.source).cloned();
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
                Event::Entries(entries) => {
                    self.entries = entries;
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
                    crate::dev::errors::record("download", format!("{error:?}"));
                    self.error_kind = Some((&error).into());
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
        let session = self.sessions.get(&media.source).cloned();
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
            "Save a YouTube, TikTok, X, Facebook or Instagram video in the quality you choose.",
        );
        card(ui, |ui| {
            section_title(ui, "Video link");
            ui.add_space(SPACE_XS);
            ui.horizontal(|ui| {
                let busy = self.download_busy();
                // `desired_width` excludes the field's margin; the rest is the button and gap.
                let width = (ui.available_width() - 130.).max(100.);
                let response = ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.link)
                        .hint_text(
                            "Paste a YouTube, TikTok, X, Facebook or Instagram video/share link",
                        )
                        .desired_width(width)
                        .margin(Vec2::new(SPACE_M, 9.)),
                );
                focus_ring(ui, &response);
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
                ui.label(caption(
                    "YouTube videos and Shorts · Public TikTok, X, Facebook and Instagram video links (experimental)",
                ));
                if ui
                    .add_enabled(
                        !self.download_busy(),
                        egui::Link::new(if self.sessions.contains_key(&self.session_source()) {
                            format!("{} session loaded · Manage", self.session_source().label())
                        } else {
                            format!("Connect {} session…", self.session_source().label())
                        }),
                    )
                    .clicked()
                {
                    self.session_expanded = true;
                }
            });
        });
        self.session_panel(ui);
        if !tools::ready() {
            ui.add_space(SPACE_M);
            ui.colored_label(
                WARNING,
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
            ui.add_space(SPACE_M);
            error_banner(ui, |ui| {
                ui.colored_label(DANGER, error);
                if self
                    .loaded_link
                    .as_ref()
                    .is_some_and(|link| engine::parse_link(&self.link).as_ref() == Ok(link))
                    && ui
                        .add_enabled(
                            !self.download_busy(),
                            egui::Button::new("Connect session / retry help"),
                        )
                        .clicked()
                {
                    self.session_expanded = true;
                }
            });
        }
        ui.add_space(SPACE_L);
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
        } else if !self.entries.is_empty() && self.video.is_none() {
            card(ui, |ui| {
                section_title(ui, "Choose one video from this post");
                for video in &self.entries {
                    if ui
                        .add(
                            egui::Button::new(format!(
                                "Video {} · {} · {}",
                                video.entry_index.unwrap_or(1),
                                video.title,
                                video.duration_label()
                            ))
                            .truncate(),
                        )
                        .on_hover_text(&video.title)
                        .clicked()
                    {
                        self.video = Some(video.clone());
                        self.selected = 0;
                        self.filter = FormatFilter::All;
                    }
                }
            });
        } else if self.video.is_some() {
            if !self.entries.is_empty()
                && !self.download_busy()
                && ui.button("Choose another video from this post").clicked()
            {
                self.video = None;
                self.phase = Phase::Choosing;
                self.saved = None;
                self.error = None;
                self.status.clear();
                return;
            }
            self.video_card(ui);
        } else if self.show_empty_state() {
            ui.add_space(SPACE_XL);
            ui.vertical_centered(|ui| {
                icon_tile(ui, 72., ACCENT_TINT, ACCENT);
                ui.add_space(SPACE_M);
                ui.label(semibold("A link is all you need").size(TEXT_HEADING));
                ui.label(
                    RichText::new("Paste a video or Shorts link above to get started.")
                        .color(MUTED),
                );
                ui.add_space(SPACE_XL);
                ui.label(caption(
                    "Add a link   /   Pick a quality   /   Save to your folder",
                ));
            });
        }
    }
    /// The welcome only makes sense while nothing else is on the page.
    pub(super) fn show_empty_state(&self) -> bool {
        self.error.is_none() && !self.session_expanded
    }
    fn video_card(&mut self, ui: &mut egui::Ui) {
        let video = self.video.as_ref().unwrap().clone();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                egui::Frame::new()
                    .fill(ACCENT_TINT)
                    .corner_radius(RADIUS_M)
                    .inner_margin(18)
                    .show(ui, |ui| {
                        ui.label(RichText::new("▶").size(TEXT_TITLE).color(ACCENT));
                    });
                ui.vertical(|ui| {
                    clamped_title(ui, &video.title);
                    ui.label(caption(format!(
                        "{}   ·   {}   ·   {}",
                        video.creator_label(),
                        video.duration_label(),
                        video.source.label()
                    )));
                });
            });
            ui.add_space(SPACE_M);
            ui.separator();
            ui.add_space(SPACE_M);
            ui.horizontal(|ui| {
                section_title(ui, "Quality & format");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(!self.download_busy(), |ui| {
                        for (filter, name) in [
                            (FormatFilter::Audio, "Audio"),
                            (FormatFilter::Video, "Video"),
                            (FormatFilter::All, "All"),
                        ] {
                            if video.source == engine::Source::X && filter == FormatFilter::Audio {
                                continue;
                            }
                            if ui.selectable_label(self.filter == filter, name).clicked() {
                                self.set_filter(filter);
                            }
                        }
                    });
                });
            });
            ui.label(caption(if video.source == engine::Source::X {
                "Direct MP4 · saves the original audio when present."
            } else if video.source == engine::Source::TikTok {
                "Experimental TikTok support · these video formats include audio."
            } else {
                "Available streams · video-only choices are combined with matching audio."
            }));
            ui.add_space(SPACE_XS);
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
                            // The row's click area comes after its contents, so its state
                            // for this frame's colors is last frame's.
                            let id = ui.id().with(index);
                            let previous = ui.ctx().read_response(id);
                            let lit = previous
                                .as_ref()
                                .is_some_and(|row| row.hovered() || row.has_focus());
                            let row = egui::Frame::new()
                                .fill(if selected {
                                    ACCENT_TINT
                                } else if lit {
                                    HOVER_FILL
                                } else {
                                    Color32::WHITE
                                })
                                .stroke(egui::Stroke::new(
                                    1.,
                                    if selected {
                                        ACCENT
                                    } else if lit {
                                        HOVER_BORDER
                                    } else {
                                        BORDER
                                    },
                                ))
                                .corner_radius(RADIUS_M)
                                .inner_margin(egui::Margin::symmetric(SPACE_M as i8, SPACE_S as i8))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        radio_mark(ui, selected, previous.as_ref());
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = SPACE_XS;
                                            ui.label(semibold(format!(
                                                "{}   {}",
                                                format.quality(),
                                                format.container_label()
                                            )));
                                            ui.label(caption(format.details()));
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(caption(format.size()));
                                            },
                                        );
                                    });
                                });
                            if ui
                                .interact(row.response.rect, id, egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.selected = index;
                            }
                            ui.add_space(SPACE_XS);
                        }
                    });
                });
            ui.add_space(SPACE_S);
            let selected = &video.formats[self.selected];
            ui.label(caption(format!(
                "Selected: {} · {}{}",
                selected.quality(),
                selected.container_label(),
                if selected.video() && !selected.direct_video && !selected.audio() {
                    " + audio (MKV if needed)"
                } else {
                    ""
                }
            )));
            ui.add_space(SPACE_S);
            match self.phase {
                Phase::Downloading => {
                    progress(ui, "Saving your video", Some(self.progress));
                    ui.horizontal(|ui| {
                        ui.label(&self.status);
                        if ui.small_button("Cancel").clicked() {
                            self.download_cancel.store(true, Ordering::Relaxed);
                            self.status = "Cancelling…".into();
                        }
                    });
                    ui.label(caption(if video.source == engine::Source::X {
                        "Downloading the selected MP4."
                    } else {
                        "Progress is for the current stream; audio and merging follow."
                    }));
                }
                Phase::Done => {
                    success_line(ui, "Download complete");
                    if let Some(saved) = &self.saved {
                        ui.label(format!("Saved from {}", saved.media.source.label()));
                        ui.label(caption(saved.path.display().to_string()));
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
                        ui.label(caption("You choose where it goes."));
                    });
                    if !self.status.is_empty() {
                        ui.label(caption(&self.status));
                    }
                }
            }
            if let Some(folder) = &self.destination
                && self.phase != Phase::Done
            {
                ui.label(caption(format!("Save to: {}", folder.display())));
            }
        });
        if !self.log.is_empty() {
            egui::CollapsingHeader::new("Download details").show(ui, |ui| {
                for line in &self.log {
                    ui.label(
                        RichText::new(line)
                            .monospace()
                            .size(TEXT_CAPTION)
                            .color(MUTED),
                    );
                }
            });
        }
    }
}
