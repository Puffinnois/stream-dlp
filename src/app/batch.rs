//! Downloading every video of a multi-video post, only when the user asks: one video after
//! another, each through the single-video download path, in one quality chosen for all.
use super::{
    App, Page, Phase,
    theme::{
        DANGER, SPACE_S, SPACE_XS, caption, card, primary, progress, section_title, semibold,
        success_line,
    },
};
use crate::usage::Outcome;
use crate::{engine, feedback::ErrorKind, platform, session::SessionCookies};
use eframe::egui;
use engine::{CompletedDownload, Event, Format, MediaIdentity, Source, Video};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
};

/// The most videos one batch saves: the cap Other sites already put on a page's list. A post
/// that lists more is not offered a batch.
const MOST: usize = 25;

/// Runs one video's download and sends its events: [`engine::download`], or a test's stand-in.
pub(crate) type Download =
    fn(MediaIdentity, Format, PathBuf, Option<SessionCookies>, Sender<Event>, Arc<AtomicBool>);

/// The quality chosen for every video of a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Quality {
    /// The format a single download selects first: the best video each entry offers.
    Best,
    /// The best video whose shorter side is at most this many lines.
    AtMost(u32),
}

impl Quality {
    const CHOICES: [Self; 4] = [
        Self::Best,
        Self::AtMost(1080),
        Self::AtMost(720),
        Self::AtMost(480),
    ];

    fn label(self) -> String {
        match self {
            Self::Best => "Best".into(),
            Self::AtMost(lines) => format!("{lines}p"),
        }
    }

    /// The index of the format of `video` to save. Below the chosen height, the closest one
    /// under it; when the entry offers none that small, its smallest. An entry whose videos
    /// report no resolution, or that has no video at all, gets its first format.
    pub(crate) fn pick(self, video: &Video) -> usize {
        let Self::AtMost(limit) = self else {
            return 0;
        };
        // A portrait 1080 × 1920 video is a 1080p video.
        let lines = |format: &Format| {
            let height = format.height.filter(|h| *h > 0)?;
            Some(
                format
                    .width
                    .filter(|w| *w > 0)
                    .map_or(height, |w| w.min(height)),
            )
        };
        let known = || {
            video
                .formats
                .iter()
                .enumerate()
                .filter(|(_, format)| format.video())
                .filter_map(|(index, format)| Some((index, lines(format)?)))
        };
        // The formats are sorted best first, so the first of equal sizes is the better one.
        known()
            .filter(|(_, lines)| *lines <= limit)
            .min_by_key(|(_, lines)| std::cmp::Reverse(*lines))
            .or_else(|| known().min_by_key(|(_, lines)| *lines))
            .map_or(0, |(index, _)| index)
    }
}

/// How one entry's download ended.
#[derive(Debug)]
pub(crate) enum Ended {
    Saved(CompletedDownload),
    /// With the message a single download of it would show.
    Failed {
        message: String,
        kind: ErrorKind,
    },
    Cancelled,
}

#[derive(Debug)]
pub(crate) struct Finished {
    /// The entry's position in the batch.
    pub(crate) entry: usize,
    pub(crate) ended: Ended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// Offered beside the chooser; nothing runs.
    Ready,
    /// Waiting for the folder.
    Picking,
    Running,
    /// Every entry ended, saved or failed.
    Done,
    /// Cancelled: the entries after the stopped one never started.
    Cancelled,
}

pub(crate) struct Batch {
    entries: Vec<Video>,
    pub(crate) quality: Quality,
    download: Download,
    cancel: Arc<AtomicBool>,
    folder: Option<PathBuf>,
    /// One per ended entry, in order; the next entry to start is the one after the last.
    results: Vec<Finished>,
    state: State,
}

