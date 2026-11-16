//! Dev mode's experimental Chrome/Edge session import through the companion extension
//! (`extension/`, `session::chrome`). Normal users keep the guided export.
use super::{
    App, SessionImport,
    theme::{SUCCESS, caption, primary, semibold},
};
use crate::session::chrome::{self, bridge::Update, registry};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// The extension's folder in the source checkout this build came from. Release packages do
/// not ship it: the extension is a Dev mode experiment.
fn extension_folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("extension")
}

impl App {
    pub(super) fn poll_extension(&mut self) {
        let unix_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        for update in self.extension.poll(unix_now, Instant::now()) {
            match update {
                Update::Imported(cookies) => {
                    if self.extension_refresh.take().is_some() {
                        self.session_loading = false;
                    }
                    // A cancelled Firefox import must not discard this one.
                    self.import_cancel = Arc::new(AtomicBool::new(false));
                    self.finish_import(SessionImport {
                        path: None,
                        browser: None,
                        from_extension: true,
                        cookies,
                    });
                }
                Update::ClearSession => self.remove_extension_session(),
                Update::RefreshFailed => {
                    if let Some(error) = self.extension_refresh.take() {
                        self.session_loading = false;
                        self.error = error;
                    }
                }
            }
        }
    }

    /// After a sign-in error: asks the connection that supplied the session for it once more.
    pub(super) fn refresh_from_extension(&mut self) -> bool {
        if !self.extension.has_session() || !self.extension.request_refresh(Instant::now()) {
            return false;
        }
        self.refresh_used = true;
        self.session_loading = true;
        self.extension_refresh = Some(self.error.take());
        true
    }

    pub(super) fn remove_extension_session(&mut self) {
        self.sessions.remove(&crate::engine::Source::YouTube);
        self.session_files.remove(&crate::engine::Source::YouTube);
        self.retry_download = false;
        if self.extension_refresh.take().is_some() {
            self.session_loading = false;
        }
    }

    fn connect_extension(&mut self, ctx: &egui::Context) {
        let result = (|| {
            let folder = chrome::folder().ok_or("The app data folder is unknown.")?;
            let exe = std::env::current_exe()
                .map_err(|_| "The app's location is unknown, so it cannot be registered.")?;
            registry::register(&mut registry::UserRegistry, &folder, &exe)?;
            let ctx = ctx.clone();
            self.extension
                .start(&folder, Arc::new(move || ctx.request_repaint()))
        })();
        self.session_error = result.err();
        self.extension_registered = None;
    }

    fn disconnect_extension(&mut self) {
        if self.extension.stop() {
            self.remove_extension_session();
        }
        self.session_error = chrome::folder()
            .ok_or_else(|| "The app data folder is unknown.".to_owned())
            .and_then(|folder| registry::unregister(&mut registry::UserRegistry, &folder))
            .err();
        self.extension_registered = None;
    }

    pub(super) fn extension_controls(&mut self, ui: &mut egui::Ui) {
        if !cfg!(windows) {
            return;
        }
        let registered = self
            .extension_registered
            .get_or_insert_with(|| {
                chrome::folder()
                    .map(|folder| registry::registered(&registry::UserRegistry, &folder))
                    .unwrap_or_default()
            })
            .clone();
        ui.label(semibold("Companion extension · experimental (Dev mode)"));
        ui.label(caption("The stream-dlp extension sends only the YouTube cookies of the browser profile it is loaded in, when you press Connect in it. It is not in a store and is not proven to work with YouTube yet. Sessions stay in memory."));
        ui.add_enabled_ui(!self.download_busy(), |ui| {
            ui.horizontal(|ui| {
                if !self.extension.listening() {
                    if primary(ui, "Connect Chrome / Edge", true).clicked() {
                        self.connect_extension(ui.ctx());
                    }
                } else if ui.button("Disconnect extension").clicked() {
                    self.disconnect_extension();
                }
                if !registered.is_empty()
                    && !self.extension.listening()
                    && ui.button("Remove registration").clicked()
                {
                    self.disconnect_extension();
                }
            });
        });
        if self.extension.listening() {
            let folder = extension_folder();
            ui.label("1. Use a separate Chrome or Edge profile for testing. Open chrome://extensions (edge://extensions in Edge), turn on Developer mode, choose Load unpacked and select:");
            ui.horizontal(|ui| {
                ui.label(semibold(folder.display().to_string()));
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(folder.display().to_string());
                }
            });
            ui.label("2. Sign in to YouTube in that profile, open the stream-dlp toolbar button and press Connect.");
        }
        if self.extension.connected() {
            ui.colored_label(SUCCESS, "Extension connected");
        }
        if let Some(note) = &self.extension.note {
            ui.label(caption(note));
        }
        if !registered.is_empty() {
            let names: Vec<_> = registered.iter().map(|b| format!("{b:?}")).collect();
            ui.label(caption(format!(
                "Registered for this Windows user with {}. Disconnect removes it; uninstalling stream-dlp does not.",
                names.join(" and ")
            )));
        }
        ui.separator();
    }
}
