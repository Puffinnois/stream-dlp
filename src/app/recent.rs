//! Recent downloads: the section on My videos and the switch in Settings, while the person keeps
//! the optional list (`crate::history`). With the switch off, neither shows anything else.
use super::{
    App,
    theme::{
        ACCENT, ACCENT_TINT, DANGER, MUTED, RADIUS_M, SPACE_L, SPACE_M, TEXT_CAPTION, caption,
        card, checkbox, confirm, section_title,
    },
};
use crate::{feedback::Site, history};
use eframe::egui::{self, RichText};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

struct Row {
    entry: history::Entry,
    /// Present until a background check says otherwise. Checked when the page is opened or
    /// refreshed and when the list changes, never every frame and never on the UI thread, so a
    /// slow or unreachable drive cannot freeze the page.
    exists: bool,
}

/// Which saved files still exist, one result per row of the list the check was started for.
type Found = (u64, Vec<bool>);

pub struct Recent {
    pub on: bool,
    file: Option<PathBuf>,
    rows: Vec<Row>,
    /// A failed save or erase, shown quietly; it never fails a download.
    pub error: Option<String>,
    confirm_clear: bool,
    confirm_off: bool,
    /// The frame this section was last drawn in, to notice the page being opened again.
    last_pass: Option<u64>,
    /// Counts list changes; a check's result applies only to the list it was started for.
    generation: u64,
    /// The one check running, if any.
    check: Option<Receiver<Found>>,
    /// The list changed while a check ran: check again when it ends.
    recheck: bool,
    /// Repaints the page when a check ends; known once the section has been drawn.
    ctx: Option<egui::Context>,
}

impl Default for Recent {
    fn default() -> Self {
        Self::in_file(history::path())
    }
}

impl Recent {
    fn in_file(file: Option<PathBuf>) -> Self {
        Self {
            on: false,
            file,
            rows: Vec::new(),
            error: None,
            confirm_clear: false,
            confirm_off: false,
            last_pass: None,
            generation: 0,
            check: None,
            recheck: false,
            ctx: None,
        }
    }

    /// At launch, from the setting. While it is off no list exists, so one left behind is erased.
    pub fn start(&mut self, on: bool) {
        if on {
            self.turn_on();
        } else if let Some(file) = &self.file {
            let _ = history::clear(file);
        }
    }

    fn turn_on(&mut self) {
        self.on = true;
        self.reload();
    }

    /// Erases the list; the setting stays on if that fails, so the person can try again.
    fn turn_off(&mut self) -> Result<(), String> {
        if let Some(file) = &self.file {
            history::clear(file)?;
        }
        self.on = false;
        self.rows.clear();
        self.error = None;
        Ok(())
    }

    pub fn reload(&mut self) {
        let entries = self.file.as_deref().map(history::load).unwrap_or_default();
        self.set_rows(entries);
    }

    fn set_rows(&mut self, entries: Vec<history::Entry>) {
        self.rows = entries
            .into_iter()
            .map(|entry| Row {
                exists: true,
                entry,
            })
            .collect();
        self.generation += 1;
        self.start_check();
    }

    /// Checks the rows' files on one background worker. While one runs, the next waits for it.
    fn start_check(&mut self) {
        if self.check.is_some() {
            self.recheck = true;
            return;
        }
        self.recheck = false;
        if self.rows.is_empty() {
            return;
        }
        let paths: Vec<PathBuf> = self.rows.iter().map(|row| row.entry.path.clone()).collect();
        let (generation, ctx) = (self.generation, self.ctx.clone());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let found = paths.iter().map(|path| path.is_file()).collect();
            let _ = tx.send((generation, found));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        self.check = Some(rx);
    }

    /// Takes a finished check's result when one has arrived; the worker wakes the page for it.
    fn poll_check(&mut self) {
        let Some(rx) = &self.check else {
            return;
        };
        let found = match rx.try_recv() {
            Ok(found) => Some(found),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => None,
        };
        self.check = None;
        if let Some((generation, found)) = found {
            self.apply_check(generation, found);
        }
        if self.recheck {
            self.start_check();
        }
    }

    /// Applies a check to the rows it was started for; a result for an older list is dropped.
    fn apply_check(&mut self, generation: u64, found: Vec<bool>) -> bool {
        if generation != self.generation || found.len() != self.rows.len() {
            return false;
        }
        for (row, exists) in self.rows.iter_mut().zip(found) {
            row.exists = exists;
        }
        true
    }

    /// Notes a finished download while the list is kept.
    pub fn record(&mut self, path: PathBuf, title: &str, platform: Site) {
        if !self.on {
            return;
        }
        let Some(file) = self.file.clone() else {
            return;
        };
        self.apply(history::record(
            &file,
            history::Entry::new(path, title, platform),
        ));
    }

