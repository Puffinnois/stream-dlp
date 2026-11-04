//! Public feedback's UI: the Send feedback panel anyone can use, and Dev mode's Inbox where a
//! developer decides what reaches `notes.md`. The rules and requests are `crate::feedback`; see
//! `docs/reference/public-feedback.md`.
use super::{App, Page, Phase, theme};
use crate::dev::notes::{self, Tag};
use crate::engine;
use crate::feedback::{self as service, Context, Decision, ErrorKind, InboxPage, Pending};
use eframe::egui::{self, Key, RichText, Vec2};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

/// Between two notes from this copy of the app; the service has the limits that count.
const COOLDOWN: Duration = Duration::from_secs(30);

pub struct Feedback {
    /// Without an address the app shows no feedback at all.
    url: Option<String>,
    open: bool,
    focus: bool,
    text: String,
    tag: Tag,
    context: Option<Context>,
    sending: Option<mpsc::Receiver<Result<(), String>>>,
    next_send: Option<Instant>,
    message: Option<String>,
    inbox: Inbox,
}

#[derive(Default)]
struct Inbox {
    open: bool,
    token: String,
    page: InboxPage,
    work: Option<mpsc::Receiver<Result<Done, String>>>,
    confirm_block: Option<String>,
    message: Option<String>,
}

enum Done {
    Loaded(InboxPage),
    Decided(String, Option<String>),
}

impl Feedback {
    pub fn new(url: Option<String>) -> Self {
        Self {
            url,
            open: false,
            focus: false,
            text: String::new(),
            tag: Tag::default(),
            context: None,
            sending: None,
            next_send: None,
            message: None,
            inbox: Inbox::default(),
        }
    }
}

impl App {
    /// All a public note says about the app. Only fixed words go in, so the link, the error's
    /// text and any file name stay on the machine.
    fn feedback_context(&self) -> Context {
        match self.page {
            Page::Download => {
                let phase = match self.phase {
                    Phase::Idle => service::Phase::Idle,
                    Phase::Inspecting => service::Phase::Inspecting,
                    Phase::Choosing => service::Phase::Choosing,
                    Phase::Picking => service::Phase::Picking,
                    Phase::Downloading => service::Phase::Downloading,
                    Phase::Done => service::Phase::Done,
                };
                let source = engine::parse_link(&self.link)
                    .ok()
                    .map(|link| link.source.into());
                let error = self
                    .error
                    .as_ref()
                    .map(|_| self.error_kind.unwrap_or(ErrorKind::Other));
                Context::new(service::Page::Download, Some(phase), source, error)
            }
            Page::Videos => {
                let error = self.library.has_error().then_some(ErrorKind::Other);
                Context::new(service::Page::Videos, None, None, error)
            }
            Page::Gif => {
                let error = self.gif.has_error().then_some(ErrorKind::Other);
                Context::new(service::Page::Gif, None, None, error)
            }
            Page::Settings => Context::new(service::Page::Settings, None, None, None),
        }
    }

    fn open_feedback(&mut self) {
        self.feedback.context = Some(self.feedback_context());
        self.feedback.open = true;
        self.feedback.focus = true;
    }

    /// The sidebar's entry, above Settings.
    pub(super) fn feedback_button(&mut self, ui: &mut egui::Ui) {
        if self.feedback.url.is_some()
            && theme::nav_item(
                ui,
                RichText::new("Send feedback").size(theme::TEXT_CAPTION),
                self.feedback.open,
                28.,
            )
            .on_hover_text("Tell the developers about a problem or an idea")
            .clicked()
        {
            self.open_feedback();
        }
    }

    pub(super) fn feedback_windows(&mut self, ctx: &egui::Context) {
        self.poll_feedback(ctx);
        if self.dev.capturing() {
            return;
        }
        if self.feedback.open {
            self.feedback_panel(ctx);
        }
        if self.feedback.inbox.open {
            self.inbox_window(ctx);
        }
    }

