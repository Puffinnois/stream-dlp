#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod backend;
mod browser_session;
mod export_help;
mod gif_converter;
mod platform;
mod process;
mod release_channel;
mod session;
mod streamable_upload;
mod tiktok;
#[cfg(windows)]
mod updater;
#[cfg(not(windows))]
#[path = "updater_manual.rs"]
mod updater;
mod video_library;
mod video_preview;

use backend::{Event, Video};
use eframe::egui::{self, Color32, RichText, Vec2};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

const BLUE: Color32 = Color32::from_rgb(79, 70, 229);
const INK: Color32 = Color32::from_rgb(30, 33, 43);
const MUTED: Color32 = Color32::from_rgb(100, 108, 124);
const BORDER: Color32 = Color32::from_rgb(228, 230, 237);

#[derive(PartialEq)]
enum Phase {
    Idle,
    Inspecting,
    Choosing,
    Picking,
    Downloading,
    Done,
}
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Download,
    Videos,
    Gif,
    Settings,
}

struct App {
    page: Page,
    library: video_library::Library,
    gif: gif_converter::Converter,
    updater: updater::Updater,
    link: String,
    loaded_link: String,
    video: Option<Video>,
    selected: usize,
    filter: usize,
    phase: Phase,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    folder_tx: mpsc::Sender<Option<PathBuf>>,
    folder_rx: mpsc::Receiver<Option<PathBuf>>,
    cancel: Arc<AtomicBool>,
    error: Option<String>,
    status: String,
    progress: f32,
    destination: Option<PathBuf>,
    saved: Option<backend::CompletedDownload>,
    confirm_install: bool,
    closing: bool,
    session: Option<session::SessionCookies>,
    session_expanded: bool,
    session_loading: bool,
    session_error: Option<String>,
    session_tx: mpsc::Sender<Option<SessionImport>>,
    session_rx: mpsc::Receiver<Option<SessionImport>>,
    session_folder: PathBuf,
    session_file: Option<PathBuf>,
    session_mode: usize,
    browser_choice: usize,
    browser_profiles: Vec<browser_session::Profile>,
    browser_profile: usize,
    browser_source: Option<browser_session::Profile>,
    browser_importing: bool,
    retry_download: bool,
    auto_refresh: bool,
    refresh_used: bool,
    export_browser: usize,
    export_step: usize,
    log: std::collections::VecDeque<String>,
}

