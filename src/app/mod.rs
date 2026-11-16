//! The window: shared state, the sidebar, and the frame loop. Each page is its own module.
mod dev;
mod download;
mod extension_panel;
mod feedback;
mod gif;
mod preview;
mod recent;
mod session_panel;
mod settings;
pub mod theme;
mod usage;
mod videos;

use crate::{engine, session, tools, update};
use download::FormatFilter;
use eframe::egui::{self, Color32, RichText};
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
use theme::{
    ACCENT, PAGE_MARGIN, RAIL, RAIL_TEXT, SPACE_L, SPACE_S, SPACE_XL, TEXT_CAPTION, TEXT_SECTION,
    icon_tile, nav_item, semibold,
};

const NAV_HEIGHT: f32 = 40.;

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
    dev: dev::DevTools,
    feedback: feedback::Feedback,
    usage: crate::usage::Reporter,
    usage_view: usage::UsageView,
    offer_note: bool,
    link: String,
    loaded_link: Option<engine::Link>,
    video: Option<Video>,
    entries: Vec<Video>,
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
    /// What kind of error `error` is, for public feedback, which never carries its text.
    error_kind: Option<crate::feedback::ErrorKind>,
    status: String,
    progress: f32,
    destination: Option<PathBuf>,
    saved: Option<engine::CompletedDownload>,
    show_update: bool,
    closing: bool,
    sessions: std::collections::HashMap<engine::Source, session::SessionCookies>,
    session_expanded: bool,
    session_loading: bool,
    session_error: Option<String>,
    session_tx: mpsc::Sender<Option<SessionImport>>,
    session_rx: mpsc::Receiver<Option<SessionImport>>,
    session_folder: PathBuf,
    session_files: std::collections::HashMap<engine::Source, PathBuf>,
    session_mode: SessionMode,
    browser_choice: ImportBrowser,
    browser_profiles: Vec<session::firefox::Profile>,
    browser_profile: usize,
    browser_source: Option<session::firefox::Profile>,
    browser_importing: bool,
    retry_download: bool,
    auto_refresh: bool,
    refresh_used: bool,
    /// Dev mode's experimental Chrome/Edge extension connection (`extension_panel`).
    extension: session::chrome::bridge::Bridge,
    /// The browsers the extension's host is registered with; read when the panel first shows.
    extension_registered: Option<Vec<session::chrome::registry::Browser>>,
    /// A refresh asked of the extension, with the sign-in error to show if none arrives.
    extension_refresh: Option<Option<String>>,
    export_browser: ExportBrowser,
    export_step: ExportStep,
    log: std::collections::VecDeque<String>,
}

struct SessionImport {
    path: Option<PathBuf>,
    browser: Option<session::firefox::Profile>,
    /// Sent by the Chrome/Edge extension.
    from_extension: bool,
    cookies: Result<session::SessionCookies, String>,
}

