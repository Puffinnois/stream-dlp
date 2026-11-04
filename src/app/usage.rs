//! Usage reports' UI: the question on first launch, the switch in Settings, the offer to write a
//! note after an unexpected close, and Dev mode's summary. The rest is `crate::usage`; see
//! `docs/reference/usage-reports.md`.
use super::{App, Page, Phase, theme};
use crate::usage::{self, Event, Outcome, Summary};
use eframe::egui::{self, Color32, RichText};
use std::{sync::mpsc, time::Duration};

const SENT: &[&str] = &[
    "App version, macOS version and chip generation, such as M1",
    "Whether the app started and closed normally, and whether its bundled tools run",
    "For each video lookup and download: the site, and whether it worked or what kind of error it hit",
    "Which pages you open, and whether the folder picker and cookie import worked",
    "After a crash: the place in the app's code where it happened",
];
const NEVER_SENT: &str = "Never sent: links, video titles, file names, folders, cookies or error \
    messages.";
const SENDER: &str = "Your IP address is only used, in scrambled form, to limit abuse and roughly \
    count how many people report each day. Totals are kept for a year.";

/// Dev mode's summary window.
#[derive(Default)]
pub struct UsageView {
    open: bool,
    summary: Summary,
    work: Option<mpsc::Receiver<Result<Summary, String>>>,
    message: Option<String>,
}

fn what_is_sent(ui: &mut egui::Ui) {
    for line in SENT {
        ui.label(format!("•  {line}"));
    }
    ui.add_space(theme::SPACE_XS);
    ui.label(theme::semibold(NEVER_SENT));
    ui.label(theme::caption(SENDER));
}

impl App {
    /// Runs every frame, after the pages.
    pub(super) fn usage_frame(&mut self, ctx: &egui::Context) {
        self.usage.tick(ctx);
        self.usage.page(match self.page {
            Page::Download => crate::feedback::Page::Download,
            Page::Videos => crate::feedback::Page::Videos,
            Page::Gif => crate::feedback::Page::Gif,
            Page::Settings => crate::feedback::Page::Settings,
        });
        self.poll_usage_view();
        if self.dev.capturing() {
            return;
        }
        if self.usage.needs_answer() {
            self.usage_question(ctx);
        } else if self.usage.take_closed_unexpectedly() {
            self.offer_note = true;
        }
        if self.offer_note {
            self.offer_note_window(ctx);
        }
        if self.usage_view.open {
            self.usage_view_window(ctx);
        }
    }

    /// How a lookup or a download that was running ended. Call it before changing the phase.
    pub(super) fn report_outcome(&mut self, outcome: Outcome) {
        let Some(site) = self.loaded_link.as_ref().map(|link| link.source.into()) else {
            return;
        };
        match self.phase {
            Phase::Inspecting => self.usage.record(Event::Inspect(site, outcome)),
            Phase::Downloading => self.usage.record(Event::Download(site, outcome)),
            _ => {}
        }
    }

