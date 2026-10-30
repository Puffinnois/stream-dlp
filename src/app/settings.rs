//! The Settings page: updates, download tools, and the install confirmation.
use super::{
    App,
    theme::{BORDER, MUTED, card, page_header, primary},
};
use crate::{tools, update};
use eframe::egui::{self, Color32, RichText};

impl App {
    pub(super) fn settings(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Settings",
            "App updates, download tools, and version information.",
        );
        card(ui, |ui| {
            ui.label(RichText::new("Updates").size(18.).strong());
            ui.label(
                RichText::new(format!("Installed: {}", env!("CARGO_PKG_VERSION"))).color(MUTED),
            );
            if let Some(update) = &self.updater.available {
                ui.separator();
                ui.strong(format!("Version {} is available", update.version));
                if primary(ui, &format!("Update to {}…", update.version), true).clicked() {
                    self.show_update = true;
                }
            }
            if self.updater.downloading {
                download_progress(ui, &self.updater);
            } else if self.updater.checking {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&self.updater.message);
                });
            } else if !self.updater.message.is_empty() {
                ui.label(&self.updater.message);
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.updater.checking && !self.updater.downloading,
                        egui::Button::new("Check for updates"),
                    )
                    .clicked()
                {
                    self.updater.check(ui.ctx());
                }
                ui.hyperlink_to("Release notes", update::RELEASES);
            });
            let note = if !cfg!(windows) {
                "Mac updates are manual: download the new Mac package from its release page."
            } else if self.updater.silent() {
                "Updates download in the background and install when you choose Update & restart."
            } else {
                "This portable copy updates through setup, where you choose its folder. A fresh ZIP from Releases works too."
            };
            ui.label(RichText::new(note).size(12.).color(MUTED));
        });
        ui.add_space(16.);
        card(ui, |ui| {
            ui.label(RichText::new("Download tools").size(18.).strong());
            ui.label(if tools::ready() {
                "All download tools are ready."
            } else {
                "Download tools need setup."
            });
            if !self.tool_update.versions.is_empty() {
                ui.label(RichText::new(&self.tool_update.versions).color(MUTED));
            }
            ui.label(RichText::new("yt-dlp finds streams, FFmpeg combines video and audio, and Deno handles YouTube’s JavaScript challenges.").color(MUTED));
            if cfg!(windows) {
                self.tool_update_controls(ui);
            } else if !tools::ready() {
                ui.label(tools::help());
            }
            egui::CollapsingHeader::new("Tool details & setup").show(ui, |ui| {
                ui.label(format!("Location: {}", tools::dir().display()));
                ui.label(tools::help());
                ui.hyperlink_to("yt-dlp project", "https://github.com/yt-dlp/yt-dlp");
            });
        });
        ui.add_space(16.);
        card(ui, |ui| {
            ui.label(RichText::new("About stream-dlp").size(18.).strong());
            ui.label("A small native app for downloading videos and making GIFs.");
            ui.label(
                RichText::new(format!(
                    "Version {} · Built with Rust",
                    env!("CARGO_PKG_VERSION")
                ))
                .color(MUTED),
            );
        });
    }
    fn tool_update_controls(&mut self, ui: &mut egui::Ui) {
        if self.tool_update.active {
            ui.horizontal(|ui| {
                match self.tool_update.progress {
                    Some(share) => ui.add(egui::ProgressBar::new(share).desired_width(200.)),
                    None => ui.spinner(),
                };
                ui.label(&self.tool_update.status);
                if ui.button("Cancel").clicked() {
                    self.tool_update.cancel();
                }
            });
            return;
        }
        let label = if tools::ready() {
            "Update download tools"
        } else {
            "Install download tools"
        };
        let busy = self.busy();
        if primary(ui, label, !busy).clicked() {
            self.tool_update.start(ui.ctx());
        }
        if busy {
            ui.label("Finish or cancel the current operation first.");
        } else if !self.tool_update.status.is_empty() {
            ui.label(&self.tool_update.status);
        }
        ui.label(
            RichText::new("When a site stops working, a new yt-dlp usually fixes it. Only tools with a new release are downloaded, each checked against GitHub’s SHA-256 digest.")
                .size(12.)
                .color(MUTED),
        );
    }

    /// The update dialog: release notes, download progress, and Update & restart.
    pub(super) fn update_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_update {
            return;
        }
        let Some(update) = self.updater.available.clone() else {
            self.show_update = false;
            return;
        };
        let busy = self.busy();
        let (mut dismiss, mut close_app) = (false, false);
        let response = egui::Modal::new(egui::Id::new("update"))
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .corner_radius(16)
                    .inner_margin(28)
                    .stroke(egui::Stroke::new(1., BORDER)),
            )
            .show(ctx, |ui| {
                ui.set_width(460.);
                ui.label(
                    RichText::new(format!("Update to {}", update.version))
                        .size(23.)
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("Installed: {}", env!("CARGO_PKG_VERSION"))).color(MUTED),
                );
                ui.add_space(12.);
                egui::ScrollArea::vertical()
                    .max_height(260.)
                    .show(ui, |ui| release_notes(ui, &update.notes));
                ui.add_space(12.);
                if self.updater.downloading {
                    download_progress(ui, &self.updater);
                } else if self.updater.ready() {
                    ui.label(RichText::new("Downloaded and verified.").color(MUTED));
                } else if cfg!(windows) {
                    ui.label(&self.updater.message);
                }
                if !cfg!(windows) {
                    ui.hyperlink_to("Open the release page", &update.page);
                    ui.label("Download the Mac package there and replace this app with it.");
                } else if busy {
                    ui.label("Finish or cancel the current operation before installing.");
                } else if !self.updater.silent() {
                    ui.label(
                        RichText::new("Setup opens: choose this app’s folder to update it.")
                            .size(12.)
                            .color(MUTED),
                    );
                }
                ui.add_space(16.);
                ui.horizontal(|ui| {
                    if cfg!(windows) {
                        let ready = self.updater.ready();
                        if ready || self.updater.silent() {
                            let label = if self.updater.silent() {
                                "Update & restart"
                            } else {
                                "Open setup"
                            };
                            if primary(ui, label, ready && !busy).clicked() {
                                close_app = self.updater.install();
                            }
                        }
                        if !ready && !self.updater.downloading {
                            let enabled = !self.updater.checking;
                            let clicked = if self.updater.silent() {
                                ui.add_enabled(enabled, egui::Button::new("Retry download"))
                                    .clicked()
                            } else {
                                primary(ui, "Download setup", enabled).clicked()
                            };
                            if clicked {
                                self.updater.download(ctx);
                            }
                        }
                    }
                    if ui.button("Later").clicked() {
                        dismiss = true;
                    }
                });
            });
        if close_app {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if dismiss || close_app || response.should_close() {
            self.show_update = false;
        }
    }
}