    fn poll_feedback(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.feedback.sending {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.feedback.sending = None;
                    self.feedback.text.clear();
                    self.feedback.tag = Tag::default();
                    self.feedback.message = None;
                    self.feedback.open = false;
                    self.feedback.next_send = Some(Instant::now() + COOLDOWN);
                    self.dev.toast("Thank you. Your note was sent.");
                }
                Ok(Err(error)) => {
                    self.feedback.sending = None;
                    self.feedback.message = Some(error);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100))
                }
                Err(mpsc::TryRecvError::Disconnected) => self.feedback.sending = None,
            }
        }
        if let Some(rx) = &self.feedback.inbox.work {
            match rx.try_recv() {
                Ok(result) => {
                    self.feedback.inbox.work = None;
                    self.inbox_finished(result);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100))
                }
                Err(mpsc::TryRecvError::Disconnected) => self.feedback.inbox.work = None,
            }
        }
    }

    fn feedback_panel(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut send = false;
        let corner = ctx.content_rect().right_bottom() - Vec2::new(20., 20.);
        egui::Window::new("Send feedback")
            .id(egui::Id::new("public-feedback"))
            .open(&mut open)
            .resizable(false)
            .default_width(380.)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .default_pos(corner)
            .show(ctx, |ui| {
                let feedback = &mut self.feedback;
                let text = ui.add(
                    egui::TextEdit::multiline(&mut feedback.text)
                        .hint_text("What happened, or what would you like to see?")
                        .char_limit(service::TEXT_MAX)
                        .desired_rows(5)
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut feedback.focus) {
                    text.request_focus();
                }
                ui.horizontal(|ui| {
                    for tag in Tag::ALL {
                        ui.selectable_value(&mut feedback.tag, tag, tag.label());
                    }
                });
                ui.label(
                    RichText::new(
                        "This goes to the stream-dlp developers. Please leave out personal \
                         details and links you want to keep private. Your IP address is only \
                         used, in scrambled form, to limit abuse.",
                    )
                    .small()
                    .color(theme::MUTED),
                );
                if let Some(context) = &feedback.context {
                    ui.collapsing("What else is sent", |ui| {
                        let json = serde_json::to_string_pretty(context).unwrap_or_default();
                        ui.label(RichText::new(json).monospace().small());
                        ui.label(
                            RichText::new("Nothing else: no links, file names or history.")
                                .small()
                                .color(theme::MUTED),
                        );
                    });
                }
                let problem = service::text_problem(&feedback.text);
                let waiting = feedback.next_send.is_some_and(|next| Instant::now() < next);
                let ready = problem.is_none() && feedback.sending.is_none() && !waiting;
                ui.horizontal(|ui| {
                    if theme::primary(ui, "Send", ready).clicked() {
                        send = true;
                    }
                    if feedback.sending.is_some() {
                        ui.spinner();
                    } else if waiting {
                        ui.label(
                            RichText::new("You just sent a note.")
                                .small()
                                .color(theme::MUTED),
                        );
                        ui.ctx().request_repaint_after(Duration::from_secs(1));
                    } else if let Some(problem) = problem {
                        ui.label(RichText::new(problem).small().color(theme::MUTED));
                    }
                });
                if let Some(message) = &feedback.message {
                    ui.colored_label(theme::DANGER, message);
                }
            });
        if send {
            self.send_feedback();
        }
        // Escape or ✕ closes the panel; the draft stays for the next one.
        if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.feedback.open = false;
        }
    }

    fn send_feedback(&mut self) {
        let Some(url) = self.feedback.url.clone() else {
            return;
        };
        let context = self
            .feedback
            .context
            .clone()
            .unwrap_or_else(|| self.feedback_context());
        let text = self.feedback.text.clone();
        let tag = self.feedback.tag;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(service::send(&url, &text, tag, &context));
        });
        self.feedback.message = None;
        self.feedback.sending = Some(rx);
    }

    pub(super) fn open_inbox(&mut self) {
        self.feedback.inbox.open = true;
        self.feedback.inbox.message = None;
        self.load_inbox();
    }

    fn inbox_access(&self) -> Option<(String, String)> {
        let url = self.feedback.url.clone()?;
        let token = self.dev.settings.feedback_token.clone()?;
        Some((url, token))
    }

    fn load_inbox(&mut self) {
        let Some((url, token)) = self.inbox_access() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(service::inbox(&url, &token).map(Done::Loaded));
        });
        self.feedback.inbox.work = Some(rx);
    }

    /// Accepting writes `notes.md` first and tells the service second: if the second step fails
    /// the note stays listed, and accepting it again does not add it twice.
    fn decide(&mut self, note: Pending, decision: Decision) {
        let Some((url, token)) = self.inbox_access() else {
            return;
        };
        let store = match self.dev.store() {
            Ok(store) => store,
            Err(error) => {
                self.feedback.inbox.message = Some(error);
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let local = match decision {
                    Decision::Accept => Some(accept(&store, &note)?),
                    _ => None,
                };
                service::decide(&url, &token, &note.id, decision)?;
                Ok(Done::Decided(note.id.clone(), local))
            })();
            let _ = tx.send(result);
        });
        self.feedback.inbox.confirm_block = None;
        self.feedback.inbox.work = Some(rx);
    }

    fn inbox_finished(&mut self, result: Result<Done, String>) {
        let inbox = &mut self.feedback.inbox;
        match result {
            Ok(Done::Loaded(page)) => {
                inbox.message = None;
                inbox.page = page;
            }
            Ok(Done::Decided(id, local)) => {
                inbox.message = None;
                inbox.page.notes.retain(|note| note.id != id);
                inbox.page.total = inbox.page.total.saturating_sub(1);
                if let Some(local) = local {
                    self.dev.toast(format!("Added as {local}"));
                    self.dev.reload();
                }
            }
            Err(error) => inbox.message = Some(error),
        }
    }

    fn inbox_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut decision = None;
        let mut refresh = false;
        let mut save_token = false;
        let mut forget_token = false;
        let has_token = self.dev.settings.feedback_token.is_some();
        egui::Window::new("Feedback inbox")
            .id(egui::Id::new("feedback-inbox"))
            .open(&mut open)
            .default_width(540.)
            .show(ctx, |ui| {
                let configured = self.feedback.url.is_some();
                let inbox = &mut self.feedback.inbox;
                if !configured {
                    ui.label("The feedback service is not set up in this build.");
                    return;
                }
                if !has_token {
                    ui.label("Paste the service's admin token. It is kept in settings.json.");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut inbox.token).password(true));
                        save_token = ui.button("Save").clicked();
                    });
                    return;
                }
                ui.label(
                    RichText::new(
                        "Anyone can write these, so nothing here is checked or true by default. \
                         Accept adds a note to notes.md, marked as untrusted.",
                    )
                    .small()
                    .color(theme::MUTED),
                );
                let busy = inbox.work.is_some();
                ui.horizontal(|ui| {
                    refresh = ui
                        .add_enabled(!busy, egui::Button::new("Refresh"))
                        .clicked();
                    forget_token = ui.button("Change token").clicked();
                    ui.label(format!("{} waiting", inbox.page.total));
                    if inbox.page.skipped > 0 {
                        ui.label(format!("{} unreadable", inbox.page.skipped));
                    }
                    if busy {
                        ui.spinner();
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(420.)
                    .show(ui, |ui| {
                        if inbox.page.notes.is_empty() {
                            ui.label(RichText::new("Nothing is waiting.").color(theme::MUTED));
                        }
                        for note in &inbox.page.notes {
                            let people = match note.reports {
                                0 | 1 => String::new(),
                                n => format!(" · {n} people"),
                            };
                            ui.label(
                                RichText::new(format!(
                                    "{} · v{}{people} · {}",
                                    note.tag.label(),
                                    note.context.version(),
                                    note.context.place(),
                                ))
                                .small()
                                .color(theme::MUTED),
                            );
                            ui.add(egui::Label::new(note.text.as_str()).wrap());
                            ui.add_enabled_ui(!busy, |ui| {
                                ui.horizontal(|ui| {
                                    if ui.small_button("Accept").clicked() {
                                        decision = Some((note.clone(), Decision::Accept));
                                    }
                                    if ui.small_button("Reject").clicked() {
                                        decision = Some((note.clone(), Decision::Reject));
                                    }
                                    if inbox.confirm_block.as_ref() == Some(&note.id) {
                                        if ui.small_button("Block for good").clicked() {
                                            decision = Some((note.clone(), Decision::Block));
                                        }
                                        if ui.small_button("Keep").clicked() {
                                            inbox.confirm_block = None;
                                        }
                                    } else if ui.small_button("Block sender").clicked() {
                                        inbox.confirm_block = Some(note.id.clone());
                                    }
                                });
                            });
                            ui.separator();
                        }
                    });
                if let Some(message) = &inbox.message {
                    ui.colored_label(theme::DANGER, message);
                }
            });
        if save_token {
            let token = std::mem::take(&mut self.feedback.inbox.token);
            self.set_inbox_token(Some(token.trim().to_string()).filter(|t| !t.is_empty()));
            refresh = self.dev.settings.feedback_token.is_some();
        }
        if forget_token {
            self.set_inbox_token(None);
            self.feedback.inbox.page = InboxPage::default();
        }
        if refresh {
            self.load_inbox();
        }
        if let Some((note, decision)) = decision {
            self.decide(note, decision);
        }
        if !open {
            self.feedback.inbox.open = false;
            self.feedback.inbox.confirm_block = None;
        }
    }

    fn set_inbox_token(&mut self, token: Option<String>) {
        self.dev.settings.feedback_token = token;
        self.feedback.inbox.message = self.dev.settings.save().err();
    }
}