    /// A question with two equal answers. It waits for one: no default, and closing it is no answer.
    fn usage_question(&mut self, ctx: &egui::Context) {
        let mut answer = None;
        egui::Modal::new(egui::Id::new("usage-question"))
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .corner_radius(theme::RADIUS_L)
                    .inner_margin(theme::SPACE_XL)
                    .stroke(egui::Stroke::new(1., theme::BORDER)),
            )
            .show(ctx, |ui| {
                ui.set_width(480.);
                ui.label(
                    theme::semibold("Help improve the Mac version?").size(theme::TEXT_HEADING),
                );
                ui.add_space(theme::SPACE_S);
                ui.label(
                    "stream-dlp is still experimental on Mac. If you agree, the app sends the \
                     developers short reports on how it is working, so problems can be found \
                     and fixed. You can change your mind at any time in Settings.",
                );
                ui.add_space(theme::SPACE_M);
                what_is_sent(ui);
                ui.add_space(theme::SPACE_L);
                ui.horizontal(|ui| {
                    if ui.button("Share reports").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("No thanks").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if let Some(share) = answer {
            self.usage.answer(share);
        }
    }

    /// The Settings page's card; only copies that report show it.
    pub(super) fn usage_settings(&mut self, ui: &mut egui::Ui) {
        if !self.usage.available() {
            return;
        }
        ui.add_space(theme::SPACE_L);
        theme::card(ui, |ui| {
            theme::section_title(ui, "Usage reports");
            let mut share = self.usage.sharing();
            if theme::checkbox(ui, &mut share, "Send usage reports to the developers").clicked() {
                self.usage.answer(share);
            }
            ui.label(theme::caption(
                "Turning this off deletes what is waiting to be sent.",
            ));
            egui::CollapsingHeader::new("What is sent").show(ui, |ui| {
                what_is_sent(ui);
                if self.usage.sharing() {
                    ui.add_space(theme::SPACE_S);
                    ui.label(theme::semibold("Waiting to be sent"));
                    ui.label(RichText::new(self.usage.waiting()).monospace().small());
                }
            });
        });
    }

    /// Reports cannot say what someone saw, so after a crash the app offers a note.
    fn offer_note_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut write = false;
        let mut later = false;
        egui::Window::new("Did something go wrong?")
            .id(egui::Id::new("usage-offer-note"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(360.)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    "stream-dlp may have closed unexpectedly last time. If you remember what \
                     you were doing, a short note would really help.",
                );
                ui.add_space(theme::SPACE_S);
                ui.horizontal(|ui| {
                    write = ui.button("Tell us what happened").clicked();
                    later = ui.button("Not now").clicked();
                });
            });
        if write {
            self.open_feedback();
        }
        if write || later || !open {
            self.offer_note = false;
        }
    }

    pub(super) fn open_usage_view(&mut self) {
        self.usage_view.open = true;
        self.load_usage_view();
    }

    fn load_usage_view(&mut self) {
        let (Some(url), Some(token)) = (
            self.feedback.service_url(),
            self.dev.settings.feedback_token.clone(),
        ) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(usage::summary(&url, &token));
        });
        self.usage_view.work = Some(rx);
        self.usage_view.message = None;
    }

    fn poll_usage_view(&mut self) {
        let view = &mut self.usage_view;
        let Some(rx) = &view.work else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(summary)) => {
                view.summary = summary;
                view.work = None;
            }
            Ok(Err(message)) => {
                view.message = Some(message);
                view.work = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => view.work = None,
        }
    }

    fn usage_view_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut refresh = false;
        let has_token = self.dev.settings.feedback_token.is_some();
        egui::Window::new("Usage reports")
            .id(egui::Id::new("usage-view"))
            .open(&mut open)
            .default_width(620.)
            .show(ctx, |ui| {
                let view = &self.usage_view;
                if !has_token {
                    ui.label("Save the service's admin token in the Feedback inbox first.");
                    return;
                }
                ui.label(theme::caption(
                    "Anyone can send reports, so read these as a guide, not as proof.",
                ));
                ui.horizontal(|ui| {
                    refresh = ui
                        .add_enabled(view.work.is_none(), egui::Button::new("Refresh"))
                        .clicked();
                    let reach = &view.summary.senders_by_day;
                    if let (Some(latest), Some(peak)) = (reach.last(), reach.iter().max()) {
                        ui.label(format!(
                            "Senders per day, roughly: {latest} latest, {peak} at most"
                        ));
                    }
                    if view.work.is_some() {
                        ui.spinner();
                        ctx.request_repaint_after(Duration::from_millis(100));
                    }
                });
                if let Some(message) = &view.message {
                    ui.colored_label(theme::DANGER, message);
                }
                ui.separator();
                if view.summary.rows.is_empty() {
                    ui.label(RichText::new("No reports yet.").color(theme::MUTED));
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(420.)
                    .show(ui, |ui| {
                        egui::Grid::new("usage-rows").striped(true).show(ui, |ui| {
                            for title in
                                ["Event", "Subject", "Result", "Count", "Version", "System"]
                            {
                                ui.label(theme::semibold(title));
                            }
                            ui.end_row();
                            for row in &view.summary.rows {
                                ui.label(&row.kind);
                                ui.label(&row.subject);
                                ui.label(&row.result);
                                ui.label(row.count.to_string());
                                ui.label(&row.version);
                                ui.label(format!("{} {} {}", row.os, row.os_version, row.chip));
                                ui.end_row();
                            }
                        });
                    });
            });
        if refresh {
            self.load_usage_view();
        }
        if !open {
            self.usage_view.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine, feedback::ErrorKind};
    use engine::Event as EngineEvent;

    #[test]
    fn a_copy_that_does_not_report_shows_and_records_nothing() {
        let mut app = App::with_profiles(Vec::new());
        assert!(!app.usage.available());
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| app.usage_frame(ctx));
        assert!(!app.offer_note);
        assert_eq!(app.usage.waiting(), "[]");
    }

    fn reporting_app(dir: &std::path::Path) -> App {
        let mut app = App::with_profiles(Vec::new());
        app.usage = usage::Reporter::for_test(dir);
        app.usage.answer(true);
        app.loaded_link = engine::parse_link("https://www.youtube.com/watch?v=abcdefghijk").ok();
        app
    }

    fn reported(app: &App, kind: &str, result: &str) -> bool {
        let waiting: serde_json::Value = serde_json::from_str(&app.usage.waiting()).unwrap();
        waiting
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == kind && entry["result"] == result)
    }

    #[test]
    fn a_download_reports_only_how_it_ended() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = reporting_app(dir.path());
        app.phase = Phase::Downloading;
        app.tx
            .send(EngineEvent::Failed(engine::EngineError::Network(
                engine::Source::YouTube,
            )))
            .unwrap();
        app.poll();
        assert!(reported(&app, "download", "network"));
        assert_eq!(app.error_kind, Some(ErrorKind::Network));

        app.phase = Phase::Inspecting;
        app.tx.send(EngineEvent::Cancelled).unwrap();
        app.poll();
        assert!(reported(&app, "inspect", "cancelled"));

        app.phase = Phase::Downloading;
        app.tx
            .send(EngineEvent::Complete(engine::CompletedDownload {
                path: "/Users/someone/secret-video.mp4".into(),
                media: engine::MediaIdentity {
                    entry_index: None,
                    source: engine::Source::YouTube,
                    source_url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
                    id: None,
                    extractor_key: None,
                },
            }))
            .unwrap();
        app.poll();
        assert!(reported(&app, "download", "ok"));
        assert!(!app.usage.waiting().contains("secret-video"));
        assert!(!app.usage.waiting().contains("abcdefghijk"));
    }

    #[test]
    fn the_folder_picker_reports_its_result() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = reporting_app(dir.path());
        app.phase = Phase::Picking;
        app.folder_tx.send(None).unwrap();
        app.poll();
        assert!(reported(&app, "folder_picker", "dismissed"));
    }
}