    fn remove(&mut self, path: &Path) {
        if let Some(file) = self.file.clone() {
            self.apply(history::remove(&file, path));
        }
    }

    fn clear(&mut self) {
        if let Some(file) = &self.file {
            match history::clear(file) {
                Ok(()) => {
                    self.rows.clear();
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    fn apply(&mut self, result: Result<Vec<history::Entry>, String>) {
        match result {
            Ok(entries) => {
                self.set_rows(entries);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// The section on My videos. Returns a file the person chose, which the page selects exactly
    /// as it selects an existing video.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        busy: bool,
        selected: Option<&Path>,
    ) -> Option<PathBuf> {
        if !self.on {
            return None;
        }
        if self.ctx.is_none() {
            self.ctx = Some(ui.ctx().clone());
        }
        let pass = ui.ctx().cumulative_pass_nr();
        if self.last_pass.is_none_or(|last| pass > last + 1) {
            self.reload();
        }
        self.last_pass = Some(pass);
        self.poll_check();
        let (mut chosen, mut removed) = (None, None);
        ui.add_space(SPACE_L);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, "Recent downloads");
                ui.label(caption(match self.rows.len() {
                    1 => "1 download".to_string(),
                    count => format!("{count} downloads"),
                }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !busy && !self.rows.is_empty(),
                            egui::Button::new("Clear all"),
                        )
                        .clicked()
                    {
                        self.confirm_clear = true;
                    }
                });
            });
            if self.rows.is_empty() {
                ui.add_space(SPACE_M);
                ui.label("Videos you download will appear here.");
                ui.add_space(SPACE_M);
            }
            let now = chrono::Local::now();
            egui::ScrollArea::vertical()
                .id_salt("recent_downloads")
                .max_height(330.)
                .show(ui, |ui| {
                    for row in &self.rows {
                        let entry = &row.entry;
                        let title = entry.display_title();
                        let folder = entry
                            .path
                            .parent()
                            .map(|folder| folder.display().to_string())
                            .unwrap_or_default();
                        ui.horizontal(|ui| {
                            if ui
                                .button("Remove")
                                .on_hover_text("Remove from this list; the file stays")
                                .clicked()
                            {
                                removed = Some(entry.path.clone());
                            }
                            ui.vertical(|ui| {
                                if row.exists {
                                    let mut button = egui::Button::new(&title)
                                        .corner_radius(RADIUS_M)
                                        .min_size(egui::vec2(ui.available_width(), 0.))
                                        .truncate();
                                    if selected == Some(entry.path.as_path()) {
                                        button = button
                                            .fill(ACCENT_TINT)
                                            .stroke(egui::Stroke::new(1., ACCENT));
                                    }
                                    if ui
                                        .add_enabled(!busy, button)
                                        .on_hover_text(&title)
                                        .clicked()
                                    {
                                        chosen = Some(entry.path.clone());
                                    }
                                    ui.label(caption(format!(
                                        "{} · {} · {folder}",
                                        entry.platform_label(),
                                        entry.when(now),
                                    )));
                                } else {
                                    ui.add(
                                        egui::Label::new(RichText::new(&title).color(MUTED))
                                            .truncate(),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "File moved or deleted · {} · {}",
                                            entry.platform_label(),
                                            entry.when(now),
                                        ))
                                        .size(TEXT_CAPTION)
                                        .color(MUTED)
                                        .italics(),
                                    );
                                }
                            });
                        });
                        ui.add_space(SPACE_M / 2.);
                    }
                });
            ui.label(caption(
                "This list stays on this computer. Removing an entry or clearing the list never deletes a video.",
            ));
            if let Some(error) = &self.error {
                ui.colored_label(DANGER, error);
            }
        });
        if let Some(path) = removed {
            self.remove(&path);
        }
        if self.confirm_clear {
            match confirm(
                ui.ctx(),
                "clear-recent-downloads",
                "Clear recent downloads?",
                "The list of recent downloads will be erased. The videos themselves are not deleted.",
                "Clear all",
            ) {
                Some(true) => {
                    self.confirm_clear = false;
                    self.clear();
                }
                Some(false) => self.confirm_clear = false,
                None => {}
            }
        }
        chosen
    }
}