impl Batch {
    /// A batch of exactly these entries, the ones the chooser lists. `None` for fewer than two
    /// or for more than a batch may save.
    pub(crate) fn new(entries: &[Video]) -> Option<Self> {
        (2..=MOST).contains(&entries.len()).then(|| Self {
            entries: entries.to_vec(),
            quality: Quality::Best,
            download: engine::download,
            cancel: Arc::new(AtomicBool::new(false)),
            folder: None,
            results: Vec::new(),
            state: State::Ready,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_download(mut self, download: Download) -> Self {
        self.download = download;
        self
    }

    pub(crate) fn state(&self) -> State {
        self.state
    }
    pub(crate) fn total(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn results(&self) -> &[Finished] {
        &self.results
    }
    pub(crate) fn entry(&self, index: usize) -> &Video {
        &self.entries[index]
    }
    /// The flag that stops the running download and the rest of the batch.
    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }
    /// Whether it has started: running or ended.
    pub(crate) fn started(&self) -> bool {
        matches!(self.state, State::Running | State::Done | State::Cancelled)
    }
    /// The running entry's position.
    pub(crate) fn running(&self) -> Option<usize> {
        (self.state == State::Running).then_some(self.results.len())
    }
    pub(crate) fn saved(&self) -> impl Iterator<Item = (usize, &CompletedDownload)> {
        self.results
            .iter()
            .filter_map(|finished| match &finished.ended {
                Ended::Saved(saved) => Some((finished.entry, saved)),
                _ => None,
            })
    }
    /// Every entry ended and none was saved.
    pub(crate) fn all_failed(&self) -> bool {
        self.state == State::Done && self.saved().next().is_none()
    }

    /// Starts the first entry, saving into `folder`.
    pub(crate) fn start(
        &mut self,
        folder: PathBuf,
        sessions: &HashMap<Source, SessionCookies>,
        tx: &Sender<Event>,
    ) {
        self.folder = Some(folder);
        self.results.clear();
        self.cancel = Arc::new(AtomicBool::new(false));
        self.state = State::Running;
        self.advance(sessions, tx);
    }

    /// Records how the running entry's download ended and starts the next one, unless the
    /// batch was cancelled. Events that do not end a download are ignored.
    pub(crate) fn ended(
        &mut self,
        event: Event,
        sessions: &HashMap<Source, SessionCookies>,
        tx: &Sender<Event>,
    ) {
        let Some(entry) = self.running() else {
            return;
        };
        let ended = match event {
            Event::Complete(saved) => match self.saved().find(|(_, s)| s.path == saved.path) {
                // yt-dlp never replaces a file, so a second entry with the same file name is
                // reported as the first one's file: that is not this entry saved.
                Some((first, _)) => Ended::Failed {
                    message: format!(
                        "It has the same file name as {}, which was kept: existing files are never replaced. Download it on its own into another folder.",
                        entry_name(&self.entries[first])
                    ),
                    kind: ErrorKind::Other,
                },
                None => Ended::Saved(saved),
            },
            Event::Failed(error) => Ended::Failed {
                message: error.user_message(),
                kind: (&error).into(),
            },
            Event::Cancelled => Ended::Cancelled,
            Event::Loaded(_) | Event::Entries(_) | Event::Line(_) => return,
        };
        let cancelled = matches!(ended, Ended::Cancelled);
        self.results.push(Finished { entry, ended });
        if cancelled {
            self.state = State::Cancelled;
        } else {
            self.advance(sessions, tx);
        }
    }

    /// Starts the next entry through the single-video download path, with its own site's
    /// session only, or ends the batch.
    fn advance(&mut self, sessions: &HashMap<Source, SessionCookies>, tx: &Sender<Event>) {
        let next = self.results.len();
        if self.cancel.load(Ordering::Relaxed) {
            self.state = State::Cancelled;
            return;
        }
        let (Some(video), Some(folder)) = (self.entries.get(next), self.folder.clone()) else {
            self.state = State::Done;
            return;
        };
        let format = video.formats[self.quality.pick(video)].clone();
        let media = video.identity();
        let session = sessions.get(&media.source).cloned();
        let (tx, cancel, download) = (tx.clone(), self.cancel.clone(), self.download);
        std::thread::spawn(move || download(media, format, folder, session, tx, cancel));
    }
}

/// How the chooser names an entry.
fn entry_name(video: &Video) -> String {
    format!("Video {}", video.entry_index.unwrap_or(1))
}

impl App {
    /// The download-all offer under the chooser's list.
    pub(super) fn batch_offer(&mut self, ui: &mut egui::Ui) {
        let busy = self.download_busy();
        let Some(batch) = &mut self.batch else {
            return;
        };
        ui.add_space(SPACE_S);
        ui.separator();
        ui.add_space(SPACE_S);
        ui.horizontal_wrapped(|ui| {
            ui.label(semibold("Or save them all in"));
            ui.add_enabled_ui(!busy, |ui| {
                for quality in Quality::CHOICES {
                    if ui
                        .selectable_label(batch.quality == quality, quality.label())
                        .clicked()
                    {
                        batch.quality = quality;
                    }
                }
            });
        });
        ui.label(caption(match batch.quality {
            Quality::Best => {
                "One after another, each in the best quality it offers.".into()
            }
            Quality::AtMost(lines) => format!(
                "One after another, each at {lines}p or the closest quality below; a video without one gets its smallest."
            ),
        }));
        ui.add_space(SPACE_XS);
        let total = batch.total();
        if primary(ui, &format!("Download all {total} videos"), !busy).clicked() {
            batch.state = State::Picking;
            self.choose_folder();
        }
    }

    /// The folder for a batch was chosen, or not.
    pub(super) fn batch_folder(&mut self, folder: Option<PathBuf>) {
        let Some(batch) = &mut self.batch else {
            return;
        };
        let Some(folder) = folder else {
            batch.state = State::Ready;
            self.phase = Phase::Choosing;
            return;
        };
        self.refresh_used = false;
        self.destination = Some(folder.clone());
        self.error = None;
        self.error_kind = None;
        self.saved = None;
        self.phase = Phase::Downloading;
        batch.start(folder, &self.sessions, &self.tx);
        // Closing the app and the Cancel button stop the batch, not only its current video.
        self.download_cancel = batch.cancel_flag();
        self.entry_started();
    }

    /// Whether a batch is waiting for its folder.
    pub(super) fn batch_picking(&self) -> bool {
        self.batch
            .as_ref()
            .is_some_and(|batch| batch.state() == State::Picking)
    }

    fn entry_started(&mut self) {
        if let Some(batch) = &self.batch
            && let Some(entry) = batch.running()
        {
            self.progress = 0.;
            self.status = super::download::connecting(batch.entry(entry).source);
            self.log.clear();
        }
    }

    /// Hands an engine event to a running batch. Gives it back when no batch runs or the
    /// event does not end a download.
    pub(super) fn batch_event(&mut self, event: Event) -> Option<Event> {
        if !self
            .batch
            .as_ref()
            .is_some_and(|b| b.state() == State::Running)
            || matches!(event, Event::Line(_) | Event::Loaded(_) | Event::Entries(_))
        {
            return Some(event);
        }
        // Usage counts each video as the download it is; nothing names the post.
        match &event {
            Event::Complete(_) => self.report_outcome(Outcome::Ok),
            Event::Failed(error) => {
                self.report_outcome(Outcome::Failed(error.into()));
                crate::dev::errors::record("download", format!("{error:?}"));
            }
            Event::Cancelled => self.report_outcome(Outcome::Cancelled),
            _ => {}
        }
        let batch = self.batch.as_mut()?;
        batch.ended(event, &self.sessions, &self.tx);
        if let Some(Finished {
            entry,
            ended: Ended::Saved(saved),
        }) = batch.results().last()
        {
            self.library.recent.record(
                saved.path.clone(),
                &batch.entries[*entry].title,
                saved.media.source.into(),
            );
        }
        let state = batch.state();
        match state {
            State::Running => self.entry_started(),
            State::Done | State::Cancelled => {
                self.phase = Phase::Choosing;
                self.progress = 1.;
                self.status.clear();
                if batch.all_failed() {
                    // A batch that saved nothing failed; the card lists each reason.
                    self.error_kind = batch.results().iter().find_map(|f| match f.ended {
                        Ended::Failed { kind, .. } => Some(kind),
                        _ => None,
                    });
                    self.error = Some(format!(
                        "None of the {} videos could be saved.",
                        batch.total()
                    ));
                }
            }
            State::Ready | State::Picking => {}
        }
        None
    }

    /// The running batch, or how it ended.
    pub(super) fn batch_card(&mut self, ui: &mut egui::Ui) {
        let Some(batch) = &self.batch else {
            return;
        };
        let total = batch.total();
        let mut cancel = false;
        let mut back = false;
        let mut gif = None;
        let mut upload = None;
        card(ui, |ui| {
            if let Some(entry) = batch.running() {
                section_title(ui, format!("Downloading all {total} videos"));
                ui.add_space(SPACE_XS);
                progress(
                    ui,
                    &format!("Video {} of {total}", entry + 1),
                    Some(self.progress),
                );
                let video = batch.entry(entry);
                ui.label(caption(format!("{} · {}", entry_name(video), video.title)));
                ui.horizontal(|ui| {
                    ui.label(&self.status);
                    if ui.small_button("Cancel").clicked() {
                        cancel = true;
                    }
                });
                ui.label(caption(
                    "Cancel stops this video and the rest; videos already saved are kept.",
                ));
                return;
            }
            let saved = batch.saved().count();
            if saved == total {
                success_line(ui, &format!("All {total} videos are saved"));
            } else if saved > 0 {
                success_line(ui, &format!("{saved} of {total} videos are saved"));
            }
            if batch.state() == State::Cancelled {
                let skipped = total - batch.results().len();
                ui.label(format!(
                    "Cancelled. {skipped} {} not started. Partial downloads can be resumed by choosing the same folder.",
                    if skipped == 1 { "video was" } else { "videos were" }
                ));
            }
            for finished in batch.results() {
                let video = batch.entry(finished.entry);
                ui.add_space(SPACE_XS);
                ui.label(semibold(format!("{} · {}", entry_name(video), video.title)));
                match &finished.ended {
                    Ended::Saved(saved) => {
                        ui.label(caption(saved.path.display().to_string()));
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!self.gif.busy, egui::Button::new("Make GIF").small())
                                .clicked()
                            {
                                gif = Some(saved.path.clone());
                            }
                            if ui.small_button("Upload on Streamable").clicked() {
                                upload = Some(saved.path.clone());
                            }
                        });
                    }
                    Ended::Failed { message, .. } => {
                        ui.colored_label(DANGER, message);
                    }
                    Ended::Cancelled => {
                        ui.label(caption("Cancelled"));
                    }
                }
            }
            if let Some(error) = &self.library.recent.error {
                ui.label(caption(error).color(DANGER));
            }
            ui.add_space(SPACE_S);
            ui.horizontal_wrapped(|ui| {
                if primary(ui, "Open folder", true).clicked()
                    && let Some(folder) = &self.destination
                {
                    let _ = platform::open_folder(folder);
                }
                if ui.button("Back to the videos in this post").clicked() {
                    back = true;
                }
            });
        });
        if cancel {
            self.download_cancel.store(true, Ordering::Relaxed);
            self.status = "Cancelling…".into();
        }
        if let Some(path) = gif {
            self.gif.select_file(ui.ctx(), path);
            self.page = Page::Gif;
        }
        if let Some(path) = upload {
            self.library.select_file(ui.ctx(), path);
            self.page = Page::Videos;
        }
        if back {
            let quality = self.batch.as_ref().map(|batch| batch.quality);
            self.batch = Batch::new(&self.entries);
            if let (Some(batch), Some(quality)) = (&mut self.batch, quality) {
                batch.quality = quality;
            }
            self.error = None;
            self.status.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    /// A stand-in download: reports which entry, format and session it was given, as a line,
    /// and leaves the ending to the test.
    fn started(
        media: MediaIdentity,
        format: Format,
        _folder: PathBuf,
        session: Option<SessionCookies>,
        tx: Sender<Event>,
        _cancel: Arc<AtomicBool>,
    ) {
        let _ = tx.send(Event::Line(format!(
            "{} {} {:?}",
            media.entry_index.unwrap_or(0),
            format.format_id,
            session.map(|s| s.source)
        )));
    }

    fn video(source: Source, index: usize, formats: serde_json::Value) -> Video {
        let mut video = engine::parse_video(
            &serde_json::json!({"id": format!("id{index}"), "formats": formats}).to_string(),
        )
        .unwrap();
        video.source = source;
        video.source_url = "https://www.youtube.com/watch?v=abcdefghijk".into();
        video.entry_index = Some(index);
        video.title = format!("Synthetic entry {index}");
        video
    }

    fn entries(source: Source, count: usize) -> Vec<Video> {
        (1..=count)
            .map(|index| {
                video(
                    source,
                    index,
                    serde_json::json!([
                        {"format_id": "hd", "vcodec": "avc1", "acodec": "mp4a", "height": 720, "width": 1280},
                        {"format_id": "sd", "vcodec": "avc1", "acodec": "mp4a", "height": 360, "width": 640}
                    ]),
                )
            })
            .collect()
    }

    fn saved(folder: &std::path::Path, index: usize) -> Event {
        Event::Complete(CompletedDownload {
            path: folder.join(format!("entry {index}.mp4")),
            media: MediaIdentity {
                source: Source::YouTube,
                source_url: String::new(),
                entry_index: Some(index),
                id: None,
                extractor_key: None,
            },
        })
    }

    fn failed() -> Event {
        Event::Failed(engine::EngineError::Unavailable(Source::YouTube))
    }

    struct Run {
        batch: Batch,
        tx: Sender<Event>,
        rx: mpsc::Receiver<Event>,
        sessions: HashMap<Source, SessionCookies>,
        folder: PathBuf,
    }

    impl Run {
        fn new(entries: &[Video]) -> Self {
            let (tx, rx) = mpsc::channel();
            Self {
                batch: Batch::new(entries).unwrap().with_download(started),
                tx,
                rx,
                sessions: HashMap::new(),
                folder: PathBuf::from("synthetic folder"),
            }
        }
        fn start(&mut self) -> String {
            self.batch
                .start(self.folder.clone(), &self.sessions, &self.tx);
            self.next_start()
        }
        /// The line the stand-in sent for the entry that started.
        fn next_start(&self) -> String {
            match self.rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Event::Line(line)) => line,
                _ => panic!("no entry started"),
            }
        }
        fn nothing_started(&self) -> bool {
            self.rx.recv_timeout(Duration::from_millis(200)).is_err()
        }
        fn end(&mut self, event: Event) {
            self.batch.ended(event, &self.sessions, &self.tx);
        }
    }

