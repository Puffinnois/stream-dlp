//! The YouTube connection panel: Firefox import, cookie files, and the export guide.
use super::{
    App, Phase, SessionImport,
    theme::{MUTED, card, primary},
};
use crate::{engine, platform, session, tools};
use eframe::egui::{self, Color32, RichText};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// What the YouTube connection panel shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionMode {
    Browser,
    CookieFile,
    ExportGuide,
}

/// The browser to import a session from; only Firefox imports directly.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ImportBrowser {
    Firefox,
    Chrome,
    Edge,
}

impl ImportBrowser {
    const ALL: [Self; 3] = [Self::Firefox, Self::Chrome, Self::Edge];

    fn label(self) -> &'static str {
        match self {
            Self::Firefox => "Firefox · automatic",
            Self::Chrome => "Chrome · guided export",
            Self::Edge => "Edge · guided export",
        }
    }
}

/// The browser the export guide is written for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ExportBrowser {
    ChromeOrEdge,
    Firefox,
}

impl ExportBrowser {
    fn label(self) -> &'static str {
        match self {
            Self::ChromeOrEdge => "Chrome / Edge",
            Self::Firefox => "Firefox",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ExportStep {
    Browser,
    Export,
    SelectFile,
}

impl ExportStep {
    const ALL: [Self; 3] = [Self::Browser, Self::Export, Self::SelectFile];

    fn label(self) -> &'static str {
        match self {
            Self::Browser => "1  Browser",
            Self::Export => "2  Export",
            Self::SelectFile => "3  Select file",
        }
    }

    fn previous(self) -> Option<Self> {
        match self {
            Self::Browser => None,
            Self::Export => Some(Self::Browser),
            Self::SelectFile => Some(Self::Export),
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::Browser => Some(Self::Export),
            Self::Export => Some(Self::SelectFile),
            Self::SelectFile => None,
        }
    }
}

impl App {
    pub(super) fn poll_session(&mut self) {
        if let Ok(result) = self.session_rx.try_recv() {
            self.session_loading = false;
            self.browser_importing = false;
            if let Some(import) = result {
                if let Some(folder) = import.path.as_ref().and_then(|p| p.parent()) {
                    self.session_folder = folder.to_path_buf();
                }
                match import.cookies {
                    Ok(session) => {
                        if self.import_cancel.load(Ordering::Relaxed) || self.closing {
                            self.session_error = Some("Browser import cancelled.".into());
                            return;
                        }
                        self.session = Some(session);
                        self.session_file = import.path;
                        self.browser_source = import.browser;
                        self.session_error = None;
                        self.error = None;
                        self.saved = None;
                        self.phase = Phase::Idle;
                        self.session_expanded = false;
                        let refresh_used = self.refresh_used;
                        if self.retry_download
                            && self.video.is_some()
                            && self.destination.is_some()
                            && self.loaded_link.as_ref().is_some_and(|loaded| {
                                engine::parse_link(&self.link).is_ok_and(|link| link == *loaded)
                            })
                        {
                            self.retry_download = false;
                            self.start_download(self.destination.clone().unwrap());
                        } else if engine::parse_link(&self.link).is_ok() && tools::ready() {
                            self.inspect();
                        }
                        self.refresh_used = refresh_used;
                    }
                    Err(error) => {
                        self.session_error = Some(error);
                        self.session_expanded = true;
                    }
                }
            }
        }
    }
    fn import_session(&mut self, folder: PathBuf) {
        self.refresh_used = false;
        self.session_loading = true;
        self.import_cancel = Arc::new(AtomicBool::new(false));
        self.session_error = None;
        let tx = self.session_tx.clone();
        std::thread::spawn(move || {
            let result = rfd::FileDialog::new()
                .set_title("Choose your YouTube cookies.txt export")
                .add_filter("Netscape cookie export", &["txt"])
                .set_directory(session::export::picker_folder(&folder))
                .pick_file()
                .map(|path| SessionImport {
                    cookies: session::SessionCookies::load(&path),
                    path: Some(path),
                    browser: None,
                });
            let _ = tx.send(result);
        });
    }
    pub(super) fn import_browser(&mut self, profile: session::firefox::Profile, automatic: bool) {
        self.refresh_used = automatic;
        self.session_loading = true;
        self.browser_importing = true;
        self.session_error = None;
        self.import_cancel = Arc::new(AtomicBool::new(false));
        let (tx, cancel) = (self.session_tx.clone(), self.import_cancel.clone());
        std::thread::spawn(move || {
            let cookies = session::firefox::import(&profile, &cancel);
            let _ = tx.send(Some(SessionImport {
                path: None,
                browser: Some(profile),
                cookies,
            }));
        });
    }
    fn browser_controls(&mut self, ui: &mut egui::Ui) {
        if !cfg!(windows) {
            ui.label("On Mac, connect your YouTube session using an exported cookies.txt file. Automatic browser import is not available in this experimental release.");
            if ui
                .add_enabled(
                    !self.download_busy(),
                    egui::Button::new("Open export guide"),
                )
                .clicked()
            {
                self.session_mode = SessionMode::ExportGuide;
                self.export_step = ExportStep::Browser;
            }
            return;
        }
        ui.label("Choose your browser. Firefox can import directly; Chrome and Edge currently need an exported file.");
        ui.add_enabled_ui(!self.download_busy(), |ui| {
            egui::ComboBox::from_id_salt("session_browser")
                .selected_text(self.browser_choice.label())
                .show_ui(ui, |ui| {
                    for browser in ImportBrowser::ALL {
                        ui.selectable_value(&mut self.browser_choice, browser, browser.label());
                    }
                });
        });
        if self.browser_choice != ImportBrowser::Firefox {
            ui.label("Direct import for this browser is not available yet. The export guide will help you connect your session.");
            if ui
                .add_enabled(
                    !self.download_busy(),
                    egui::Button::new("Open export guide"),
                )
                .clicked()
            {
                self.export_browser = ExportBrowser::ChromeOrEdge;
                self.export_step = ExportStep::Browser;
                self.session_mode = SessionMode::ExportGuide;
            }
            return;
        }
        ui.label("Import reads YouTube cookies from the Firefox profile you select. Cookies stay in memory; they are used only for YouTube and removed when you disconnect or close the app.");
        ui.label(RichText::new("Automatic import is experimental. Private windows and Firefox containers need the manual export option.").size(12.).color(MUTED));
        ui.add_enabled_ui(!self.download_busy(), |ui| {
            ui.checkbox(&mut self.auto_refresh, "Auto mode: refresh this profile once if YouTube asks to sign in");
            if self.browser_profiles.is_empty() {
                ui.label("No Firefox cookie profiles found. Open Firefox, sign in to YouTube, then refresh this list.");
            } else {
                let label = |p: &session::firefox::Profile| format!("{}{}", p.name, if p.default { " (default)" } else { "" });
                egui::ComboBox::from_id_salt("firefox_profile")
                    .selected_text(label(&self.browser_profiles[self.browser_profile]))
                    .show_ui(ui, |ui| {
                        for (index, profile) in self.browser_profiles.iter().enumerate() {
                            ui.selectable_value(&mut self.browser_profile, index, label(profile));
                        }
                    });
                if primary(ui, "Import from Firefox & retry", true).clicked() {
                    self.import_browser(self.browser_profiles[self.browser_profile].clone(), false);
                }
            }
            if ui.button("Refresh profile list").clicked() {
                self.browser_profiles = session::firefox::profiles();
                self.browser_profile = 0;
            }
        });
    }
    pub(super) fn session_panel(&mut self, ui: &mut egui::Ui) {
        if !self.session_expanded {
            return;
        }
        ui.add_space(12.);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("YouTube connection").size(18.).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Collapse").clicked() {
                        self.session_expanded = false;
                    }
                });
            });
            ui.label(
                RichText::new("Connect only when a video needs your signed-in session.")
                    .color(MUTED),
            );
            if self.session_mode == SessionMode::Browser {
                self.browser_controls(ui);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Import a cookies file…").clicked() {
                        self.session_mode = SessionMode::CookieFile;
                    }
                    if ui.button("Export guide").clicked() {
                        self.session_mode = SessionMode::ExportGuide;
                    }
                });
            } else {
                if ui.button("← Browser connection").clicked() {
                    self.session_mode = SessionMode::Browser;
                }
                if self.session_mode == SessionMode::ExportGuide {
                    self.export_instructions(ui);
                } else {
                    ui.label("Choose a Netscape-format .txt export of your YouTube cookies.");
                    self.session_file_controls(ui, false);
                }
            }
            ui.add_space(8.);
            ui.label(RichText::new("Your session stays in memory until removed or the app closes. Import retries the current link or failed download once. Cookies act like a sign-in; keep exports private.").size(12.).color(MUTED));
            if self.session_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(if self.browser_importing {
                        "Importing YouTube session from Firefox…"
                    } else {
                        "Choose your session export…"
                    });
                    if self.browser_importing && ui.button("Cancel import").clicked() {
                        self.import_cancel.store(true, Ordering::Relaxed);
                    }
                });
            }
            if self.session.is_some() {
                ui.colored_label(
                    Color32::from_rgb(31, 139, 99),
                    "YouTube cookies imported · access checked when used",
                );
                if let Some(path) = &self.session_file {
                    ui.label(
                        RichText::new(path.display().to_string())
                            .size(12.)
                            .color(MUTED),
                    );
                }
                if let Some(profile) = self.browser_source.clone() {
                    ui.label(format!("Firefox · {}", profile.name));
                    if ui
                        .add_enabled(
                            !self.download_busy(),
                            egui::Button::new("Refresh from Firefox & retry"),
                        )
                        .clicked()
                    {
                        self.import_browser(profile, false);
                    }
                }
                if ui
                    .add_enabled(!self.download_busy(), egui::Button::new("Remove session"))
                    .clicked()
                {
                    self.session = None;
                    self.session_file = None;
                    self.browser_source = None;
                    self.retry_download = false;
                    self.session_error = None;
                    self.video = None;
                    self.saved = None;
                    self.error = None;
                    self.status.clear();
                    self.log.clear();
                    self.phase = Phase::Idle;
                }
            }
            if let Some(error) = &self.session_error {
                ui.colored_label(Color32::from_rgb(175, 46, 57), error);
            }
        });
    }
    fn session_file_controls(&mut self, ui: &mut egui::Ui, guided: bool) {
        let folder = if guided {
            session::export::export_folder()
        } else {
            self.session_folder.clone()
        };
        ui.label(
            RichText::new(format!(
                "Look in: {}",
                session::export::picker_folder(&folder).display()
            ))
            .size(12.)
            .color(MUTED),
        );
        ui.add_enabled_ui(!self.download_busy(), |ui| {
            if primary(ui, "Choose exported .txt file", true).clicked() {
                self.import_session(folder);
            }
            if ui.button("Look in Downloads instead").clicked() {
                self.import_session(session::export::downloads_folder());
            }
        });
        ui.label(RichText::new("If the browser saved it automatically, open its Downloads list and reveal the file in your file manager. Choose that file here. Names such as cookies (1).txt are fine; JSON and HTML files are not cookie exports.").size(12.).color(MUTED));
    }
    fn export_instructions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for step in ExportStep::ALL {
                ui.selectable_value(&mut self.export_step, step, step.label());
            }
        });
        match self.export_step {
            ExportStep::Browser => {
                ui.label(RichText::new("Choose the browser you use for YouTube").strong());
                egui::ComboBox::from_id_salt("export_browser")
                    .selected_text(self.export_browser.label())
                    .show_ui(ui, |ui| {
                        for browser in [ExportBrowser::ChromeOrEdge, ExportBrowser::Firefox] {
                            ui.selectable_value(&mut self.export_browser, browser, browser.label());
                        }
                    });
                ui.label("Open this official store page in the selected browser. If needed, install the exporter yourself:");
                if self.export_browser == ExportBrowser::ChromeOrEdge {
                    ui.hyperlink_to(
                        "Get cookies.txt LOCALLY · Chrome Web Store",
                        session::export::CHROME_EXPORTER,
                    );
                    ui.label("Edge users: if the Chrome extension cannot be installed, use Firefox and its exporter instead.");
                } else {
                    ui.hyperlink_to(
                        "cookies.txt by Lennon Hill · Firefox Add-ons",
                        session::export::FIREFOX_EXPORTER,
                    );
                }
                ui.label("For the private-window export below, allow the exporter in private / incognito windows from your browser’s extension settings.");
            }
            ExportStep::Export => {
                ui.label(RichText::new("Export only your YouTube session").strong());
                ui.label("1. Open a new private / incognito window, sign in to YouTube, and complete any verification yourself.");
                ui.hyperlink_to("YouTube sign-in page", "https://www.youtube.com/");
                ui.label(
                    "2. In that same tab, open the page below. Keep it as the only private tab.",
                );
                ui.hyperlink_to(
                    "youtube.com/robots.txt",
                    "https://www.youtube.com/robots.txt",
                );
                ui.label(if self.export_browser == ExportBrowser::ChromeOrEdge { "3. Open Get cookies.txt LOCALLY and export cookies for youtube.com / this site in Netscape format." } else { "3. Open cookies.txt from the browser toolbar and export the current YouTube site in Netscape format." });
                ui.label("4. Save the file below, then close the private window. Use Open export folder to create and open the suggested folder first.");
                let folder = session::export::export_folder();
                ui.label(
                    RichText::new(format!(
                        "Save as: {}",
                        folder.join(session::export::EXPORT_NAME).display()
                    ))
                    .strong(),
                );
                ui.horizontal(|ui| {
                    if ui.button("Open export folder").clicked() {
                        let result = std::fs::create_dir_all(&folder).and_then(|_| platform::open_folder(&folder));
                        if result.is_err() { self.session_error = Some("Could not open the export folder. Save the file to Downloads and choose it in the next step.".into()); }
                    }
                    if ui.button("Copy folder path").clicked() { ui.ctx().copy_text(folder.display().to_string()); }
                });
                ui.label(RichText::new("If no Save As window appears, the file probably went into Downloads. Open the browser Downloads list to find its location.").size(12.).color(MUTED));
            }
            ExportStep::SelectFile => {
                ui.label(RichText::new("Select the file you just exported").strong());
                ui.label("Look for youtube-cookies.txt, cookies.txt, or a YouTube-named .txt export. Use the latest export if several files have similar names.");
                self.session_file_controls(ui, true);
            }
        }
        ui.add_space(6.);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.export_step.previous().is_some(),
                    egui::Button::new("Back"),
                )
                .clicked()
                && let Some(previous) = self.export_step.previous()
            {
                self.export_step = previous;
            }
            if let Some(next) = self.export_step.next()
                && primary(ui, "Next", true).clicked()
            {
                self.export_step = next;
            }
        });
        ui.hyperlink_to(
            "Full instructions from yt-dlp",
            "https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies",
        );
    }
}
