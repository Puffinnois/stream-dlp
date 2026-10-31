//! Dev mode's UI: Ctrl+Shift+D switches it, the amber DEV chip and its menu, the Note button,
//! the notes panel (Ctrl+Shift+N) and the list of notes. The notes file, the setting and the
//! recent errors are `crate::dev`; see `docs/reference/dev-notes.md`.
use super::{App, Page, Phase, theme};
use crate::dev::{
    self,
    notes::{self, Status, Tag},
};
use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, RichText, Vec2};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

const AMBER: Color32 = Color32::from_rgb(217, 119, 6);
/// "Include recent errors" covers this much time.
const ERRORS_WITHIN: Duration = Duration::from_secs(15 * 60);
const MAX_SHOTS: usize = 6;
const TOAST: Duration = Duration::from_millis(2500);
const TOGGLE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::D);
const NEW_NOTE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::N);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);

/// A screenshot hides the panel from the frame after the click until the image arrives.
#[derive(PartialEq)]
enum Capture {
    Idle,
    Requested,
    Waiting,
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    Open,
    Done,
    All,
}

enum Action {
    Status(String, Status),
    Copy(Vec<String>),
    Delete(String),
}

struct Shot {
    image: Arc<egui::ColorImage>,
    texture: egui::TextureHandle,
}

pub struct DevTools {
    pub on: bool,
    settings: dev::Settings,
    panel: bool,
    focus: bool,
    list: bool,
    text: String,
    tag: Tag,
    shots: Vec<Shot>,
    include_errors: bool,
    /// Where the app was when the panel opened: the summary line and the details.
    context: Option<(String, serde_json::Value)>,
    capture: Capture,
    saving: Option<mpsc::Receiver<Result<String, String>>>,
    notes: Vec<notes::Note>,
    filter: Filter,
    confirm_delete: Option<String>,
    message: Option<String>,
    toast: Option<(String, Instant)>,
    folder: Option<mpsc::Receiver<Option<PathBuf>>>,
}

impl DevTools {
    /// `flag` is `--dev-mode`: on for this launch, until it is switched.
    pub fn new(settings: dev::Settings, flag: bool) -> Self {
        Self {
            on: flag || settings.dev_mode(),
            settings,
            panel: false,
            focus: false,
            list: false,
            text: String::new(),
            tag: Tag::default(),
            shots: Vec::new(),
            include_errors: false,
            context: None,
            capture: Capture::Idle,
            saving: None,
            notes: Vec::new(),
            filter: Filter::Open,
            confirm_delete: None,
            message: None,
            toast: None,
            folder: None,
        }
    }

    fn store(&self) -> Result<notes::Store, String> {
        self.settings
            .notes_dir()
            .map(|dir| notes::Store { dir })
            .ok_or_else(|| "The app data folder is unknown.".into())
    }

    fn reload(&mut self) {
        match self.store().and_then(|store| store.read()) {
            Ok(file) => self.notes = file.notes().cloned().collect(),
            Err(error) => self.message = Some(error),
        }
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }
}