    #[test]
    fn entries_download_one_after_another_in_order() {
        let mut run = Run::new(&entries(Source::YouTube, 3));
        assert_eq!(run.start(), "1 hd None");
        assert!(run.nothing_started(), "Never two at once");
        for index in 1..=3 {
            assert_eq!(run.batch.running(), Some(index - 1));
            run.end(saved(&run.folder, index));
            if index < 3 {
                assert_eq!(run.next_start(), format!("{} hd None", index + 1));
            }
        }
        assert_eq!(run.batch.state(), State::Done);
        assert!(run.nothing_started());
        let order: Vec<_> = run.batch.saved().map(|(entry, _)| entry).collect();
        assert_eq!(order, [0, 1, 2]);
    }

    #[test]
    fn a_failure_in_the_middle_does_not_stop_the_batch() {
        let mut run = Run::new(&entries(Source::YouTube, 3));
        run.start();
        run.end(saved(&run.folder, 1));
        run.next_start();
        run.end(failed());
        assert_eq!(run.next_start(), "3 hd None");
        run.end(saved(&run.folder, 3));
        assert_eq!(run.batch.state(), State::Done);
        assert!(!run.batch.all_failed());
        let Ended::Failed { message, kind } = &run.batch.results()[1].ended else {
            panic!("expected the second entry to fail");
        };
        assert_eq!(
            *message,
            engine::EngineError::Unavailable(Source::YouTube).user_message()
        );
        assert_eq!(*kind, ErrorKind::Unavailable);
        assert_eq!(run.batch.saved().count(), 2);
    }

