//! The window: shared state, the sidebar, and the frame loop. Each page is its own module.
mod download;
mod gif;
mod preview;
mod session_panel;
mod settings;
pub mod theme;
mod videos;

use crate::{engine, session, tools, update};
use download::FormatFilter;
use eframe::egui::{self, Color32, RichText, Vec2};
use engine::{Event, Video};
use session_panel::{ExportBrowser, ExportStep, ImportBrowser, SessionMode};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use theme::{BLUE, icon_tile};

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

pub struct App {
    page: Page,
    library: videos::Library,
    gif: gif::Converter,
    updater: update::Updater,
    tool_update: tools::ToolUpdate,
    link: String,
    loaded_link: Option<engine::Link>,
    video: Option<Video>,
    selected: usize,
    filter: FormatFilter,
    phase: Phase,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    folder_tx: mpsc::Sender<Option<PathBuf>>,
    folder_rx: mpsc::Receiver<Option<PathBuf>>,
    download_cancel: Arc<AtomicBool>,
    import_cancel: Arc<AtomicBool>,
    error: Option<String>,
    status: String,
    progress: f32,
    destination: Option<PathBuf>,
    saved: Option<engine::CompletedDownload>,
    show_update: bool,
    closing: bool,
    session: Option<session::SessionCookies>,
    session_expanded: bool,
    session_loading: bool,
    session_error: Option<String>,
    session_tx: mpsc::Sender<Option<SessionImport>>,
    session_rx: mpsc::Receiver<Option<SessionImport>>,
    session_folder: PathBuf,
    session_file: Option<PathBuf>,
    session_mode: SessionMode,
    browser_choice: ImportBrowser,
    browser_profiles: Vec<session::firefox::Profile>,
    browser_profile: usize,
    browser_source: Option<session::firefox::Profile>,
    browser_importing: bool,
    retry_download: bool,
    auto_refresh: bool,
    refresh_used: bool,
    export_browser: ExportBrowser,
    export_step: ExportStep,
    log: std::collections::VecDeque<String>,
}