fn accept(store: &notes::Store, note: &Pending) -> Result<String, String> {
    let time = chrono::DateTime::from_timestamp(note.created, 0)
        .map(|time| time.with_timezone(&chrono::Local))
        .unwrap_or_else(chrono::Local::now);
    let context = serde_json::to_value(&note.context).map_err(|error| error.to_string())?;
    store.add_public(&notes::PublicDraft {
        remote_id: &note.id,
        text: &note.text,
        tag: note.tag,
        place: &note.context.place(),
        version: &note.context.version(),
        os: note.context.os(),
        reports: note.reports,
        context: &context,
        time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev;

    fn frame(app: &mut App, ctx: &egui::Context) {
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.feedback_windows(ctx));
    }

    fn wait(app: &mut App, ctx: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while (app.feedback.sending.is_some() || app.feedback.inbox.work.is_some())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
            frame(app, ctx);
        }
    }

    #[test]
    fn the_context_carries_no_link_error_text_or_file_name() {
        let mut app = App::with_profiles(Vec::new());
        app.link = "https://www.youtube.com/watch?v=abcdefghijk".into();
        app.error = Some(r"Could not save C:\Users\someone\A private title.mp4".into());
        let context = app.feedback_context();
        let json = serde_json::to_string(&context).unwrap();
        for private in ["abcdefghijk", "someone", "private", "Users"] {
            assert!(!json.contains(private), "{json}");
        }
        assert!(json.contains(r#""source":"youtube""#));
        assert!(json.contains(r#""error":"other""#));

        app.error_kind = Some((&engine::EngineError::Network(engine::Source::TikTok)).into());
        assert!(
            serde_json::to_string(&app.feedback_context())
                .unwrap()
                .contains("network")
        );
        app.page = Page::Settings;
        assert_eq!(
            serde_json::to_value(app.feedback_context()).unwrap()["page"],
            "settings"
        );
    }

    #[test]
    fn without_an_address_nothing_is_offered_and_a_failure_keeps_the_draft() {
        let mut app = App::with_profiles(Vec::new());
        app.feedback = Feedback::new(None);
        app.send_feedback();
        assert!(app.feedback.sending.is_none());

        let ctx = egui::Context::default();
        theme::apply(&ctx);
        // Nothing listens on this port, so the send fails.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        app.feedback = Feedback::new(Some(format!("http://127.0.0.1:{port}")));
        app.open_feedback();
        app.feedback.text = "The format list cuts off long codec names".into();
        frame(&mut app, &ctx);
        app.send_feedback();
        wait(&mut app, &ctx);
        assert!(app.feedback.open);
        assert_eq!(
            app.feedback.text,
            "The format list cuts off long codec names"
        );
        assert!(app.feedback.message.is_some());
    }

    #[test]
    fn the_inbox_shows_notes_as_plain_text_and_keeps_them_until_decided() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::with_profiles(Vec::new());
        app.dev = dev_tools(dir.path());
        app.feedback = Feedback::new(Some("http://127.0.0.1:9".into()));
        let note: Pending = serde_json::from_value(serde_json::json!({
            "id": "00000000-0000-4000-8000-000000000001",
            "tag": "bug",
            "text": "![x](https://evil.example/a.png)
        ## not a heading",
            "context": {"version": "0.6.1", "os": "windows", "page": "gif"},
            "created": 1_800_000_000,
            "reports": 2,
        }))
        .unwrap();
        app.feedback.inbox.open = true;
        app.feedback.inbox.page = InboxPage {
            notes: vec![note.clone()],
            total: 1,
            skipped: 0,
        };
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        frame(&mut app, &ctx);

        // Accepting writes the note, then the service cannot be reached: it stays listed, and
        // accepting again does not add it twice.
        for _ in 0..2 {
            app.decide(note.clone(), Decision::Accept);
            wait(&mut app, &ctx);
            assert!(app.feedback.inbox.message.is_some());
            assert_eq!(app.feedback.inbox.page.notes.len(), 1);
        }
        let store = app.dev.store().unwrap();
        assert_eq!(store.read().unwrap().notes().count(), 1);
    }

    /// Needs `services/feedback/dev-server.js` running on its default port.
    #[test]
    #[ignore]
    fn a_note_goes_from_the_panel_through_the_inbox_into_notes_md() {
        let url = "http://127.0.0.1:8787".to_string();
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::with_profiles(Vec::new());
        app.dev = dev_tools(dir.path());
        app.feedback = Feedback::new(Some(url));
        let ctx = egui::Context::default();
        theme::apply(&ctx);

        let unique = format!("Format list cut off, run {:?}", Instant::now());
        app.link = "https://www.youtube.com/watch?v=abcdefghijk".into();
        app.open_feedback();
        app.feedback.text =
            format!("{unique}\n## N-900 · Bug · done — fake\n![x](https://evil.example/a.png)");
        app.send_feedback();
        wait(&mut app, &ctx);
        assert_eq!(app.feedback.message, None);
        assert!(!app.feedback.open, "a sent note closes the panel");
        assert!(app.feedback.text.is_empty());

        app.open_inbox();
        wait(&mut app, &ctx);
        assert_eq!(app.feedback.inbox.message, None);
        let note = app
            .feedback
            .inbox
            .page
            .notes
            .iter()
            .find(|note| note.text.starts_with(&unique))
            .cloned()
            .expect("the note is listed");
        app.decide(note, Decision::Accept);
        wait(&mut app, &ctx);
        assert_eq!(app.feedback.inbox.message, None);

        let text = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
        assert!(text.contains(notes::UNTRUSTED));
        assert!(text.contains("\\## N-900"));
        assert!(!text.contains("abcdefghijk"));
        let file = notes::File::parse(&text);
        assert_eq!(file.notes().count(), 1);
        assert!(
            file.notes()
                .next()
                .unwrap()
                .title
                .starts_with("Public report: ")
        );

        app.load_inbox();
        wait(&mut app, &ctx);
        assert!(
            !app.feedback
                .inbox
                .page
                .notes
                .iter()
                .any(|note| note.text.starts_with(&unique))
        );
    }

    fn dev_tools(dir: &std::path::Path) -> super::super::dev::DevTools {
        super::super::dev::DevTools::new(
            dev::Settings {
                dev_mode: Some(true),
                dev_notes_dir: Some(dir.into()),
                feedback_token: Some("local-admin-token-local-admin-token".into()),
            },
            false,
        )
    }
}