    #[test]
    fn a_batch_where_every_entry_failed_is_a_failure() {
        let mut app = App::with_profiles(Vec::new());
        app.loaded_link = engine::parse_link("https://www.youtube.com/watch?v=abcdefghijk").ok();
        app.entries = entries(Source::YouTube, 2);
        app.batch = Batch::new(&app.entries).map(|b| b.with_download(started));
        app.batch_folder(Some(PathBuf::from("synthetic folder")));
        assert!(app.phase == Phase::Downloading);
        for _ in 0..2 {
            app.tx.send(failed()).unwrap();
            app.poll();
        }
        assert!(app.phase == Phase::Choosing);
        assert!(app.batch.as_ref().unwrap().all_failed());
        assert_eq!(
            app.error.as_deref(),
            Some("None of the 2 videos could be saved.")
        );
        assert_eq!(app.error_kind, Some(ErrorKind::Unavailable));
    }

    #[test]
    fn cancel_during_an_entry_keeps_earlier_files_and_starts_no_later_one() {
        let mut app = App::with_profiles(Vec::new());
        app.entries = entries(Source::YouTube, 4);
        app.batch = Batch::new(&app.entries).map(|b| b.with_download(started));
        let folder = PathBuf::from("synthetic folder");
        app.batch_folder(Some(folder.clone()));
        app.tx.send(saved(&folder, 1)).unwrap();
        app.poll();
        assert_eq!(app.batch.as_ref().unwrap().running(), Some(1));
        // The Cancel button: the running download sees the flag and stops.
        app.download_cancel.store(true, Ordering::Relaxed);
        app.tx.send(Event::Cancelled).unwrap();
        app.poll();
        let batch = app.batch.as_ref().unwrap();
        assert_eq!(batch.state(), State::Cancelled);
        assert_eq!(batch.saved().count(), 1);
        assert_eq!(batch.results().len(), 2, "Entries 3 and 4 never started");
        assert!(matches!(batch.results()[1].ended, Ended::Cancelled));
        assert!(app.phase == Phase::Choosing && app.error.is_none());

        // Cancelled just as an entry finished: the file is kept and nothing else starts.
        let mut run = Run::new(&entries(Source::YouTube, 3));
        run.start();
        run.batch.cancel_flag().store(true, Ordering::Relaxed);
        run.end(saved(&run.folder, 1));
        assert_eq!(run.batch.state(), State::Cancelled);
        assert_eq!(run.batch.saved().count(), 1);
        assert!(run.nothing_started());
    }