impl App {
    /// Shortcuts, screenshots and finished saves; runs before anything is drawn.
    pub(super) fn dev_input(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_shortcut(&TOGGLE)) {
            self.set_dev_mode(!self.dev.on);
        }
        if self.dev.on && ctx.input_mut(|i| i.consume_shortcut(&NEW_NOTE)) {
            self.open_note(ctx);
        }
        let images: Vec<Arc<egui::ColorImage>> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
                .collect()
        });
        if self.dev.capture == Capture::Waiting
            && let Some(image) = images.into_iter().last()
        {
            let texture = ctx.load_texture(
                format!("dev-shot-{}", self.dev.shots.len()),
                (*image).clone(),
                egui::TextureOptions::LINEAR,
            );
            self.dev.shots.push(Shot { image, texture });
            self.dev.capture = Capture::Idle;
        }
        if self.dev.capture == Capture::Requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.dev.capture = Capture::Waiting;
        }
        if self.dev.capture != Capture::Idle {
            ctx.request_repaint();
        }
        if let Some(rx) = &self.dev.saving {
            match rx.try_recv() {
                Ok(Ok(id)) => {
                    self.dev.saving = None;
                    self.dev.text.clear();
                    self.dev.shots.clear();
                    self.dev.tag = Tag::default();
                    self.dev.context = None;
                    self.dev.message = None;
                    self.dev.panel = false;
                    self.dev.toast(format!("Saved {id}"));
                    if self.dev.list {
                        self.dev.reload();
                    }
                }
                Ok(Err(error)) => {
                    self.dev.saving = None;
                    self.dev.message = Some(format!("Could not save the note: {error}"));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100))
                }
                Err(mpsc::TryRecvError::Disconnected) => self.dev.saving = None,
            }
        }
        if let Some(rx) = &self.dev.folder
            && let Ok(choice) = rx.try_recv()
        {
            self.dev.folder = None;
            if let Some(folder) = choice {
                self.dev.settings.dev_notes_dir = Some(folder);
                self.dev.message = self.dev.settings.save().err();
                self.dev.reload();
            }
        }
    }

    fn set_dev_mode(&mut self, on: bool) {
        self.dev.on = on;
        self.dev.settings.dev_mode = Some(on);
        let saved = self.dev.settings.save();
        if !on {
            self.dev.panel = false;
            self.dev.list = false;
        }
        self.dev.toast(match (on, saved) {
            (true, Ok(())) => "Dev mode on · Ctrl+Shift+N writes a note".into(),
            (false, Ok(())) => "Dev mode off · Ctrl+Shift+D turns it back on".into(),
            (_, Err(error)) => format!("Dev mode switched for this launch only: {error}"),
        });
    }

    fn open_note(&mut self, ctx: &egui::Context) {
        if self.dev.context.is_none() {
            self.dev.context = Some(self.note_context(ctx));
        }
        self.dev.include_errors = !dev::errors::recent(ERRORS_WITHIN).is_empty();
        self.dev.panel = true;
        self.dev.focus = true;
    }

    fn open_list(&mut self) {
        self.dev.list = true;
        self.dev.message = None;
        self.dev.reload();
    }

    fn open_notes_folder(&mut self) {
        let opened = self.dev.store().and_then(|store| {
            std::fs::create_dir_all(&store.dir)
                .and_then(|()| crate::platform::open_folder(&store.dir))
                .map_err(|error| format!("Cannot open {}: {error}", store.dir.display()))
        });
        if let Err(error) = opened {
            self.dev.toast(error);
        }
    }

    /// Where the app is: a summary line like `Download · youtube.com/watch?v=… · Failed`, and
    /// the details as JSON.
    fn note_context(&self, ctx: &egui::Context) -> (String, serde_json::Value) {
        let (page, parts, details) = match self.page {
            Page::Download => {
                let link = self.link.trim();
                let short = link
                    .trim_start_matches("https://")
                    .trim_start_matches("www.");
                let format = self.video.as_ref().and_then(|video| {
                    let format = video.formats.get(self.selected)?;
                    Some(format!("{} {}", format.quality(), format.ext))
                });
                let phase = match self.phase {
                    Phase::Idle => "idle",
                    Phase::Inspecting => "finding the video",
                    Phase::Choosing => "choosing a format",
                    Phase::Picking => "choosing a folder",
                    Phase::Downloading => "downloading",
                    Phase::Done => "done",
                };
                let mut parts: Vec<String> = Vec::new();
                if !short.is_empty() {
                    parts.push(cut(short, 60));
                }
                parts.extend(format.clone());
                parts.push(if self.error.is_some() {
                    "error".into()
                } else {
                    phase.into()
                });
                let log: Vec<&String> = self.log.iter().rev().take(10).rev().collect();
                let details = serde_json::json!({
                    "link": link,
                    "title": self.video.as_ref().map(|video| &video.title),
                    "format": format,
                    "phase": phase,
                    "status": self.status,
                    "error": self.error,
                    "session": self.session.is_some(),
                    "log": log,
                });
                ("Download", parts, details)
            }
            Page::Videos => {
                let (parts, details) = self.library.note_context();
                ("My videos", parts, details)
            }
            Page::Gif => {
                let (parts, details) = self.gif.note_context();
                ("GIF converter", parts, details)
            }
            Page::Settings => {
                let details = serde_json::json!({
                    "update": self.updater.available.as_ref().map(|update| &update.version),
                    "update_message": self.updater.message,
                    "tools": self.tool_update.versions,
                    "tools_status": self.tool_update.status,
                });
                ("Settings", Vec::new(), details)
            }
        };
        let place = std::iter::once(page.to_string())
            .chain(parts)
            .collect::<Vec<_>>()
            .join(" · ");
        let screen = ctx.content_rect();
        let context = serde_json::json!({
            "page": page,
            "details": details,
            "app": {
                "version": env!("CARGO_PKG_VERSION"),
                "build": if cfg!(debug_assertions) { "debug" } else { "release" },
                "os": std::env::consts::OS,
            },
            "window": {
                "width": screen.width().round(),
                "height": screen.height().round(),
                "scale": ctx.pixels_per_point(),
            },
            "time": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        });
        (place, context)
    }

    /// The amber DEV chip beside the version in the sidebar, with Dev mode's menu.
    pub(super) fn dev_chip(&mut self, ui: &mut egui::Ui) {
        if !self.dev.on {
            return;
        }
        let chip = egui::Button::new(
            RichText::new("DEV")
                .size(10.)
                .strong()
                .color(Color32::WHITE),
        )
        .small()
        .fill(AMBER)
        .stroke(egui::Stroke::NONE)
        .corner_radius(6);
        let mut choice = None;
        let (response, _) = egui::containers::menu::MenuButton::from_button(chip).ui(ui, |ui| {
            for (n, label) in [
                "New note  (Ctrl+Shift+N)",
                "All notes",
                "Open notes folder",
                "Turn off Dev mode  (Ctrl+Shift+D)",
            ]
            .into_iter()
            .enumerate()
            {
                if ui.button(label).clicked() {
                    choice = Some(n);
                }
            }
        });
        response.on_hover_text("Dev mode");
        match choice {
            Some(0) => self.open_note(ui.ctx()),
            Some(1) => self.open_list(),
            Some(2) => self.open_notes_folder(),
            Some(3) => self.set_dev_mode(false),
            _ => {}
        }
    }

    /// The Note button, the panel, the list and the toast; runs after the pages.
    pub(super) fn dev_windows(&mut self, ctx: &egui::Context) {
        if let Some((text, at)) = &self.dev.toast {
            if at.elapsed() < TOAST {
                egui::Area::new(egui::Id::new("dev-toast"))
                    .anchor(egui::Align2::CENTER_TOP, Vec2::new(94., 16.))
                    .order(egui::Order::Tooltip)
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(26, 29, 40))
                            .corner_radius(9)
                            .inner_margin(egui::Margin::symmetric(14, 8))
                            .show(ui, |ui| ui.label(RichText::new(text).color(Color32::WHITE)));
                    });
                ctx.request_repaint_after(TOAST.saturating_sub(at.elapsed()));
            } else {
                self.dev.toast = None;
            }
        }
        if !self.dev.on || self.dev.capture != Capture::Idle {
            return;
        }
        if !self.dev.panel {
            egui::Area::new(egui::Id::new("dev-note-button"))
                .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-20., -20.))
                .show(ctx, |ui| {
                    let button =
                        egui::Button::new(RichText::new("+ Note").color(Color32::WHITE).strong())
                            .fill(AMBER)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(9);
                    if ui
                        .add(button)
                        .on_hover_text("New dev note (Ctrl+Shift+N)")
                        .clicked()
                    {
                        self.open_note(ctx);
                    }
                });
        }
        if self.dev.panel {
            self.note_panel(ctx);
        }
        if self.dev.list {
            self.note_list(ctx);
        }
    }

    fn note_panel(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut recapture = false;
        let mut save = false;
        let errors = dev::errors::recent(ERRORS_WITHIN);
        let corner = ctx.content_rect().right_bottom() - Vec2::new(20., 20.);
        egui::Window::new("New dev note")
            .id(egui::Id::new("dev-note"))
            .open(&mut open)
            .resizable(false)
            .default_width(380.)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .default_pos(corner)
            .show(ctx, |ui| {
                let text = ui.add(
                    egui::TextEdit::multiline(&mut self.dev.text)
                        .hint_text("What did you notice? The first line is the title.")
                        .desired_rows(5)
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut self.dev.focus) {
                    text.request_focus();
                }
                ui.horizontal(|ui| {
                    for tag in Tag::ALL {
                        ui.selectable_value(&mut self.dev.tag, tag, tag.label());
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let mut remove = None;
                    for (n, shot) in self.dev.shots.iter().enumerate() {
                        ui.add(egui::Image::new(&shot.texture).max_size(Vec2::new(96., 64.)));
                        if ui.small_button("×").on_hover_text("Remove").clicked() {
                            remove = Some(n);
                        }
                    }
                    if let Some(n) = remove {
                        self.dev.shots.remove(n);
                    }
                    if ui
                        .add_enabled(
                            self.dev.shots.len() < MAX_SHOTS,
                            egui::Button::new("Screenshot"),
                        )
                        .on_hover_text("The whole window, without this panel")
                        .clicked()
                    {
                        self.dev.capture = Capture::Requested;
                        ctx.request_repaint();
                    }
                });
                if let Some((place, context)) = &self.dev.context {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(place).small().color(theme::MUTED));
                        if ui
                            .small_button("Recapture")
                            .on_hover_text("Record where the app is now")
                            .clicked()
                        {
                            recapture = true;
                        }
                    });
                    ui.collapsing("Context", |ui| {
                        let json = serde_json::to_string_pretty(context).unwrap_or_default();
                        egui::ScrollArea::vertical()
                            .max_height(160.)
                            .show(ui, |ui| ui.label(RichText::new(json).monospace().small()));
                    });
                }
                ui.add_enabled(
                    !errors.is_empty(),
                    egui::Checkbox::new(
                        &mut self.dev.include_errors,
                        format!("Include recent errors ({})", errors.len()),
                    ),
                );
                let ready = !self.dev.text.trim().is_empty() && self.dev.saving.is_none();
                ui.horizontal(|ui| {
                    let shortcut = ui.input_mut(|i| i.consume_shortcut(&SAVE));
                    if theme::primary(ui, "Save", ready).clicked() || (shortcut && ready) {
                        save = true;
                    }
                    ui.label(RichText::new("Ctrl+Enter").small().color(theme::MUTED));
                    if self.dev.saving.is_some() {
                        ui.spinner();
                    }
                });
                if let Some(message) = &self.dev.message {
                    ui.colored_label(Color32::from_rgb(180, 45, 45), message);
                }
            });
        if recapture {
            self.dev.context = Some(self.note_context(ctx));
        }
        if save {
            self.save_note(errors);
        }
        // Escape or ✕ closes the panel; the draft stays for the next one.
        if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.dev.panel = false;
        }
    }

    /// Saves in the background: encoding the screenshots takes a moment in a debug build.
    fn save_note(&mut self, errors: Vec<dev::errors::Recent>) {
        let store = match self.dev.store() {
            Ok(store) => store,
            Err(error) => {
                self.dev.message = Some(error);
                return;
            }
        };
        let (place, context) = self
            .dev
            .context
            .clone()
            .unwrap_or_else(|| (String::new(), serde_json::Value::Null));
        let errors = if self.dev.include_errors {
            errors
        } else {
            Vec::new()
        };
        let text = self.dev.text.clone();
        let tag = self.dev.tag;
        let images: Vec<_> = self
            .dev
            .shots
            .iter()
            .map(|shot| shot.image.clone())
            .collect();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let pngs: Result<Vec<Vec<u8>>, String> = images
                .iter()
                .map(|image| {
                    let [width, height] = image.size.map(|n| n as u32);
                    notes::png(width, height, image.as_raw())
                })
                .collect();
            let draft = notes::Draft {
                text: &text,
                tag,
                place: &place,
                context: &context,
                errors: &errors,
                time: chrono::Local::now(),
            };
            let _ = tx.send(pngs.and_then(|pngs| store.add(&draft, &pngs)));
        });
        self.dev.message = None;
        self.dev.saving = Some(rx);
    }

    fn note_list(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut action = None;
        let mut change_folder = false;
        let mut open_folder = false;
        egui::Window::new("Dev notes")
            .id(egui::Id::new("dev-notes"))
            .open(&mut open)
            .default_width(540.)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (filter, label) in [
                        (Filter::Open, "Open"),
                        (Filter::Done, "Done"),
                        (Filter::All, "All"),
                    ] {
                        ui.selectable_value(&mut self.dev.filter, filter, label);
                    }
                });
                let filter = self.dev.filter;
                let shown: Vec<&notes::Note> = self
                    .dev
                    .notes
                    .iter()
                    .rev()
                    .filter(|note| match filter {
                        Filter::Open => note.meta.status == Status::Open,
                        Filter::Done => note.meta.status == Status::Done,
                        Filter::All => true,
                    })
                    .collect();
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(360.)
                    .show(ui, |ui| {
                        if shown.is_empty() {
                            ui.label(RichText::new("No notes here.").color(theme::MUTED));
                        }
                        for note in shown {
                            let id = &note.meta.id;
                            ui.label(
                                RichText::new(format!(
                                    "{id} · {} · {}",
                                    note.meta.tag.label(),
                                    note.meta
                                        .created
                                        .get(..16)
                                        .unwrap_or_default()
                                        .replace('T', " ")
                                ))
                                .small()
                                .color(theme::MUTED),
                            );
                            ui.label(RichText::new(&note.title).strong());
                            if !note.place.is_empty() {
                                ui.label(RichText::new(&note.place).small().color(theme::MUTED));
                            }
                            ui.horizontal(|ui| {
                                let (label, status) = match note.meta.status {
                                    Status::Open => ("Mark done", Status::Done),
                                    Status::Done => ("Reopen", Status::Open),
                                };
                                if ui.small_button(label).clicked() {
                                    action = Some(Action::Status(id.clone(), status));
                                }
                                if ui.small_button("Copy for Claude").clicked() {
                                    action = Some(Action::Copy(vec![id.clone()]));
                                }
                                if self.dev.confirm_delete.as_ref() == Some(id) {
                                    if ui.small_button("Delete for good").clicked() {
                                        action = Some(Action::Delete(id.clone()));
                                    }
                                    if ui.small_button("Keep").clicked() {
                                        self.dev.confirm_delete = None;
                                    }
                                } else if ui.small_button("Delete").clicked() {
                                    self.dev.confirm_delete = Some(id.clone());
                                }
                            });
                            ui.separator();
                        }
                    });
                ui.horizontal(|ui| {
                    open_folder = ui.button("Open folder").clicked();
                    change_folder = ui
                        .add_enabled(
                            self.dev.folder.is_none(),
                            egui::Button::new("Change folder…"),
                        )
                        .clicked();
                    let open_ids: Vec<String> = self
                        .dev
                        .notes
                        .iter()
                        .filter(|note| note.meta.status == Status::Open)
                        .map(|note| note.meta.id.clone())
                        .collect();
                    if ui
                        .add_enabled(!open_ids.is_empty(), egui::Button::new("Copy all open"))
                        .clicked()
                    {
                        action = Some(Action::Copy(open_ids));
                    }
                });
                if let Some(dir) = self.dev.settings.notes_dir() {
                    ui.label(
                        RichText::new(dir.join("notes.md").display().to_string())
                            .small()
                            .color(theme::MUTED),
                    );
                }
                if let Some(message) = &self.dev.message {
                    ui.label(RichText::new(message).small());
                }
            });
        if open_folder {
            self.open_notes_folder();
        }
        if change_folder {
            let (tx, rx) = mpsc::channel();
            let start = self.dev.settings.notes_dir();
            std::thread::spawn(move || {
                let mut dialog = rfd::FileDialog::new();
                if let Some(start) = start.filter(|dir| dir.is_dir()) {
                    dialog = dialog.set_directory(start);
                }
                let _ = tx.send(dialog.pick_folder());
            });
            self.dev.folder = Some(rx);
        }
        if let Some(action) = action {
            self.dev_action(ctx, action);
        }
        if !open {
            self.dev.list = false;
            self.dev.confirm_delete = None;
        }
    }

    fn dev_action(&mut self, ctx: &egui::Context, action: Action) {
        let result = self.dev.store().and_then(|store| match action {
            Action::Status(id, status) => store.set_status(&id, status).map(|()| None),
            Action::Delete(id) => store.remove(&id).map(|()| Some(format!("Deleted {id}"))),
            Action::Copy(ids) => {
                let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
                let text = store.for_claude(&ids)?;
                ctx.copy_text(text);
                Ok(Some(format!(
                    "Copied {} for Claude",
                    if ids.len() == 1 {
                        ids[0].to_string()
                    } else {
                        format!("{} notes", ids.len())
                    }
                )))
            }
        });
        self.dev.confirm_delete = None;
        self.dev.message = match result {
            Ok(message) => message,
            Err(error) => Some(error),
        };
        self.dev.reload();
    }
}