struct SessionImport {
    path: Option<PathBuf>,
    browser: Option<session::firefox::Profile>,
    cookies: Result<session::SessionCookies, String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        Self::with_profiles(session::firefox::profiles())
    }

    fn with_profiles(browser_profiles: Vec<session::firefox::Profile>) -> Self {
        let (tx, rx) = mpsc::channel();
        let (folder_tx, folder_rx) = mpsc::channel();
        let (session_tx, session_rx) = mpsc::channel();
        Self {
            page: Page::Download,
            library: videos::Library::default(),
            gif: gif::Converter::default(),
            updater: update::Updater::default(),
            tool_update: tools::ToolUpdate::default(),
            link: String::new(),
            loaded_link: None,
            video: None,
            selected: 0,
            filter: FormatFilter::All,
            phase: Phase::Idle,
            tx,
            rx,
            folder_tx,
            folder_rx,
            download_cancel: Arc::new(AtomicBool::new(false)),
            import_cancel: Arc::new(AtomicBool::new(false)),
            error: None,
            status: String::new(),
            progress: 0.,
            destination: None,
            saved: None,
            show_update: false,
            closing: false,
            session: None,
            session_expanded: false,
            session_loading: false,
            session_error: None,
            session_tx,
            session_rx,
            session_folder: session::export::export_folder(),
            session_file: None,
            session_mode: if cfg!(windows) {
                SessionMode::Browser
            } else {
                SessionMode::CookieFile
            },
            browser_choice: ImportBrowser::Firefox,
            browser_profiles,
            browser_profile: 0,
            browser_source: None,
            browser_importing: false,
            retry_download: false,
            auto_refresh: true,
            refresh_used: false,
            export_browser: ExportBrowser::ChromeOrEdge,
            export_step: ExportStep::Browser,
            log: Default::default(),
        }
    }

    /// Whether the Download page is working: finding a video, choosing a folder, downloading,
    /// or importing a session. GIF conversions and uploads do not block it.
    fn download_busy(&self) -> bool {
        self.session_loading
            || matches!(
                self.phase,
                Phase::Inspecting | Phase::Picking | Phase::Downloading
            )
    }
    /// Whether anything is running. Closing waits for it; installing an update needs it idle.
    /// An update downloading in the background does not count.
    fn busy(&self) -> bool {
        self.download_busy()
            || self.gif.busy
            || self.library.uploader.status.active
            || self.tool_update.active
    }
    fn cancel_all(&self) {
        self.library.uploader.cancel();
        self.tool_update.cancel();
        self.gif.cancel();
        self.download_cancel.store(true, Ordering::Relaxed);
        self.import_cancel.store(true, Ordering::Relaxed);
    }
    fn poll(&mut self) {
        self.poll_session();
        self.poll_download();
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
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
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
                    if let Some(update) = &self.updater.available
                        && ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("●  Update to {}", update.version))
                                        .size(12.)
                                        .color(Color32::WHITE),
                                )
                                .fill(rail)
                                .stroke(egui::Stroke::NONE)
                                .min_size(Vec2::new(ui.available_width(), 28.)),
                            )
                            .on_hover_text("See what’s new and install the update")
                            .clicked()
                    {
                        self.show_update = true;
                    }
                });
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.closing {
            self.updater.update(ctx);
        }
        self.poll();
        self.library.poll();
        self.gif.poll();
        self.tool_update.poll();
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.cancel_all();
            if self.busy() {
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.closing {
            self.cancel_all();
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
                        Page::Download => {
                            ui.add_enabled_ui(!self.closing && !self.tool_update.active, |ui| {
                                self.content(ui)
                            });
                        }
                        Page::Settings => self.settings(ui),
                        Page::Gif => {
                            ui.add_enabled_ui(!self.closing && !self.tool_update.active, |ui| {
                                self.gif.show(ui)
                            });
                        }
                        Page::Videos => {
                            ui.add_enabled_ui(!self.closing && !self.tool_update.active, |ui| {
                                self.library.show(ui);
                            });
                        }
                    });
            });
        self.update_dialog(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_download_keeps_identity_when_input_changes() {
        let mut app = App::with_profiles(Vec::new());
        let media = engine::MediaIdentity {
            source: engine::Source::TikTok,
            source_url: "https://www.tiktok.com/@creator/video/123".into(),
            id: Some("123".into()),
            extractor_key: Some("TikTok".into()),
        };
        let path = PathBuf::from("download.mp4");
        app.link = "https://www.youtube.com/watch?v=abcdefghijk".into();
        app.tx
            .send(Event::Complete(engine::CompletedDownload {
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
        Event::Failed(engine::EngineError::SignInNeeded(engine::Source::YouTube))
    }

    #[test]
    fn format_filter_moves_a_hidden_selection_to_a_shown_format() {
        let mut app = App::with_profiles(Vec::new());
        app.video = Some(
            engine::parse_video(
                r#"{"formats":[{"format_id":"137","vcodec":"avc1","acodec":"none","height":1080},
                {"format_id":"140","vcodec":"none","acodec":"mp4a"}]}"#,
            )
            .unwrap(),
        );
        app.set_filter(FormatFilter::Audio);
        assert_eq!(
            app.video.as_ref().unwrap().formats[app.selected].format_id,
            "140"
        );
        app.set_filter(FormatFilter::All);
        assert_eq!(app.selected, 1, "A shown selection is kept");
        app.set_filter(FormatFilter::Video);
        assert_eq!(
            app.video.as_ref().unwrap().formats[app.selected].format_id,
            "137"
        );
    }

    #[test]
    fn gif_conversions_and_uploads_do_not_block_downloading() {
        let mut app = App::with_profiles(Vec::new());
        app.gif.busy = true;
        app.library.uploader.status.active = true;
        assert!(!app.download_busy());
        assert!(app.busy(), "Closing and updates still wait for them");
        app.phase = Phase::Downloading;
        assert!(app.download_busy());
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
            app.browser_source = Some(session::firefox::Profile {
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
        app.loaded_link = engine::parse_link("https://www.tiktok.com/@creator/video/123").ok();
        app.browser_source = Some(session::firefox::Profile {
            name: "Unused".into(),
            path: PathBuf::from("unused"),
            default: false,
        });
        app.tx
            .send(Event::Failed(engine::EngineError::SignInNeeded(
                engine::Source::TikTok,
            )))
            .unwrap();
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
            app.import_cancel.store(cancelled, Ordering::Relaxed);
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