    #[test]
    fn each_entry_falls_back_from_the_chosen_quality() {
        let tall = video(
            Source::YouTube,
            1,
            serde_json::json!([
                {"format_id": "2160", "vcodec": "vp9", "acodec": "none", "height": 2160, "width": 3840},
                {"format_id": "1080", "vcodec": "vp9", "acodec": "none", "height": 1080, "width": 1920},
                {"format_id": "720", "vcodec": "vp9", "acodec": "none", "height": 720, "width": 1280},
                {"format_id": "audio", "vcodec": "none", "acodec": "opus"}
            ]),
        );
        let small = video(
            Source::YouTube,
            2,
            serde_json::json!([
                {"format_id": "1440", "vcodec": "avc1", "acodec": "none", "height": 1440, "width": 2560},
                {"format_id": "900", "vcodec": "avc1", "acodec": "none", "height": 900, "width": 1600}
            ]),
        );
        // Portrait: 1080 × 1920 is a 1080p video.
        let portrait = video(
            Source::YouTube,
            3,
            serde_json::json!([
                {"format_id": "p1080", "vcodec": "h264", "acodec": "aac", "height": 1920, "width": 1080},
                {"format_id": "p720", "vcodec": "h264", "acodec": "aac", "height": 1280, "width": 720}
            ]),
        );
        let unknown = video(
            Source::YouTube,
            4,
            serde_json::json!([{"format_id": "direct", "vcodec": "h264", "acodec": "aac"}]),
        );
        let id =
            |video: &Video, quality: Quality| video.formats[quality.pick(video)].format_id.clone();
        assert_eq!(id(&tall, Quality::Best), "2160");
        assert_eq!(id(&tall, Quality::AtMost(1080)), "1080");
        assert_eq!(id(&tall, Quality::AtMost(480)), "720", "Its smallest");
        assert_eq!(
            id(&small, Quality::AtMost(1080)),
            "900",
            "The closest below"
        );
        assert_eq!(id(&small, Quality::AtMost(720)), "900");
        assert_eq!(id(&portrait, Quality::AtMost(1080)), "p1080");
        assert_eq!(id(&portrait, Quality::AtMost(720)), "p720");
        assert_eq!(id(&unknown, Quality::AtMost(720)), "direct");

        // In a batch, every entry gets its own fallback.
        let mut run = Run::new(&[tall, small]);
        run.batch.quality = Quality::AtMost(720);
        assert_eq!(run.start(), "1 720 None");
        run.end(failed());
        assert_eq!(run.next_start(), "2 900 None");
    }