struct SessionImport {
    path: Option<PathBuf>,
    browser: Option<browser_session::Profile>,
    cookies: Result<session::SessionCookies, String>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        // Use the Windows UI typeface when present, with egui's bundled fallback.
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
            fonts
                .font_data
                .insert("Segoe UI".into(), egui::FontData::from_owned(data).into());
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, "Segoe UI".into());
        }
        ctx.set_fonts(fonts);
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.override_text_color = Some(INK);
        style.visuals.panel_fill = Color32::from_rgb(246, 247, 250);
        style.visuals.window_fill = Color32::WHITE;
        style.visuals.selection.bg_fill = BLUE;
        style.visuals.selection.stroke.color = Color32::WHITE;
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(232, 234, 242);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(246, 247, 251);
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., BORDER);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(239, 238, 255);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1., BLUE);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(223, 235, 255);
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1., BORDER);
        style.visuals.extreme_bg_color = Color32::from_rgb(250, 250, 253);
        for widget in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.noninteractive,
        ] {
            widget.corner_radius = egui::CornerRadius::same(8);
        }
        style.spacing.interact_size = Vec2::new(40., 36.);
        style.spacing.item_spacing = Vec2::new(10., 10.);
        style.spacing.button_padding = Vec2::new(18., 10.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.));
        ctx.set_style(style);
        Self::with_profiles(browser_session::profiles())
    }

    fn with_profiles(browser_profiles: Vec<browser_session::Profile>) -> Self {
        let (tx, rx) = mpsc::channel();
        let (folder_tx, folder_rx) = mpsc::channel();
        let (session_tx, session_rx) = mpsc::channel();
        Self {
            page: Page::Download,
            library: video_library::Library::default(),
            gif: gif_converter::Converter::default(),
            updater: updater::Updater::default(),
            link: String::new(),
            loaded_link: String::new(),
            video: None,
            selected: 0,
            filter: 0,
            phase: Phase::Idle,
            tx,
            rx,
            folder_tx,
            folder_rx,
            cancel: Arc::new(AtomicBool::new(false)),
            error: None,
            status: String::new(),
            progress: 0.,
            destination: None,
            saved: None,
            confirm_install: false,
            closing: false,
            session: None,
            session_expanded: false,
            session_loading: false,
            session_error: None,
            session_tx,
            session_rx,
            session_folder: export_help::export_folder(),
            session_file: None,
            session_mode: if cfg!(windows) { 0 } else { 1 },
            browser_choice: 0,
            browser_profiles,
            browser_profile: 0,
            browser_source: None,
            browser_importing: false,
            retry_download: false,
            auto_refresh: true,
            refresh_used: false,
            export_browser: 0,
            export_step: 0,
            log: Default::default(),
        }
    }

    fn busy(&self) -> bool {
        self.gif.busy
            || self.library.uploader.status.active
            || self.updater.downloading
            || self.session_loading
            || matches!(
                self.phase,
                Phase::Inspecting | Phase::Picking | Phase::Downloading
            )
    }
    fn inspect(&mut self) {
        match backend::normalize_url(&self.link) {
            Err(error) => self.error = Some(error),
            Ok(link) => {
                self.refresh_used = false;
                self.retry_download = false;
                self.loaded_link = link.clone();
                self.error = None;
                self.video = None;
                self.saved = None;
                // The last video's "ready" or "cancelled" line must not label the next one.
                self.status.clear();
                self.progress = 0.;
                self.log.clear();
                self.phase = Phase::Inspecting;
                self.cancel = Arc::new(AtomicBool::new(false));
                let (tx, cancel) = (self.tx.clone(), self.cancel.clone());
                let session = self.session.clone();
                std::thread::spawn(move || backend::inspect(link, session, tx, cancel));
            }
        }
    }
    fn poll(&mut self) {
        if let Ok(result) = self.session_rx.try_recv() {
            self.session_loading = false;
            self.browser_importing = false;
            if let Some(import) = result {
                if let Some(folder) = import.path.as_ref().and_then(|p| p.parent()) {
                    self.session_folder = folder.to_path_buf();
                }
                match import.cookies {
                    Ok(session) => {
                        if self.cancel.load(Ordering::Relaxed) || self.closing {
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
                            && backend::normalize_url(&self.link).as_deref()
                                == Ok(self.loaded_link.as_str())
                        {
                            self.retry_download = false;
                            self.start_download(self.destination.clone().unwrap());
                        } else if backend::normalize_url(&self.link).is_ok()
                            && backend::tools_ready()
                        {
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
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Loaded(video) => {
                    self.video = Some(video);
                    self.selected = 0;
                    self.filter = 0;
                    self.phase = Phase::Choosing;
                }
                Event::Line(line) => {
                    if let Some((fraction, status)) = backend::progress(&line) {
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
                    let needs_session = backend::needs_session(&error);
                    self.retry_download = self.phase == Phase::Downloading;
                    self.error = Some(error);
                    self.phase = if self.video.is_some() {
                        Phase::Choosing
                    } else {
                        Phase::Idle
                    };
                    if needs_session
                        && backend::source(&self.loaded_link) != Some(backend::Source::TikTok)
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
    fn start_download(&mut self, folder: PathBuf) {
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
        self.cancel = Arc::new(AtomicBool::new(false));
        let (media, format, tx, cancel) = (
            self.video.as_ref().unwrap().identity(),
            self.video.as_ref().unwrap().formats[self.selected].clone(),
            self.tx.clone(),
            self.cancel.clone(),
        );
        let session = self.session.clone();
        std::thread::spawn(move || backend::download(media, format, folder, session, tx, cancel));
    }
    /// Switches the format list (0 all, 1 video, 2 audio); a hidden selection moves to the
    /// first shown format so the download always matches a visible row.
    fn set_filter(&mut self, filter: usize) {
        self.filter = filter;
        if let Some(video) = &self.video
            && !video
                .formats
                .get(self.selected)
                .is_some_and(|f| shown(filter, f))
            && let Some(first) = video.formats.iter().position(|f| shown(filter, f))
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
    fn import_session(&mut self, folder: PathBuf) {
        self.refresh_used = false;
        self.session_loading = true;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.session_error = None;
        let tx = self.session_tx.clone();
        std::thread::spawn(move || {
            let result = rfd::FileDialog::new()
                .set_title("Choose your YouTube cookies.txt export")
                .add_filter("Netscape cookie export", &["txt"])
                .set_directory(export_help::picker_folder(&folder))
                .pick_file()
                .map(|path| SessionImport {
                    cookies: session::SessionCookies::load(&path),
                    path: Some(path),
                    browser: None,
                });
            let _ = tx.send(result);
        });
    }
    fn import_browser(&mut self, profile: browser_session::Profile, automatic: bool) {
        self.refresh_used = automatic;
        self.session_loading = true;
        self.browser_importing = true;
        self.session_error = None;
        self.cancel = Arc::new(AtomicBool::new(false));
        let (tx, cancel) = (self.session_tx.clone(), self.cancel.clone());
        std::thread::spawn(move || {
            let cookies = browser_session::import(&profile, &cancel);
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
                .add_enabled(!self.busy(), egui::Button::new("Open export guide"))
                .clicked()
            {
                self.session_mode = 2;
                self.export_step = 0;
            }
            return;
        }
        ui.label("Choose your browser. Firefox can import directly; Chrome and Edge currently need an exported file.");
        ui.add_enabled_ui(!self.busy(), |ui| {
            egui::ComboBox::from_id_salt("session_browser")
                .selected_text(
                    [
                        "Firefox · automatic",
                        "Chrome · guided export",
                        "Edge · guided export",
                    ][self.browser_choice],
                )
                .show_ui(ui, |ui| {
                    for (index, name) in [
                        "Firefox · automatic",
                        "Chrome · guided export",
                        "Edge · guided export",
                    ]
                    .iter()
                    .enumerate()
                    {
                        ui.selectable_value(&mut self.browser_choice, index, *name);
                    }
                });
        });
        if self.browser_choice != 0 {
            ui.label("Direct import for this browser is not available yet. The export guide will help you connect your session.");
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Open export guide"))
                .clicked()
            {
                self.export_browser = 0;
                self.export_step = 0;
                self.session_mode = 2;
            }
            return;
        }
        ui.label("Import reads YouTube cookies from the Firefox profile you select. Cookies stay in memory; they are used only for YouTube and removed when you disconnect or close the app.");
        ui.label(RichText::new("Automatic import is experimental. Private windows and Firefox containers need the manual export option.").size(12.).color(MUTED));
        ui.add_enabled_ui(!self.busy(), |ui| {
            ui.checkbox(&mut self.auto_refresh, "Auto mode: refresh this profile once if YouTube asks to sign in");
            if self.browser_profiles.is_empty() {
                ui.label("No Firefox cookie profiles found. Open Firefox, sign in to YouTube, then refresh this list.");
            } else {
                let label = |p: &browser_session::Profile| format!("{}{}", p.name, if p.default { " (default)" } else { "" });
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
                self.browser_profiles = browser_session::profiles();
                self.browser_profile = 0;
            }
        });
    }
    fn session_panel(&mut self, ui: &mut egui::Ui) {
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
            if self.session_mode == 0 {
                self.browser_controls(ui);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Import a cookies file…").clicked() {
                        self.session_mode = 1;
                    }
                    if ui.button("Export guide").clicked() {
                        self.session_mode = 2;
                    }
                });
            } else {
                if ui.button("← Browser connection").clicked() {
                    self.session_mode = 0;
                }
                if self.session_mode == 2 {
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
                        self.cancel.store(true, Ordering::Relaxed);
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
                            !self.busy(),
                            egui::Button::new("Refresh from Firefox & retry"),
                        )
                        .clicked()
                    {
                        self.import_browser(profile, false);
                    }
                }
                if ui
                    .add_enabled(!self.busy(), egui::Button::new("Remove session"))
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
            export_help::export_folder()
        } else {
            self.session_folder.clone()
        };
        ui.label(
            RichText::new(format!(
                "Look in: {}",
                export_help::picker_folder(&folder).display()
            ))
            .size(12.)
            .color(MUTED),
        );
        ui.add_enabled_ui(!self.busy(), |ui| {
            if primary(ui, "Choose exported .txt file", true).clicked() {
                self.import_session(folder);
            }
            if ui.button("Look in Downloads instead").clicked() {
                self.import_session(export_help::downloads_folder());
            }
        });
        ui.label(RichText::new("If the browser saved it automatically, open its Downloads list and reveal the file in your file manager. Choose that file here. Names such as cookies (1).txt are fine; JSON and HTML files are not cookie exports.").size(12.).color(MUTED));
    }

    fn export_instructions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (step, label) in ["1  Browser", "2  Export", "3  Select file"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.export_step, step, *label);
            }
        });
        match self.export_step {
            0 => {
                ui.label(RichText::new("Choose the browser you use for YouTube").strong());
                egui::ComboBox::from_id_salt("export_browser")
                    .selected_text(if self.export_browser == 0 {
                        "Chrome / Edge"
                    } else {
                        "Firefox"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.export_browser, 0, "Chrome / Edge");
                        ui.selectable_value(&mut self.export_browser, 1, "Firefox");
                    });
                ui.label("Open this official store page in the selected browser. If needed, install the exporter yourself:");
                if self.export_browser == 0 {
                    ui.hyperlink_to(
                        "Get cookies.txt LOCALLY · Chrome Web Store",
                        export_help::CHROME_EXPORTER,
                    );
                    ui.label("Edge users: if the Chrome extension cannot be installed, use Firefox and its exporter instead.");
                } else {
                    ui.hyperlink_to(
                        "cookies.txt by Lennon Hill · Firefox Add-ons",
                        export_help::FIREFOX_EXPORTER,
                    );
                }
                ui.label("For the private-window export below, allow the exporter in private / incognito windows from your browser’s extension settings.");
            }
            1 => {
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
                ui.label(if self.export_browser == 0 { "3. Open Get cookies.txt LOCALLY and export cookies for youtube.com / this site in Netscape format." } else { "3. Open cookies.txt from the browser toolbar and export the current YouTube site in Netscape format." });
                ui.label("4. Save the file below, then close the private window. Use Open export folder to create and open the suggested folder first.");
                let folder = export_help::export_folder();
                ui.label(
                    RichText::new(format!(
                        "Save as: {}",
                        folder.join(export_help::EXPORT_NAME).display()
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
            _ => {
                ui.label(RichText::new("Select the file you just exported").strong());
                ui.label("Look for youtube-cookies.txt, cookies.txt, or a YouTube-named .txt export. Use the latest export if several files have similar names.");
                self.session_file_controls(ui, true);
            }
        }
        ui.add_space(6.);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.export_step > 0, egui::Button::new("Back"))
                .clicked()
            {
                self.export_step -= 1;
            }
            if self.export_step < 2 && primary(ui, "Next", true).clicked() {
                self.export_step += 1;
            }
        });
        ui.hyperlink_to(
            "Full instructions from yt-dlp",
            "https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies",
        );
    }
    fn settings(&mut self, ui: &mut egui::Ui) {
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
                    release_channel::Channel::installed().label()
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
                            release_channel::Channel::Public,
                            "Public · stable releases",
                        );
                        ui.selectable_value(
                            &mut channel,
                            release_channel::Channel::Dev,
                            "Dev · experimental prereleases",
                        );
                    });
            });
            if channel != self.updater.channel {
                self.updater.set_channel(channel, ui.ctx());
            }
            if self.updater.channel == release_channel::Channel::Dev {
                ui.label(
                    RichText::new("Dev builds are experimental and may have unfinished features.")
                        .size(12.)
                        .color(MUTED),
                );
            }
            if self.updater.channel != release_channel::Channel::installed() {
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
                    ui.hyperlink_to("Download Mac package", updater::RELEASES);
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
                    ui.hyperlink_to("Get Mac updates", updater::RELEASES);
                }
                ui.hyperlink_to("Release notes", updater::RELEASES);
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
            ui.label(if backend::tools_ready() {
                "All download tools are ready."
            } else {
                "Download tools need setup."
            });
            ui.label(RichText::new("yt-dlp finds streams, FFmpeg combines video and audio, and Deno handles YouTube’s JavaScript challenges.").color(MUTED));
            egui::CollapsingHeader::new("Tool details & setup").show(ui, |ui| {
                ui.label(format!("Location: {}", backend::tools_dir().display()));
                ui.label(platform::tools_help());
                ui.hyperlink_to("yt-dlp project", "https://github.com/yt-dlp/yt-dlp");
            });
            if !backend::tools_ready() {
                ui.label(platform::tools_help());
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

    fn install_confirmation(&mut self, ctx: &egui::Context) {
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

    fn sidebar(&mut self, ctx: &egui::Context) {
        let rail = Color32::from_rgb(26, 29, 40);
        let quiet = Color32::from_rgb(184, 190, 208);
        egui::SidePanel::left("sidebar")
            .exact_width(188.)
            .resizable(false)
            .frame(egui::Frame::new().fill(rail).inner_margin(16))
            .show(ctx, |ui| {
                ui.add_space(16.);
                ui.horizontal(|ui| {
                    icon_tile(ui, 36., BLUE, Color32::WHITE);
                    ui.label(
                        RichText::new("stream-dlp")
                            .size(19.)
                            .color(Color32::WHITE)
                            .strong(),
                    );
                });
                ui.add_space(36.);
                ui.label(RichText::new("WORKSPACE").size(10.).color(quiet));
                ui.add_space(4.);
                for (page, label, working) in [
                    (
                        Page::Download,
                        "Download",
                        matches!(self.phase, Phase::Inspecting | Phase::Downloading),
                    ),
                    (
                        Page::Videos,
                        "My videos",
                        self.library.uploader.status.active,
                    ),
                    (Page::Gif, "GIF converter", self.gif.busy),
                ] {
                    let active = self.page == page;
                    let label = if working {
                        format!("{label}  ·")
                    } else {
                        label.into()
                    };
                    let button =
                        egui::Button::new(RichText::new(label).size(15.).color(if active {
                            Color32::WHITE
                        } else {
                            quiet
                        }))
                        .fill(if active { BLUE } else { rail })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(9)
                        .min_size(Vec2::new(ui.available_width(), 44.));
                    if ui.add(button).clicked() {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(8.);
                    ui.label(
                        RichText::new(format!(
                            "{} · v{}",
                            release_channel::Channel::installed().label(),
                            env!("CARGO_PKG_VERSION")
                        ))
                        .size(11.)
                        .color(quiet),
                    );
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Settings").color(
                                if self.page == Page::Settings {
                                    Color32::WHITE
                                } else {
                                    quiet
                                },
                            ))
                            .fill(if self.page == Page::Settings {
                                BLUE
                            } else {
                                rail
                            })
                            .min_size(Vec2::new(ui.available_width(), 40.))
                            .stroke(egui::Stroke::NONE),
                        )
                        .clicked()
                    {
                        self.page = Page::Settings;
                    }
                    if self.updater.available.is_some()
                        && ui
                            .add(
                                egui::Button::new(
                                    RichText::new(if self.updater.downloading {
                                        "Installing update…"
                                    } else {
                                        "●  Update available"
                                    })
                                    .size(12.)
                                    .color(quiet),
                                )
                                .fill(rail)
                                .stroke(egui::Stroke::NONE)
                                .min_size(Vec2::new(ui.available_width(), 28.)),
                            )
                            .on_hover_text("Open Settings to review the available update")
                            .clicked()
                    {
                        self.page = Page::Settings;
                    }
                });
            });
    }
    fn content(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Download",
            "Save a YouTube or TikTok video in the quality you choose.",
        );
        card(ui, |ui| {
            ui.label(RichText::new("Video link").size(14.).color(INK).strong());
            ui.add_space(2.);
            ui.horizontal(|ui| {
                let busy = self.busy();
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
                    !busy && !self.link.trim().is_empty() && backend::tools_ready(),
                )
                .clicked()
                    || (enter && !busy && backend::tools_ready())
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
                        !self.busy(),
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
        if !backend::tools_ready() {
            ui.add_space(12.);
            ui.colored_label(
                Color32::from_rgb(174, 100, 20),
                format!("Download tools are missing. {}", platform::tools_help()),
            );
        }
        if self.browser_importing {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Importing your YouTube session from Firefox…");
                    if ui.button("Cancel import").clicked() {
                        self.cancel.store(true, Ordering::Relaxed);
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
                    if backend::source(&self.loaded_link) == Some(backend::Source::YouTube)
                        && ui
                            .add_enabled(
                                !self.busy(),
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
                        self.cancel.store(true, Ordering::Relaxed);
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
                    ui.add_enabled_ui(!self.busy(), |ui| {
                        for (index, name) in [(2, "Audio"), (1, "Video"), (0, "All")] {
                            if ui.selectable_label(self.filter == index, name).clicked() {
                                self.set_filter(index);
                            }
                        }
                    });
                });
            });
            ui.label(
                RichText::new(if video.source == backend::Source::TikTok {
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
                    ui.add_enabled_ui(!self.busy(), |ui| {
                        for (index, format) in video.formats.iter().enumerate() {
                            if !shown(self.filter, format) {
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
                            self.cancel.store(true, Ordering::Relaxed);
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

fn icon_tile(ui: &mut egui::Ui, size: f32, background: Color32, foreground: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    ui.painter().rect_filled(rect, size * 0.25, background);
    let center = rect.center();
    let stroke = egui::Stroke::new(2., foreground);
    ui.painter().line_segment(
        [
            center - egui::vec2(0., size * 0.19),
            center + egui::vec2(0., size * 0.16),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            center + egui::vec2(-size * 0.13, size * 0.03),
            center + egui::vec2(0., size * 0.16),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            center + egui::vec2(size * 0.13, size * 0.03),
            center + egui::vec2(0., size * 0.16),
        ],
        stroke,
    );
}

fn shown(filter: usize, format: &backend::Format) -> bool {
    match filter {
        1 => format.video(),
        2 => !format.video(),
        _ => true,
    }
}

fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add_space(6.);
    ui.label(RichText::new(title).size(32.).strong());
    ui.label(RichText::new(subtitle).size(14.).color(MUTED));
    ui.add_space(22.);
}

fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(BLUE)
            .stroke(egui::Stroke::NONE)
            .corner_radius(9)
            .min_size(Vec2::new(0., 42.)),
    )
}
fn card<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(egui::Stroke::new(1., BORDER))
        .corner_radius(16)
        .inner_margin(24)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui)
        })
        .inner
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.updater.poll() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.closing {
            self.updater.check_automatically(ctx);
        }
        self.poll();
        self.library.poll();
        self.gif.poll();
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.library.uploader.cancel();
            self.gif.cancel();
            self.cancel.store(true, Ordering::Relaxed);
            if self.busy() {
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.closing {
            self.cancel.store(true, Ordering::Relaxed);
            if !self.busy() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(246, 247, 250))
                    .inner_margin(32),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("page")
                    .show(ui, |ui| match self.page {
                        Page::Download => self.content(ui),
                        Page::Settings => self.settings(ui),
                        Page::Gif => {
                            ui.add_enabled_ui(!self.updater.downloading && !self.closing, |ui| {
                                self.gif.show(ui)
                            });
                        }
                        Page::Videos => {
                            ui.add_enabled_ui(!self.updater.downloading && !self.closing, |ui| {
                                self.library.show(ui);
                            });
                        }
                    });
            });
        self.install_confirmation(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.library.uploader.cancel();
        self.gif.cancel();
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn main() -> eframe::Result {
    if std::env::args().skip(1).any(|arg| arg == "--check-tools") {
        // CI can exercise the installed app's actual bundle lookup without a
        // desktop session, browser profile, or external video service.
        for name in ["yt-dlp", "ffmpeg", "ffprobe", "deno"] {
            let path = backend::tools_dir().join(platform::executable(name));
            let result = process::isolate(std::process::Command::new(&path).arg(
                if name == "yt-dlp" || name == "deno" {
                    "--version"
                } else {
                    "-version"
                },
            ))
            .status();
            if !result.is_ok_and(|status| status.success()) {
                eprintln!("Bundled tool check failed: {}", path.display());
                std::process::exit(1);
            }
        }
        println!("All bundled download tools are ready.");
        return Ok(());
    }
    std::thread::spawn(updater::remove_stale_installers);
    eframe::run_native(
        "stream-dlp",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1040., 840.])
                .with_min_inner_size([880., 700.])
                .with_icon(app_icon()),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/app.png"))
        .expect("the bundled app icon is valid PNG")
}

#[cfg(test)]
mod app_tests {
    use super::*;

    #[test]
    fn completed_download_keeps_identity_when_input_changes() {
        let mut app = App::with_profiles(Vec::new());
        let media = backend::MediaIdentity {
            source: backend::Source::TikTok,
            source_url: "https://www.tiktok.com/@creator/video/123".into(),
            id: Some("123".into()),
            extractor_key: Some("TikTok".into()),
        };
        let path = PathBuf::from("download.mp4");
        app.link = "https://www.youtube.com/watch?v=abcdefghijk".into();
        app.tx
            .send(Event::Complete(backend::CompletedDownload {
                path: path.clone(),
                media: media.clone(),
            }))
            .unwrap();
        app.poll();
        let saved = app.saved.as_ref().unwrap();
        assert_eq!(saved.path, path);
        assert_eq!(saved.media, media);
        assert!(app.phase == Phase::Done);
    }

    fn auth_error() -> Event {
        Event::Failed(backend::YOUTUBE_SIGN_IN.into())
    }

    #[test]
    fn format_filter_moves_a_hidden_selection_to_a_shown_format() {
        let mut app = App::with_profiles(Vec::new());
        app.video = Some(
            backend::parse_video(
                r#"{"formats":[{"format_id":"137","vcodec":"avc1","acodec":"none","height":1080},
                {"format_id":"140","vcodec":"none","acodec":"mp4a"}]}"#,
            )
            .unwrap(),
        );
        app.set_filter(2);
        assert_eq!(
            app.video.as_ref().unwrap().formats[app.selected].format_id,
            "140"
        );
        app.set_filter(0);
        assert_eq!(app.selected, 1, "A shown selection is kept");
        app.set_filter(1);
        assert_eq!(
            app.video.as_ref().unwrap().formats[app.selected].format_id,
            "137"
        );
    }

    #[test]
    fn cancelled_inspection_does_not_mention_partial_downloads() {
        for (phase, partial) in [(Phase::Inspecting, false), (Phase::Downloading, true)] {
            let mut app = App::with_profiles(Vec::new());
            app.status = "Your download is ready.".into();
            app.phase = phase;
            app.tx.send(Event::Cancelled).unwrap();
            app.poll();
            assert_eq!(app.status.contains("Partial downloads"), partial);
            assert!(partial || app.status.is_empty());
        }
    }

    #[test]
    fn automatic_refresh_is_bounded_and_disabled_mode_does_not_import() {
        let dir = tempfile::tempdir().unwrap();
        for enabled in [false, true] {
            let mut app = App::with_profiles(Vec::new());
            app.auto_refresh = enabled;
            app.browser_source = Some(browser_session::Profile {
                name: "Synthetic missing profile".into(),
                path: dir.path().join("missing"),
                default: false,
            });
            app.tx.send(auth_error()).unwrap();
            app.poll();
            if enabled {
                assert!(app.session_loading);
                assert!(app.refresh_used);
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                while app.session_loading && std::time::Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(5));
                    app.poll();
                }
                assert!(!app.session_loading);
                assert!(
                    app.session_expanded,
                    "Failed background import must show its fallback"
                );
                assert!(app.session_error.is_some());
                // A second auth error must not launch another automatic refresh.
                app.tx.send(auth_error()).unwrap();
                app.poll();
                assert!(!app.session_loading);
            } else {
                assert!(!app.session_loading);
                assert!(!app.refresh_used);
            }
        }
    }

    #[test]
    fn tiktok_errors_cannot_refresh_youtube_sessions() {
        let mut app = App::with_profiles(Vec::new());
        app.loaded_link = "https://www.tiktok.com/@creator/video/123".into();
        app.browser_source = Some(browser_session::Profile {
            name: "Unused".into(),
            path: PathBuf::from("unused"),
            default: false,
        });
        app.tx.send(auth_error()).unwrap();
        app.poll();
        assert!(!app.session_loading);
        assert!(!app.refresh_used);
    }

    #[test]
    fn import_result_preserves_refresh_budget_and_cancel_discards_new_session() {
        for cancelled in [false, true] {
            let mut app = App::with_profiles(Vec::new());
            app.refresh_used = true;
            app.session_expanded = true;
            app.session_loading = true;
            app.browser_importing = true;
            app.cancel.store(cancelled, Ordering::Relaxed);
            app.session_tx
                .send(Some(SessionImport {
                    path: None,
                    browser: None,
                    cookies: Ok(session::fake_session()),
                }))
                .unwrap();
            app.poll();
            assert!(!app.session_loading);
            assert_eq!(app.session.is_some(), !cancelled);
            assert_eq!(
                app.session_expanded, cancelled,
                "Successful import collapses the connection panel; cancellation keeps it open"
            );
            assert!(app.refresh_used);
        }
    }
}