fn download_progress(ui: &mut egui::Ui, updater: &update::Updater) {
    let (received, total) = updater.progress;
    let megabytes = |bytes: u64| bytes as f32 / 1_048_576.;
    ui.add(match total {
        Some(total) if total > 0 => {
            egui::ProgressBar::new(received as f32 / total as f32).text(format!(
                "Downloading… {:.1} of {:.1} MB",
                megabytes(received),
                megabytes(total)
            ))
        }
        _ => egui::ProgressBar::new(0.)
            .animate(true)
            .text(format!("Downloading… {:.1} MB", megabytes(received))),
    });
}

/// Release notes are Markdown; headings, bullets and bold cover what GitHub generates.
fn release_notes(ui: &mut egui::Ui, notes: &str) {
    if notes.is_empty() {
        ui.label(RichText::new("This release has no notes.").color(MUTED));
        return;
    }
    for line in notes.lines() {
        let line = line.trim_end().replace("**", "");
        let item = line.trim_start();
        if item.starts_with("<!--") {
            continue;
        } else if let Some(heading) = item.strip_prefix('#') {
            ui.add_space(4.);
            ui.label(RichText::new(heading.trim_start_matches('#').trim()).strong());
        } else if let Some(text) = item.strip_prefix("* ").or_else(|| item.strip_prefix("- ")) {
            ui.label(format!("•  {text}"));
        } else if item.is_empty() {
            ui.add_space(4.);
        } else {
            ui.label(item);
        }
    }
}