impl App {
    /// The Settings card with the switch. Turning it off asks first, then erases the list.
    pub(super) fn history_settings(&mut self, ui: &mut egui::Ui) {
        ui.add_space(SPACE_L);
        card(ui, |ui| {
            section_title(ui, "Recent downloads");
            let mut on = self.library.recent.on;
            if checkbox(ui, &mut on, "Keep a list of recent downloads").changed() {
                if on {
                    self.set_history_setting(true);
                    self.library.recent.turn_on();
                } else {
                    self.library.recent.confirm_off = true;
                }
            }
            ui.label(caption(
                "Off by default. When on, My videos lists your last 50 downloads: where each file \
                 was saved, its title, the platform and when it finished. The list stays on this \
                 computer and never includes links or sign-in details.",
            ));
            if let Some(error) = &self.library.recent.error {
                ui.colored_label(DANGER, error);
            }
        });
        if self.library.recent.confirm_off {
            match confirm(
                ui.ctx(),
                "turn-off-recent-downloads",
                "Stop keeping recent downloads?",
                "The list of recent downloads will be erased. The videos themselves are not deleted.",
                "Turn off and erase",
            ) {
                Some(true) => {
                    self.library.recent.confirm_off = false;
                    self.turn_history_off();
                }
                Some(false) => self.library.recent.confirm_off = false,
                None => {}
            }
        }
    }

    fn turn_history_off(&mut self) {
        match self.library.recent.turn_off() {
            Ok(()) => self.set_history_setting(false),
            Err(error) => self.library.recent.error = Some(error),
        }
    }

    fn set_history_setting(&mut self, on: bool) {
        self.dev.settings.download_history = on;
        if let Err(error) = self.dev.settings.save() {
            crate::dev::errors::record("settings", &error);
            self.library.recent.error =
                Some("The setting could not be saved and applies until the app closes.".into());
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::{Page, Phase};
    use super::*;
    use crate::engine;

    pub(in crate::app) fn kept(dir: &Path) -> Recent {
        let mut recent = Recent::in_file(Some(dir.join(history::FILE)));
        recent.turn_on();
        recent.settle();
        recent
    }

    impl Recent {
        /// Waits for the background checks, as the page would over the next frames.
        fn settle(&mut self) {
            while let Some(rx) = &self.check {
                let found = rx.recv_timeout(std::time::Duration::from_secs(10));
                self.check = None;
                if let Ok((generation, found)) = found {
                    self.apply_check(generation, found);
                }
                if self.recheck {
                    self.start_check();
                }
            }
        }
    }

    #[test]
    fn rows_show_as_present_until_the_check_and_an_old_check_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("present.mp4");
        std::fs::write(&present, b"synthetic").unwrap();
        let mut recent = kept(dir.path());
        recent.record(present.clone(), "Present", Site::YouTube);
        recent.settle();
        assert!(recent.rows[0].exists);

        // The file goes away; Refresh lists it at once as present, then the check flips it.
        std::fs::remove_file(&present).unwrap();
        recent.reload();
        assert!(recent.rows[0].exists, "Shown before any check ends");
        recent.settle();
        assert!(!recent.rows[0].exists, "File moved or deleted");

        // A check started for the previous list never applies to the current one.
        let old = recent.generation;
        recent.record(dir.path().join("other.mp4"), "Other", Site::X);
        assert!(!recent.apply_check(old, vec![false, false]));
        assert!(recent.rows.iter().all(|row| row.exists));
        assert!(!recent.apply_check(recent.generation, vec![false]));
        recent.settle();
        assert_eq!(
            recent.rows.iter().map(|row| row.exists).collect::<Vec<_>>(),
            [false, false]
        );
    }

    #[test]
    fn a_list_change_during_a_check_waits_for_it_on_the_one_worker() {
        let dir = tempfile::tempdir().unwrap();
        let mut recent = kept(dir.path());
        // A check that has not ended yet, for the list before the change.
        let (tx, rx) = mpsc::channel();
        recent.check = Some(rx);
        let old = recent.generation;
        recent.record(dir.path().join("a.mp4"), "A", Site::TikTok);
        assert!(recent.recheck, "No second worker while one runs");
        assert!(recent.rows[0].exists);
        // It ends with a result for the old list: dropped, then the new list is checked.
        tx.send((old, Vec::new())).unwrap();
        recent.poll_check();
        assert!(recent.rows[0].exists);
        assert!(recent.check.is_some() && !recent.recheck);
        recent.settle();
        assert!(!recent.rows[0].exists);
    }

    #[test]
    fn nothing_is_recorded_while_off_and_turning_off_erases_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(history::FILE);
        let mut recent = Recent::in_file(Some(file.clone()));
        recent.start(false);
        recent.record(dir.path().join("a.mp4"), "A", Site::TikTok);
        assert!(!file.exists());

        recent.turn_on();
        recent.record(dir.path().join("a.mp4"), "A", Site::TikTok);
        recent.settle();
        assert!(file.exists());
        assert_eq!(recent.rows.len(), 1);
        assert!(
            !recent.rows[0].exists,
            "The synthetic file was never created"
        );

        recent.turn_off().unwrap();
        assert!(!file.exists());
        assert!(recent.rows.is_empty() && !recent.on);

        // A list left behind with the setting off is erased at launch.
        history::save(&file, &[history::Entry::new("b.mp4".into(), "B", Site::X)]).unwrap();
        Recent::in_file(Some(file.clone())).start(false);
        assert!(!file.exists());
    }