    #[test]
    fn an_already_saved_file_is_never_replaced() {
        let mut run = Run::new(&entries(Source::YouTube, 3));
        run.start();
        run.end(saved(&run.folder, 1));
        run.next_start();
        // yt-dlp reports the first entry's existing file for an entry with the same name.
        run.end(saved(&run.folder, 1));
        run.next_start();
        run.end(saved(&run.folder, 3));
        let results = run.batch.results();
        let Ended::Failed { message, .. } = &results[1].ended else {
            panic!("A second entry with the first one's file was not saved");
        };
        assert!(message.contains("Video 1"), "{message}");
        assert!(message.contains("never replaced"), "{message}");
        assert_eq!(run.batch.saved().count(), 2);
    }

    #[test]
    fn a_batch_takes_exactly_the_listed_entries_and_at_most_25() {
        assert!(Batch::new(&entries(Source::Other, 1)).is_none());
        assert!(Batch::new(&entries(Source::Other, 26)).is_none());
        let mut run = Run::new(&entries(Source::Other, 25));
        assert_eq!(run.batch.total(), 25);
        run.start();
        for index in 1..=25 {
            run.end(failed());
            if index < 25 {
                run.next_start();
            }
        }
        assert_eq!(run.batch.state(), State::Done);
        assert_eq!(run.batch.results().len(), 25);
        assert!(run.nothing_started());
    }

