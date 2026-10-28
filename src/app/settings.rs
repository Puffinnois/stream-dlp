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
        let busy = self.busy();
        card(ui, |ui| {
            ui.label(RichText::new("Updates").size(18.).strong());
            ui.label(
                RichText::new(format!(
                    "Installed: {} · {}",
                    env!("CARGO_PKG_VERSION"),
                    update::Channel::installed().label()
                ))
                .color(MUTED),
            );
            let mut channel = self.updater.channel;
            ui.add_enabled_ui(!busy, |ui| {
                egui::ComboBox::from_label("Update channel")
                    .selected_text(channel.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut channel,
                            update::Channel::Public,
                            "Public · stable releases",
                        );
                        ui.selectable_value(
                            &mut channel,
                            update::Channel::Dev,
                            "Dev · experimental prereleases",
                        );
                    });
            });
            if channel != self.updater.channel {
                self.updater.set_channel(channel, ui.ctx());
            }
            if self.updater.channel == update::Channel::Dev {
                ui.label(
                    RichText::new("Dev builds are experimental and may have unfinished features.")
                        .size(12.)
                        .color(MUTED),
                );
            }
            if self.updater.channel != update::Channel::installed() {
                ui.label("Switching channels can install an older version. Your videos stay in their folders.");
            }
            if let Some(update) = &self.updater.available {
                ui.separator();
                ui.strong(format!(
                    "{} {} is available",
                    self.updater.channel.label(),
                    update.version
                ));
                if cfg!(windows) {
                    if primary(
                        ui,
                        "Install selected build…",
                        !busy && !self.updater.checking,
                    )
                    .clicked()
                    {
                        self.confirm_install = true;
                    }
                } else {
                    ui.hyperlink_to("Download Mac package", update::RELEASES);
                    ui.label("Close the app and replace it with the downloaded Mac package.");
                }
                if busy && !self.updater.downloading {
                    ui.label("Finish or cancel the current operation before installing.");
                }
            }
            if self.updater.checking || self.updater.downloading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&self.updater.message);
                });
            } else if !self.updater.message.is_empty() {
                ui.label(&self.updater.message);
            }
            ui.horizontal_wrapped(|ui| {
                if cfg!(windows) {
                    if ui
                        .add_enabled(
                            !self.updater.checking && !self.updater.downloading,
                            egui::Button::new("Check for updates"),
                        )
                        .clicked()
                    {
                        self.updater.check(ui.ctx());
                    }
                } else {
                    ui.hyperlink_to("Get Mac updates", update::RELEASES);
                }
                ui.hyperlink_to("Release notes", update::RELEASES);
            });
            ui.label(
                RichText::new(
                    "Updates are optional. Nothing downloads until you confirm installation.",
                )
                .size(12.)
                .color(MUTED),
            );
        });
        ui.add_space(16.);
        card(ui, |ui| {
            ui.label(RichText::new("Download tools").size(18.).strong());
            ui.label(if tools::ready() {
                "All download tools are ready."
            } else {
                "Download tools need setup."
            });
            ui.label(RichText::new("yt-dlp finds streams, FFmpeg combines video and audio, and Deno handles YouTube’s JavaScript challenges.").color(MUTED));
            egui::CollapsingHeader::new("Tool details & setup").show(ui, |ui| {
                ui.label(format!("Location: {}", tools::dir().display()));
                ui.label(tools::help());
                ui.hyperlink_to("yt-dlp project", "https://github.com/yt-dlp/yt-dlp");
            });
            if !tools::ready() {
                ui.label(tools::help());
            }
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
    pub(super) fn install_confirmation(&mut self, ctx: &egui::Context) {
        if !self.confirm_install {
            return;
        }
        let mut dismiss = false;
        let response = egui::Modal::new(egui::Id::new("confirm_install"))
            .frame(egui::Frame::new().fill(Color32::WHITE).corner_radius(16).inner_margin(28)
                .stroke(egui::Stroke::new(1., BORDER)))
            .show(ctx, |ui| {
                ui.set_width(380.);
                ui.label(RichText::new("Install this update?").size(23.).strong());
                if let Some(update) = &self.updater.available {
                    ui.label(RichText::new(format!("{} {}", self.updater.channel.label(), update.version)).color(MUTED));
                }
                ui.add_space(8.);
                ui.label("The app will download and verify the installer, then close and open setup. You can cancel in setup.");
                ui.label(RichText::new("Portable installation: choose your existing app folder in setup.").size(12.).color(MUTED));
                ui.add_space(16.);
                ui.horizontal(|ui| {
                    if primary(ui, "Download & open setup", !self.busy() && !self.updater.checking && self.updater.available.is_some()).clicked() {
                        self.updater.download(ctx);
                        dismiss = true;
                    }
                    if ui.button("Cancel").clicked() { dismiss = true; }
                });
            });
        if dismiss || response.should_close() {
            self.confirm_install = false;
        }
    }
}