impl App {
    /// `dev_mode` is the `--dev-mode` flag.
    pub fn new(cc: &eframe::CreationContext<'_>, dev_mode: bool) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self {
            dev: dev::DevTools::new(crate::dev::Settings::load(), dev_mode),
            usage: crate::usage::Reporter::new(crate::feedback::endpoint()),
            ..Self::with_profiles(session::firefox::profiles())
        };
        app.library.recent.start(app.dev.settings.download_history);
        app
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
            dev: dev::DevTools::new(crate::dev::Settings::default(), false),
            feedback: feedback::Feedback::new(crate::feedback::endpoint()),
            usage: crate::usage::Reporter::off(),
            usage_view: usage::UsageView::default(),
            offer_note: false,
            link: String::new(),
            loaded_link: None,
            video: None,
            entries: Vec::new(),
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
            error_kind: None,
            status: String::new(),
            progress: 0.,
            destination: None,
            saved: None,
            show_update: false,
            closing: false,
            sessions: Default::default(),
            session_expanded: false,
            session_loading: false,
            session_error: None,
            session_tx,
            session_rx,
            session_folder: session::export::export_folder(),
            session_files: Default::default(),
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
            extension: Default::default(),
            extension_registered: None,
            extension_refresh: None,
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
        self.poll_extension();
        self.poll_session();
        self.poll_download();
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(188.)
            .resizable(false)
            .frame(egui::Frame::new().fill(RAIL).inner_margin(SPACE_L))
            .show(ctx, |ui| {
                ui.add_space(SPACE_L);
                ui.horizontal(|ui| {
                    icon_tile(ui, 36., ACCENT, Color32::WHITE);
                    ui.label(
                        semibold("stream-dlp")
                            .size(TEXT_SECTION)
                            .color(Color32::WHITE),
                    );
                });
                ui.add_space(SPACE_XL);
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
                    let item = nav_item(ui, RichText::new(label), self.page == page, NAV_HEIGHT);
                    if working {
                        // Says which page is busy from anywhere in the app.
                        let spot = egui::Rect::from_center_size(
                            item.rect.right_center() - egui::vec2(SPACE_L, 0.),
                            egui::Vec2::splat(14.),
                        );
                        egui::Spinner::new()
                            .color(Color32::WHITE)
                            .paint_at(ui, spot);
                    }
                    if item.clicked() {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(SPACE_S);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .size(TEXT_CAPTION)
                                .color(RAIL_TEXT),
                        );
                        self.dev_chip(ui);
                    });
                    if nav_item(
                        ui,
                        RichText::new("Settings"),
                        self.page == Page::Settings,
                        NAV_HEIGHT,
                    )
                    .clicked()
                    {
                        self.page = Page::Settings;
                    }
                    self.feedback_button(ui);
                    if let Some(update) = &self.updater.available
                        && nav_item(
                            ui,
                            RichText::new(format!("●  Update to {}", update.version))
                                .size(TEXT_CAPTION),
                            false,
                            28.,
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
        self.dev_input(ctx);
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
            .frame(egui::Frame::new().fill(theme::PAGE))
            .show(ctx, |ui| {
                // The margin is inside the scroll area, so the scrollbar sits in it, clear of
                // the cards.
                egui::ScrollArea::vertical()
                    .id_salt("page")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Frame::new().inner_margin(PAGE_MARGIN).show(ui, |ui| {
                            match self.page {
                                Page::Download => {
                                    ui.add_enabled_ui(
                                        !self.closing && !self.tool_update.active,
                                        |ui| self.content(ui),
                                    );
                                }
                                Page::Settings => self.settings(ui),
                                Page::Gif => {
                                    ui.add_enabled_ui(
                                        !self.closing && !self.tool_update.active,
                                        |ui| self.gif.show(ui),
                                    );
                                }
                                Page::Videos => {
                                    ui.add_enabled_ui(
                                        !self.closing && !self.tool_update.active,
                                        |ui| {
                                            self.library.show(ui);
                                        },
                                    );
                                }
                            }
                        });
                    });
            });
        self.update_dialog(ctx);
        self.dev_windows(ctx);
        self.feedback_windows(ctx);
        self.usage_frame(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.cancel_all();
        // Tells a connected extension and removes the channel file; the registration stays.
        self.extension.stop();
        self.usage.closed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_download_keeps_identity_when_input_changes() {
        let mut app = App::with_profiles(Vec::new());
        let media = engine::MediaIdentity {
            entry_index: None,
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
    fn the_session_link_names_the_pasted_platform_or_hides() {
        let mut app = App::with_profiles(Vec::new());
        for (link, source) in [
            ("", Some(engine::Source::YouTube)),
            ("  ", Some(engine::Source::YouTube)),
            // A TikTok profile is refused by TikTok's rules but still offers its session.
            (
                "https://www.tiktok.com/@creator",
                Some(engine::Source::TikTok),
            ),
            ("instagram.com/p/Example/", Some(engine::Source::Instagram)),
            ("not a link", None),
            ("https://media.example/clip.mp4", None),
            ("https://youtube.com.example/watch?v=abcdefghijk", None),
        ] {
            app.link = link.into();
            assert_eq!(app.session_source(), source, "{link:?}");
        }
        // An expanded panel for a link without a platform does not hide the empty state.
        app.session_expanded = true;
        assert!(app.show_empty_state());
        app.link = "x.com/example/status/1".into();
        assert!(!app.show_empty_state());
    }

    #[test]
    fn the_empty_state_gives_way_to_errors_and_the_session_panel() {
        let mut app = App::with_profiles(Vec::new());
        assert!(app.show_empty_state());
        app.error = Some("Failed".into());
        assert!(!app.show_empty_state());
        app.error = None;
        app.session_expanded = true;
        assert!(!app.show_empty_state());
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
                    from_extension: false,
                    cookies: Ok(session::fake_session()),
                }))
                .unwrap();
            app.poll();
            assert!(!app.session_loading);
            assert_eq!(
                app.sessions.contains_key(&engine::Source::YouTube),
                !cancelled
            );
            assert_eq!(
                app.session_expanded, cancelled,
                "Successful import collapses the connection panel; cancellation keeps it open"
            );
            assert!(app.refresh_used);
        }
    }

    mod extension {
        use super::*;
        use session::chrome::{channel, framing, messages::ToExtension};
        use std::net::TcpStream;

        const SESSION: &str = r#"{"type":"session","cookies":[{"domain":".youtube.com","hostOnly":false,"path":"/","secure":true,"httpOnly":true,"expirationDate":4102444800,"name":"TEST","value":"fake"}]}"#;

        fn send(host: &mut TcpStream, json: &str) {
            std::io::Write::write_all(host, &framing::frame(json.as_bytes())).unwrap();
        }

        fn poll_until(app: &mut App, done: impl Fn(&App) -> bool) {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !done(app) {
                assert!(std::time::Instant::now() < deadline, "timed out");
                std::thread::sleep(Duration::from_millis(10));
                app.poll();
            }
        }

        /// An app with a session from a fake extension connection (the test plays the host).
        fn connected() -> (App, TcpStream, tempfile::TempDir) {
            let folder = tempfile::tempdir().unwrap();
            let mut app = App::with_profiles(Vec::new());
            app.extension.start(folder.path(), Arc::new(|| {})).unwrap();
            let mut host = channel::connect(folder.path()).unwrap();
            send(&mut host, r#"{"type":"hello","protocol":1}"#);
            send(&mut host, SESSION);
            poll_until(&mut app, |app| {
                app.sessions.contains_key(&engine::Source::YouTube)
            });
            assert!(app.extension.has_session());
            for _ in 0..2 {
                let _: ToExtension = framing::read_message(&mut host).unwrap();
            }
            (app, host, folder)
        }

        #[test]
        fn a_sign_in_error_asks_the_extension_once_and_imports_its_answer() {
            let (mut app, mut host, _folder) = connected();
            app.tx.send(auth_error()).unwrap();
            app.poll();
            assert!(app.refresh_used && app.session_loading);
            assert!(app.error.is_none(), "The error waits for the refresh");
            assert_eq!(
                framing::read_message::<ToExtension>(&mut host).unwrap(),
                ToExtension::Refresh
            );
            send(&mut host, SESSION);
            poll_until(&mut app, |app| !app.session_loading);
            assert!(app.extension_refresh.is_none());
            assert!(app.refresh_used, "The refresh budget survives the import");
            app.tx.send(auth_error()).unwrap();
            app.poll();
            assert!(!app.session_loading, "Only one refresh per inspection");
            assert!(app.error.is_some());
        }

        #[test]
        fn a_closed_connection_shows_the_original_error() {
            let (mut app, mut host, _folder) = connected();
            app.tx.send(auth_error()).unwrap();
            app.poll();
            let _: ToExtension = framing::read_message(&mut host).unwrap();
            drop(host);
            poll_until(&mut app, |app| !app.session_loading);
            assert!(app.error.is_some());
            assert!(app.sessions.contains_key(&engine::Source::YouTube));
        }

        #[test]
        fn disconnect_from_either_side_removes_the_session() {
            let (mut app, mut host, _folder) = connected();
            send(&mut host, r#"{"type":"disconnect"}"#);
            poll_until(&mut app, |app| app.sessions.is_empty());
            assert!(!app.extension.has_session());

            let (mut app, mut host, _folder) = connected();
            assert!(app.extension.stop());
            app.remove_extension_session();
            assert!(app.sessions.is_empty());
            assert_eq!(
                framing::read_message::<ToExtension>(&mut host).unwrap(),
                ToExtension::Disconnect
            );
        }

        #[test]
        fn another_import_replaces_the_extension_session_and_is_never_refreshed() {
            let (mut app, mut host, _folder) = connected();
            app.session_tx
                .send(Some(SessionImport {
                    path: None,
                    browser: None,
                    from_extension: false,
                    cookies: Ok(session::fake_session()),
                }))
                .unwrap();
            app.poll();
            assert!(!app.extension.has_session());
            app.tx.send(auth_error()).unwrap();
            app.poll();
            assert!(!app.session_loading);
            host.set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            assert!(framing::read_message::<ToExtension>(&mut host).is_err());
        }
    }
}