    #[test]
    fn a_silent_entry_beside_one_with_sound_keeps_its_own_sound_decision() {
        let silent = video(
            Source::Instagram,
            1,
            serde_json::json!([{"format_id": "v", "vcodec": "avc1", "acodec": "none", "height": 720}]),
        );
        let split = video(
            Source::Instagram,
            2,
            serde_json::json!([
                {"format_id": "v", "vcodec": "avc1", "acodec": "none", "height": 720},
                {"format_id": "a", "vcodec": "none", "acodec": "mp4a"}
            ]),
        );
        let pick = |video: &Video| video.formats[Quality::Best.pick(video)].clone();
        assert_eq!(pick(&silent).sound(), engine::Sound::None);
        assert_eq!(pick(&silent).selector(), "v");
        assert_eq!(pick(&split).sound(), engine::Sound::Merged);
        assert!(pick(&split).selector().starts_with("v+"));
    }

    #[test]
    fn each_entry_gets_only_its_own_sites_session() {
        let mut youtube = Run::new(&entries(Source::YouTube, 2));
        youtube
            .sessions
            .insert(Source::YouTube, crate::session::fake_session());
        assert_eq!(youtube.start(), "1 hd Some(YouTube)");
        youtube.end(failed());
        assert_eq!(youtube.next_start(), "2 hd Some(YouTube)");

        // A YouTube session never reaches another site's entries, and Other sites get none.
        for source in [Source::TikTok, Source::Other] {
            let mut run = Run::new(&entries(source, 2));
            run.sessions = youtube.sessions.clone();
            assert_eq!(run.start(), "1 hd None");
            run.end(saved(&run.folder, 1));
            assert_eq!(run.next_start(), "2 hd None");
        }
    }