    #[test]
    fn rows_follow_remove_and_clear_and_keep_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("present.mp4");
        std::fs::write(&present, b"synthetic").unwrap();
        let mut recent = kept(dir.path());
        recent.record(dir.path().join("gone.mp4"), "Gone", Site::Other);
        recent.record(present.clone(), "Present", Site::YouTube);
        recent.settle();
        assert_eq!(recent.rows.len(), 2);
        assert!(recent.rows[0].exists && !recent.rows[1].exists);

        let mut again = kept(dir.path());
        assert_eq!(
            again.rows.len(),
            2,
            "A missing file stays listed after a reload"
        );
        again.remove(&present);
        assert_eq!(again.rows.len(), 1);
        assert!(present.exists(), "Removing an entry keeps the file");
        again.clear();
        assert!(again.rows.is_empty());
        assert!(!dir.path().join(history::FILE).exists());
    }

    #[test]
    fn a_failed_save_is_shown_and_the_download_still_completes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(history::FILE);
        std::fs::create_dir(&file).unwrap();
        let mut app = App::with_profiles(Vec::new());
        app.library.recent = Recent::in_file(Some(file));
        app.library.recent.on = true;
        app.phase = Phase::Downloading;
        app.tx.send(completed(dir.path())).unwrap();
        app.poll();
        assert!(app.phase == Phase::Done);
        assert!(app.error.is_none());
        assert!(app.library.recent.error.is_some());
    }

    const TITLE: &str = "Synthetic private title";
    const FILE_NAME: &str = "Synthetic private file.mp4";

    fn completed(dir: &Path) -> engine::Event {
        engine::Event::Complete(engine::CompletedDownload {
            path: dir.join(FILE_NAME),
            media: engine::MediaIdentity {
                entry_index: None,
                source: engine::Source::YouTube,
                source_url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
                id: Some("abcdefghijk".into()),
                extractor_key: Some("Youtube".into()),
            },
        })
    }

    #[test]
    fn a_recorded_download_stays_out_of_feedback_usage_and_notes() {
        let dir = tempfile::tempdir().unwrap();
        let usage_dir = tempfile::tempdir().unwrap();
        let mut app = App::with_profiles(Vec::new());
        app.library.recent = kept(dir.path());
        app.usage = crate::usage::Reporter::for_test(usage_dir.path());
        app.usage.answer(true);
        app.loaded_link = engine::parse_link("https://www.youtube.com/watch?v=abcdefghijk").ok();
        let mut video = engine::parse_video(
            r#"{"formats":[{"format_id":"18","vcodec":"avc1","acodec":"mp4a","height":360}]}"#,
        )
        .unwrap();
        video.title = TITLE.into();
        app.video = Some(video);
        app.phase = Phase::Downloading;
        app.tx.send(completed(dir.path())).unwrap();
        app.poll();

        let saved = std::fs::read_to_string(dir.path().join(history::FILE)).unwrap();
        assert!(saved.contains(TITLE) && saved.contains("private file"));
        for forbidden in ["abcdefghijk", "youtube.com", "Youtube\""] {
            assert!(!saved.contains(forbidden), "{saved}");
        }

        let ctx = egui::Context::default();
        let mut shared = vec![app.usage.waiting()];
        for page in [Page::Download, Page::Videos] {
            app.page = page;
            shared.push(serde_json::to_string(&app.feedback_context()).unwrap());
            shared.push(app.note_context(&ctx).1.to_string());
        }
        for text in shared {
            assert!(!text.contains("private"), "{text}");
        }
    }

    #[test]
    fn the_section_and_its_questions_draw_and_answer() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("present.mp4");
        std::fs::write(&present, b"synthetic").unwrap();
        let mut app = App::with_profiles(Vec::new());
        app.library.recent = kept(dir.path());
        app.library.recent.record(present, "Present", Site::YouTube);
        app.library
            .recent
            .record(dir.path().join("gone.mp4"), "", Site::Other);
        let ctx = egui::Context::default();
        super::super::theme::apply(&ctx);
        let frame = |app: &mut App| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.library.show(ui);
                    app.settings(ui);
                });
            });
        };
        frame(&mut app);
        app.library.recent.confirm_clear = true;
        app.library.recent.confirm_off = true;
        frame(&mut app);
        assert!(
            app.library.recent.confirm_clear,
            "A question waits for an answer"
        );
        assert_eq!(app.library.recent.rows.len(), 2);
    }
}