/// `text` cut to `max` characters with an ellipsis.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    } else {
        text.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_mode_follows_the_flag_then_the_setting_then_the_build() {
        let set = |dev_mode| dev::Settings {
            dev_mode,
            dev_notes_dir: None,
        };
        assert!(DevTools::new(set(Some(false)), true).on);
        assert!(!DevTools::new(set(Some(false)), false).on);
        assert!(DevTools::new(set(Some(true)), false).on);
        assert_eq!(DevTools::new(set(None), false).on, cfg!(debug_assertions));
    }

    #[test]
    fn the_panel_attaches_a_screenshot_and_saves_the_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::with_profiles(Vec::new());
        let settings = dev::Settings {
            dev_mode: Some(true),
            dev_notes_dir: Some(dir.path().into()),
        };
        app.dev = DevTools::new(settings, false);
        let ctx = egui::Context::default();
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                app.dev_input(ctx);
                app.dev_windows(ctx);
            });
        };
        frame(&mut app, Vec::new());
        app.open_note(&ctx);
        app.dev.text = "Thumbnails flicker\nwhile scrolling".into();
        app.dev.capture = Capture::Requested;
        frame(&mut app, Vec::new());
        assert!(app.dev.capture == Capture::Waiting, "the command is sent");
        let image = Arc::new(egui::ColorImage::filled([4, 2], Color32::RED));
        let screenshot = egui::Event::Screenshot {
            viewport_id: egui::ViewportId::ROOT,
            user_data: Default::default(),
            image,
        };
        frame(&mut app, vec![screenshot]);
        assert!(app.dev.capture == Capture::Idle);
        assert_eq!(app.dev.shots.len(), 1);

        app.save_note(Vec::new());
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.dev.saving.is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            frame(&mut app, Vec::new());
        }
        assert!(!app.dev.panel, "a saved note closes the panel");
        assert!(app.dev.text.is_empty() && app.dev.shots.is_empty());
        let text = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
        assert!(text.contains("## N-001 · Idea · open — Thumbnails flicker\n"));
        assert!(text.contains("**Where:** Download · idle\n"));
        assert!(text.contains("![Screenshot 1](img/N-001-1.png)"));
        assert!(dir.path().join("img/N-001-1.png").is_file());

        app.open_list();
        assert_eq!(app.dev.notes.len(), 1);
        app.dev_action(&ctx, Action::Status("N-001".into(), Status::Done));
        assert_eq!(app.dev.notes[0].meta.status, Status::Done);
    }

    #[test]
    fn note_context_names_the_page_and_its_state() {
        let mut app = App::with_profiles(Vec::new());
        let ctx = egui::Context::default();
        app.link = "https://www.youtube.com/watch?v=abcdefghijk".into();
        app.error = Some("YouTube asks you to sign in.".into());
        let (place, context) = app.note_context(&ctx);
        assert_eq!(place, "Download · youtube.com/watch?v=abcdefghijk · error");
        assert_eq!(context["page"], "Download");
        assert_eq!(context["details"]["session"], false);
        assert_eq!(context["app"]["version"], env!("CARGO_PKG_VERSION"));
        app.page = Page::Settings;
        assert_eq!(app.note_context(&ctx).0, "Settings");
        assert_eq!(cut("abcdef", 4), "abc…");
    }
}