    #[test]
    fn each_saved_entry_is_recorded_in_recent_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::with_profiles(Vec::new());
        app.library.recent = super::super::recent::tests::kept(dir.path());
        app.entries = entries(Source::YouTube, 3);
        app.batch = Batch::new(&app.entries).map(|b| b.with_download(started));
        app.batch_folder(Some(dir.path().to_owned()));
        for event in [saved(dir.path(), 1), failed(), saved(dir.path(), 3)] {
            app.tx.send(event).unwrap();
            app.poll();
        }
        let list = std::fs::read_to_string(dir.path().join(crate::history::FILE)).unwrap();
        assert!(list.contains("Synthetic entry 1") && list.contains("entry 1.mp4"));
        assert!(
            !list.contains("Synthetic entry 2"),
            "A failed entry is not recorded"
        );
        assert!(list.contains("Synthetic entry 3") && list.contains("entry 3.mp4"));
        assert!(app.error.is_none());
    }

    #[test]
    fn the_offer_the_progress_and_the_result_draw() {
        let mut app = App::with_profiles(Vec::new());
        app.loaded_link = engine::parse_link("https://www.youtube.com/watch?v=abcdefghijk").ok();
        app.entries = entries(Source::YouTube, 3);
        app.phase = Phase::Choosing;
        app.batch = Batch::new(&app.entries).map(|b| b.with_download(started));
        let ctx = egui::Context::default();
        super::super::theme::apply(&ctx);
        let frame = |app: &mut App| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.content(ui));
            });
        };
        frame(&mut app);
        let folder = PathBuf::from("synthetic folder");
        app.batch_folder(Some(folder.clone()));
        frame(&mut app);
        for event in [saved(&folder, 1), failed(), Event::Cancelled] {
            app.tx.send(event).unwrap();
            app.poll();
            frame(&mut app);
        }
        assert_eq!(app.batch.as_ref().unwrap().state(), State::Cancelled);
        assert!(app.batch.as_ref().unwrap().started());
    }

    /// A real post or page through inspection and the batch, with the real download, at 480p
    /// or the closest each entry offers. Prints positions, outcomes and sizes only: no link,
    /// title or file name.
    #[test]
    #[ignore = "requires STREAM_DLP_BATCH_TEST_URL, a permitted post or page with 2 to 25 videos, internet and installed tools"]
    fn live_post_saves_every_listed_video() {
        let link = engine::parse_link(&std::env::var("STREAM_DLP_BATCH_TEST_URL").unwrap())
            .unwrap_or_else(|error| panic!("{error}"));
        println!("site: {:?}", link.source);
        let (tx, rx) = mpsc::channel();
        engine::inspect(link, None, tx.clone(), Arc::new(AtomicBool::new(false)));
        let entries = loop {
            match rx.recv_timeout(Duration::from_secs(200)).unwrap() {
                Event::Entries(entries) => break entries,
                Event::Loaded(_) => panic!("One video, not a post with several"),
                Event::Failed(error) => panic!("{}", error.user_message()),
                _ => {}
            }
        };
        println!("listed: {}", entries.len());
        let mut batch = Batch::new(&entries).expect("2 to 25 entries");
        batch.quality = Quality::AtMost(480);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-output");
        std::fs::create_dir_all(&root).unwrap();
        let folder = tempfile::tempdir_in(root).unwrap();
        let sessions = HashMap::new();
        batch.start(folder.path().to_owned(), &sessions, &tx);
        while batch.state() == State::Running {
            let event = rx.recv_timeout(Duration::from_secs(900)).unwrap();
            batch.ended(event, &sessions, &tx);
        }
        assert_eq!(batch.state(), State::Done);
        let mut paths = Vec::new();
        for finished in batch.results() {
            match &finished.ended {
                Ended::Saved(saved) => {
                    let size = std::fs::metadata(&saved.path).unwrap().len();
                    println!("video {}: saved, {size} bytes", finished.entry + 1);
                    assert!(size > 0);
                    paths.push(saved.path.clone());
                }
                Ended::Failed { message, .. } => {
                    println!("video {}: failed: {message}", finished.entry + 1)
                }
                Ended::Cancelled => println!("video {}: cancelled", finished.entry + 1),
            }
        }
        assert_eq!(paths.len(), batch.total(), "Every listed video is saved");
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), batch.total(), "Each in its own file");
    }
}
