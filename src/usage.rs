//! Usage reports from the Mac version, which is hard to test without Macs: with the person's
//! agreement, counts of how the app is working go to the service in `services/feedback/`. Every
//! value is a fixed word from the types here, so no link, title, path or message can be sent. Other
//! systems never ask or send. The UI is `app::usage`; see `docs/reference/usage-reports.md`.
use crate::feedback::{ErrorKind, Os, Page, Site};
use eframe::egui::Context;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, PoisonError, mpsc},
    time::{Duration, Instant},
};

const SAVED: &str = "usage.json";
/// Exists while the app runs: finding it at launch means the last session did not close normally.
const RUNNING: &str = "usage-running";
const CRASHES: &str = "usage-crashes.txt";
const CRASHES_MAX: usize = 5;
/// The service takes no more than these in one report.
const ENTRIES_MAX: usize = 32;
const COUNT_MAX: u32 = 1000;
const FIRST_SEND: Duration = Duration::from_secs(10);
const SEND_EVERY: Duration = Duration::from_secs(5 * 60);
const RETRY_AFTER: Duration = Duration::from_secs(15 * 60);
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where crashes are noted while reporting may be on; the panic hook has nothing else to go by.
static CRASH_LOG: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    First,
    Clean,
    Unclean,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Ok,
    Cancelled,
    Failed(ErrorKind),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Started(Start),
    Helper(&'static str, bool),
    Inspect(Site, Outcome),
    Download(Site, Outcome),
    Page(Page),
    FolderPicker {
        picked: bool,
    },
    CookieImport {
        ok: bool,
    },
    /// From [`crash_place`], never the panic's message.
    Crash(String),
}

/// One line of a report: an event and how many times it happened.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
    result: String,
    count: u32,
}

fn word(value: impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(word)) => word,
        _ => String::new(),
    }
}

impl Outcome {
    fn word(self) -> String {
        match self {
            Self::Ok => "ok".into(),
            Self::Cancelled => "cancelled".into(),
            Self::Failed(kind) => word(kind),
        }
    }
}

impl Event {
    fn entry(&self) -> Entry {
        let (kind, subject, result) = match self {
            Self::Started(start) => (
                "started",
                None,
                match start {
                    Start::First => "first",
                    Start::Clean => "clean",
                    Start::Unclean => "unclean",
                }
                .to_string(),
            ),
            Self::Helper(name, ok) => (
                "helper",
                Some(name.to_string()),
                if *ok { "ok" } else { "failed" }.to_string(),
            ),
            Self::Inspect(site, outcome) => ("inspect", Some(word(site)), outcome.word()),
            Self::Download(site, outcome) => ("download", Some(word(site)), outcome.word()),
            Self::Page(page) => ("page", Some(word(page)), "opened".to_string()),
            Self::FolderPicker { picked } => (
                "folder_picker",
                None,
                if *picked { "picked" } else { "dismissed" }.to_string(),
            ),
            Self::CookieImport { ok } => (
                "cookie_import",
                None,
                if *ok { "ok" } else { "failed" }.to_string(),
            ),
            Self::Crash(place) => ("crash", Some(place.clone()), "panic".to_string()),
        };
        Entry {
            kind: kind.into(),
            subject,
            result,
            count: 1,
        }
    }
}

/// Where in this app's code a panic happened. Any other location, such as a dependency's, is a path
/// on the machine that built the app, so it is reduced to one word; the message is never kept.
pub fn crash_place(file: &str, line: u32) -> String {
    let file = file.replace('\\', "/");
    let ours = file
        .strip_prefix("src/")
        .and_then(|rest| rest.strip_suffix(".rs"))
        .is_some_and(|rest| {
            rest.len() <= 80
                && !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_/".contains(&b))
        });
    if ours {
        format!("{file}:{line}")
    } else {
        "dependency".into()
    }
}

fn valid_crash_place(place: &str) -> bool {
    place == "dependency"
        || place
            .rsplit_once(':')
            .and_then(|(file, line)| Some(crash_place(file, line.parse().ok()?)))
            .is_some_and(|sanitized| sanitized == place)
}

/// Notes a panic's place on this machine, from the panic hook. It stays here until it is sent, and
/// is deleted if the person says no.
pub fn note_panic(file: &str, line: u32) {
    let log = CRASH_LOG
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if let Some(path) = log {
        append_crash(&path, &crash_place(file, line));
    }
}

fn append_crash(path: &Path, place: &str) {
    let mut lines = std::fs::read_to_string(path).unwrap_or_default();
    if lines.lines().count() < CRASHES_MAX {
        lines.push_str(&format!("{VERSION}\t{place}\n"));
        let _ = std::fs::write(path, lines);
    }
}

fn set_crash_log(path: Option<PathBuf>) {
    *CRASH_LOG.lock().unwrap_or_else(PoisonError::into_inner) = path;
}

/// `usage.json` in the data folder. What is waiting to be sent survives a crash or going offline.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sharing: Option<bool>,
    #[serde(default)]
    version: String,
    /// The bundled helpers are tried once per app version, since each run takes a moment.
    #[serde(default)]
    helpers_checked: bool,
    #[serde(default)]
    events: Vec<Entry>,
}

#[derive(Serialize)]
struct Report<'a> {
    version: &'a str,
    os: &'static str,
    os_version: String,
    chip: String,
    events: &'a [Entry],
}

/// The major macOS version from `sw_vers -productVersion`; `0` if it is not one.
fn major_version(output: &str) -> String {
    let major = output.trim().split('.').next().unwrap_or_default();
    if !major.is_empty() && major.len() <= 3 && major.bytes().all(|b| b.is_ascii_digit()) {
        major.into()
    } else {
        "0".into()
    }
}

/// The chip's generation from a brand like `Apple M1 Pro`: only `m1`, never the full model.
fn chip_family(brand: &str) -> String {
    brand
        .trim()
        .strip_prefix("Apple M")
        .map(|rest| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .filter(|digits| (1..=2).contains(&digits.len()))
        .map_or_else(|| "other".into(), |digits| format!("m{digits}"))
}

fn output_of(program: &str, args: &[&str]) -> String {
    crate::process::isolate(Command::new(program).args(args))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

fn machine() -> (String, String) {
    if !cfg!(target_os = "macos") {
        return ("0".into(), "other".into());
    }
    (
        major_version(&output_of("/usr/bin/sw_vers", &["-productVersion"])),
        chip_family(&output_of(
            "/usr/sbin/sysctl",
            &["-n", "machdep.cpu.brand_string"],
        )),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sent {
    Delivered,
    /// The service will never take this report, so it is dropped rather than tried forever.
    Refused,
    Later,
}

fn post(endpoint: &str, events: &[Entry]) -> Sent {
    let (os_version, chip) = machine();
    let report = Report {
        version: VERSION,
        os: Os::current().name(),
        os_version,
        chip,
        events,
    };
    let Ok(body) = serde_json::to_string(&report) else {
        return Sent::Refused;
    };
    let url = format!("{}/v1/usage", endpoint.trim_end_matches('/'));
    match crate::net::api_post(&url, None, Some(&body)) {
        Ok((202, _)) => Sent::Delivered,
        Ok((400 | 413 | 415, _)) => Sent::Refused,
        _ => Sent::Later,
    }
}

/// This copy's folder for reports, if it is one that reports. A debug build can name a folder to
/// try this on another system, without touching the real data folder.
fn folder() -> Option<PathBuf> {
    if cfg!(debug_assertions)
        && let Some(dir) = std::env::var_os("STREAM_DLP_USAGE_TEST_DIR")
        && !dir.is_empty()
    {
        return Some(dir.into());
    }
    if cfg!(target_os = "macos") {
        crate::dev::data_dir()
    } else {
        None
    }
}

/// Counts events, keeps them on disk once the person has agreed, and sends them in the background.
pub struct Reporter {
    place: Option<(String, PathBuf)>,
    saved: Saved,
    last_page: Option<Page>,
    closed_unexpectedly: bool,
    sending: Option<(mpsc::Receiver<Sent>, Vec<Entry>)>,
    helpers: Option<mpsc::Receiver<Vec<(&'static str, bool)>>>,
    next_send: Instant,
}

impl Reporter {
    /// Never asks and never sends: other systems, and builds without the service.
    pub fn off() -> Self {
        Self {
            place: None,
            saved: Saved::default(),
            last_page: None,
            closed_unexpectedly: false,
            sending: None,
            helpers: None,
            next_send: Instant::now(),
        }
    }

    #[cfg(test)]
    pub fn for_test(dir: &Path) -> Self {
        Self::in_folder("http://127.0.0.1:9".into(), dir.into())
    }

    pub fn new(endpoint: Option<String>) -> Self {
        match (endpoint, folder()) {
            (Some(endpoint), Some(dir)) => Self::in_folder(endpoint, dir),
            _ => Self::off(),
        }
    }

    fn in_folder(endpoint: String, dir: PathBuf) -> Self {
        let found: Option<Saved> = std::fs::read(dir.join(SAVED))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let first = found.is_none();
        let mut saved = found.unwrap_or_default();
        // Counts belong to the version that made them.
        if saved.version != VERSION {
            saved.version = VERSION.into();
            saved.events.clear();
            saved.helpers_checked = false;
        }
        let mut reporter = Self {
            place: Some((endpoint, dir)),
            saved,
            next_send: Instant::now() + FIRST_SEND,
            ..Self::off()
        };
        if reporter.saved.sharing != Some(false) {
            let unclean = reporter.path(RUNNING).is_some_and(|path| path.exists());
            reporter.closed_unexpectedly = unclean;
            reporter.mark_running();
            reporter.record(Event::Started(if first {
                Start::First
            } else if unclean {
                Start::Unclean
            } else {
                Start::Clean
            }));
            if reporter.saved.sharing == Some(true) {
                reporter.take_crashes();
                reporter.save();
            }
        }
        reporter
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.place.as_ref().map(|(_, dir)| dir.join(name))
    }

    fn mark_running(&self) {
        if let Some((_, dir)) = &self.place {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(dir.join(RUNNING), "");
            set_crash_log(Some(dir.join(CRASHES)));
        }
    }

    /// Moves the crashes noted on this machine into the report; those of other versions are gone.
    fn take_crashes(&mut self) {
        let Some(path) = self.path(CRASHES) else {
            return;
        };
        let lines = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        for line in lines.lines() {
            if let Some((version, place)) = line.split_once('\t')
                && version == VERSION
                && valid_crash_place(place)
            {
                self.record(Event::Crash(place.into()));
            }
        }
    }

    fn save(&self) {
        let Some(path) = self.path(SAVED) else {
            return;
        };
        if let Ok(json) = serde_json::to_vec_pretty(&self.saved) {
            let _ = crate::dev::write_atomically(&path, &json);
        }
    }

    /// Whether this copy is one that reports; the rest of the app shows nothing about it otherwise.
    pub fn available(&self) -> bool {
        self.place.is_some()
    }

    /// The person has not said yes or no yet.
    pub fn needs_answer(&self) -> bool {
        self.available() && self.saved.sharing.is_none()
    }

    pub fn sharing(&self) -> bool {
        self.available() && self.saved.sharing == Some(true)
    }

    /// Records the person's choice. Saying no deletes everything waiting on this machine.
    pub fn answer(&mut self, share: bool) {
        if !self.available() {
            return;
        }
        self.saved.sharing = Some(share);
        if share {
            self.mark_running();
            self.take_crashes();
            self.next_send = Instant::now() + FIRST_SEND;
        } else {
            self.saved.events.clear();
            self.sending = None;
            self.helpers = None;
            self.closed_unexpectedly = false;
            set_crash_log(None);
            for name in [RUNNING, CRASHES] {
                if let Some(path) = self.path(name) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        self.save();
    }

    /// Adds to the counts. Nothing is kept once the person has said no; before they answer the
    /// counts stay in memory.
    pub fn record(&mut self, event: Event) {
        if !self.available() || self.saved.sharing == Some(false) {
            return;
        }
        let new = event.entry();
        let same = |entry: &&mut Entry| {
            entry.kind == new.kind && entry.subject == new.subject && entry.result == new.result
        };
        if let Some(entry) = self.saved.events.iter_mut().find(same) {
            entry.count = (entry.count + 1).min(COUNT_MAX);
        } else if self.saved.events.len() < ENTRIES_MAX {
            self.saved.events.push(new);
        }
        if self.sharing() {
            self.save();
        }
    }

    /// Records a page once each time it is opened.
    pub fn page(&mut self, page: Page) {
        if self.last_page != Some(page) {
            self.last_page = Some(page);
            self.record(Event::Page(page));
        }
    }

    /// Whether the last session ended without closing normally, once: the app offers a note.
    pub fn take_closed_unexpectedly(&mut self) -> bool {
        if self.saved.sharing.is_none() {
            return false;
        }
        std::mem::take(&mut self.closed_unexpectedly)
    }

    /// The report as it would go out now, for the person to see.
    pub fn waiting(&self) -> String {
        serde_json::to_string_pretty(&self.saved.events).unwrap_or_default()
    }

    /// A normal exit: the next launch must not count this session as a crash.
    pub fn closed(&self) {
        if let Some(path) = self.path(RUNNING) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Takes finished background work and starts what is due. Runs every frame.
    pub fn tick(&mut self, ctx: &Context) {
        if !self.sharing() {
            return;
        }
        self.poll_helpers();
        self.poll_sending();
        if !self.saved.helpers_checked && self.helpers.is_none() {
            let (tx, rx) = mpsc::channel();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::tools::check_each());
                ctx.request_repaint();
            });
            self.helpers = Some(rx);
        }
        if self.sending.is_some() || self.saved.events.is_empty() {
            return;
        }
        let wait = self.next_send.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
            return;
        }
        let Some((endpoint, _)) = &self.place else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let (endpoint, events, ctx) = (endpoint.clone(), self.saved.events.clone(), ctx.clone());
        let batch = events.clone();
        std::thread::spawn(move || {
            let _ = tx.send(post(&endpoint, &events));
            ctx.request_repaint();
        });
        self.sending = Some((rx, batch));
    }

    fn poll_helpers(&mut self) {
        let Some(rx) = &self.helpers else {
            return;
        };
        match rx.try_recv() {
            Ok(results) => {
                self.helpers = None;
                self.saved.helpers_checked = true;
                for (name, ok) in results {
                    self.record(Event::Helper(name, ok));
                }
                self.save();
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.helpers = None,
        }
    }

    fn poll_sending(&mut self) {
        let Some((rx, _)) = &self.sending else {
            return;
        };
        match rx.try_recv() {
            Ok(sent) => {
                if let Some((_, batch)) = self.sending.take() {
                    self.finish_send(sent, &batch);
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.sending = None,
        }
    }

    /// Takes what went out off the counts, leaving what happened in the meantime.
    fn finish_send(&mut self, sent: Sent, batch: &[Entry]) {
        if sent == Sent::Later {
            self.next_send = Instant::now() + RETRY_AFTER;
            return;
        }
        self.next_send = Instant::now() + SEND_EVERY;
        for gone in batch {
            if let Some(entry) = self.saved.events.iter_mut().find(|entry| {
                entry.kind == gone.kind
                    && entry.subject == gone.subject
                    && entry.result == gone.result
            }) {
                entry.count = entry.count.saturating_sub(gone.count);
            }
        }
        self.saved.events.retain(|entry| entry.count > 0);
        self.save();
    }
}

/// One line of the developers' summary. Anyone can send reports, so these are data to read, not
/// facts, and a row that does not fit is left out.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub version: String,
    pub os: String,
    pub os_version: String,
    pub chip: String,
    pub kind: String,
    pub subject: String,
    pub result: String,
    pub count: u64,
}

impl Row {
    fn well_formed(&self) -> bool {
        [
            &self.version,
            &self.os,
            &self.os_version,
            &self.chip,
            &self.kind,
            &self.subject,
            &self.result,
        ]
        .iter()
        .all(|field| field.len() <= 100 && field.bytes().all(|b| b.is_ascii_graphic()))
    }
}

/// What Dev mode shows: the totals, and roughly how many different senders reported each day.
#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    pub days: u64,
    pub rows: Vec<Row>,
    pub senders_by_day: Vec<u64>,
}

fn parse_summary(body: &str) -> Result<Summary, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "The service sent an unreadable answer.")?;
    let rows = value["rows"]
        .as_array()
        .ok_or("The service sent an unreadable answer.")?
        .iter()
        .filter_map(|row| serde_json::from_value::<Row>(row.clone()).ok())
        .filter(Row::well_formed)
        .collect();
    let senders_by_day = value["reach"]
        .as_array()
        .map(|days| {
            days.iter()
                .filter_map(|day| day["senders"].as_u64())
                .collect()
        })
        .unwrap_or_default();
    Ok(Summary {
        days: value["days"].as_u64().unwrap_or_default(),
        rows,
        senders_by_day,
    })
}

pub fn summary(endpoint: &str, token: &str) -> Result<Summary, String> {
    let url = format!("{}/v1/admin/usage?days=30", endpoint.trim_end_matches('/'));
    match crate::net::api_get(&url, token)? {
        (200, body) => parse_summary(&body),
        (401, _) => Err("The service refused this token.".into()),
        (404, _) => Err("The deployed service has no usage reports yet.".into()),
        (status, _) => Err(format!("The service could not answer (HTTP {status}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reporter(dir: &Path) -> Reporter {
        Reporter::for_test(dir)
    }

    fn entries(reporter: &Reporter) -> Vec<(String, Option<String>, String, u32)> {
        reporter
            .saved
            .events
            .iter()
            .map(|e| (e.kind.clone(), e.subject.clone(), e.result.clone(), e.count))
            .collect()
    }

    #[test]
    fn events_become_only_fixed_words() {
        let entry = Event::Download(Site::YouTube, Outcome::Failed(ErrorKind::SignIn)).entry();
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({
                "kind": "download", "subject": "youtube", "result": "sign_in", "count": 1,
            })
        );
        let entry = Event::FolderPicker { picked: false }.entry();
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({"kind": "folder_picker", "result": "dismissed", "count": 1})
        );
        assert_eq!(Event::Page(Page::Gif).entry().subject.unwrap(), "gif");
    }

    #[test]
    fn a_crash_keeps_only_a_place_in_this_apps_code() {
        assert_eq!(crash_place("src\\app\\mod.rs", 7), "src/app/mod.rs:7");
        for path in [
            "C:\\Users\\someone\\.cargo\\registry\\src\\egui\\lib.rs",
            "/Users/someone/stream-dlp/src/main.rs",
            "/rustc/abc/library/core/src/option.rs",
            "src/../../secret.rs",
            "src/Someone Name.rs",
            "src/.rs",
        ] {
            assert_eq!(crash_place(path, 1), "dependency", "{path}");
        }
        assert!(valid_crash_place("src/app/mod.rs:7"));
        assert!(valid_crash_place("dependency"));
        assert!(!valid_crash_place("/Users/someone/video.mp4:7"));
        assert!(!valid_crash_place("src/app/mod.rs"));
        let here = std::panic::Location::caller();
        assert!(crash_place(here.file(), here.line()).starts_with("src/usage.rs:"));
    }

    #[test]
    fn machine_details_are_reduced_to_a_generation() {
        assert_eq!(major_version("15.1.1\n"), "15");
        assert_eq!(major_version("26\n"), "26");
        assert_eq!(major_version(""), "0");
        assert_eq!(major_version("Someone's Mac"), "0");
        assert_eq!(chip_family("Apple M1 Pro\n"), "m1");
        assert_eq!(chip_family("Apple M12"), "m12");
        assert_eq!(chip_family("Intel(R) Core(TM) i7"), "other");
        assert_eq!(chip_family("Apple M"), "other");
        assert_eq!(chip_family(""), "other");
    }

    #[test]
    fn other_systems_never_ask_or_keep_anything() {
        let mut off = Reporter::off();
        off.record(Event::Started(Start::First));
        off.answer(true);
        assert!(!off.available() && !off.needs_answer() && !off.sharing());
        assert!(off.saved.events.is_empty());
    }

    #[test]
    fn nothing_is_kept_on_disk_before_a_yes_and_no_deletes_everything() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = reporter(dir.path());
        assert!(first.needs_answer());
        first.record(Event::CookieImport { ok: true });
        assert!(!dir.path().join(SAVED).exists());
        assert_eq!(
            first.saved.events.len(),
            2,
            "Held in memory until they answer"
        );
        append_crash(&dir.path().join(CRASHES), "src/main.rs:1");

        first.answer(false);
        assert!(first.saved.events.is_empty());
        assert!(!dir.path().join(CRASHES).exists());
        assert!(!dir.path().join(RUNNING).exists());
        first.record(Event::CookieImport { ok: true });
        assert!(first.saved.events.is_empty());
        assert!(
            !std::fs::read_to_string(dir.path().join(SAVED))
                .unwrap()
                .contains("cookie_import")
        );

        let later = reporter(dir.path());
        assert!(!later.needs_answer() && !later.sharing());
        assert!(later.saved.events.is_empty());
        assert!(!dir.path().join(RUNNING).exists(), "No trace after a no");
    }

    #[test]
    fn a_yes_saves_counts_and_a_later_launch_tells_how_the_last_one_ended() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = reporter(dir.path());
        first.answer(true);
        first.record(Event::Download(Site::TikTok, Outcome::Ok));
        first.record(Event::Download(Site::TikTok, Outcome::Ok));
        assert_eq!(
            entries(&first),
            vec![
                ("started".into(), None, "first".into(), 1),
                ("download".into(), Some("tiktok".into()), "ok".into(), 2),
            ]
        );
        first.closed();

        let mut clean = reporter(dir.path());
        assert!(clean.sharing());
        assert!(!clean.take_closed_unexpectedly());
        assert!(
            entries(&clean).contains(&("started".into(), None, "clean".into(), 1)),
            "{:?}",
            entries(&clean)
        );
        assert_eq!(clean.saved.events[0].kind, "started");
        drop(clean);

        // The last one never called `closed`.
        let mut unclean = reporter(dir.path());
        assert!(entries(&unclean).contains(&("started".into(), None, "unclean".into(), 1)));
        assert!(unclean.take_closed_unexpectedly());
        assert!(!unclean.take_closed_unexpectedly(), "Offered once");
    }

    #[test]
    fn a_crash_before_the_answer_waits_for_a_yes() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = reporter(dir.path());
        append_crash(&dir.path().join(CRASHES), "src/main.rs:1");
        std::fs::write(
            dir.path().join(CRASHES),
            format!(
                "{}0.0.1\tsrc/old.rs:1\n{VERSION}\t/Users/someone/video.mp4:1\n",
                std::fs::read_to_string(dir.path().join(CRASHES)).unwrap()
            ),
        )
        .unwrap();
        assert!(!entries(&first).iter().any(|entry| entry.0 == "crash"));
        first.answer(true);
        let crashes: Vec<_> = entries(&first)
            .into_iter()
            .filter(|entry| entry.0 == "crash")
            .collect();
        assert_eq!(
            crashes,
            vec![(
                "crash".to_string(),
                Some("src/main.rs:1".to_string()),
                "panic".to_string(),
                1
            )]
        );
        assert!(!dir.path().join(CRASHES).exists());
    }

    #[test]
    fn crash_notes_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CRASHES);
        for line in 0..20 {
            append_crash(&path, &format!("src/main.rs:{line}"));
        }
        assert_eq!(
            std::fs::read_to_string(path).unwrap().lines().count(),
            CRASHES_MAX
        );
    }

    #[test]
    fn counts_of_another_version_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(SAVED),
            r#"{"sharing":true,"version":"0.0.1","helpers_checked":true,
                "events":[{"kind":"page","subject":"gif","result":"opened","count":3}]}"#,
        )
        .unwrap();
        let reporter = reporter(dir.path());
        assert!(reporter.sharing());
        assert!(!reporter.saved.helpers_checked);
        assert_eq!(reporter.saved.events.len(), 1);
        assert_eq!(reporter.saved.events[0].kind, "started");
    }

    #[test]
    fn reports_stay_within_what_the_service_takes() {
        let dir = tempfile::tempdir().unwrap();
        let mut reporter = reporter(dir.path());
        for line in 0..100 {
            reporter.record(Event::Crash(format!("src/main.rs:{line}")));
        }
        assert_eq!(reporter.saved.events.len(), ENTRIES_MAX);
        for _ in 0..1100 {
            reporter.record(Event::Started(Start::First));
        }
        assert_eq!(reporter.saved.events[0].count, COUNT_MAX);
    }

    #[test]
    fn a_page_counts_when_it_is_opened_not_every_frame() {
        let dir = tempfile::tempdir().unwrap();
        let mut reporter = reporter(dir.path());
        reporter.page(Page::Gif);
        reporter.page(Page::Gif);
        reporter.page(Page::Settings);
        reporter.page(Page::Gif);
        let gif = entries(&reporter)
            .into_iter()
            .find(|entry| entry.1.as_deref() == Some("gif"))
            .unwrap();
        assert_eq!(gif.3, 2);
    }

    #[test]
    fn what_went_out_comes_off_and_what_failed_stays() {
        let dir = tempfile::tempdir().unwrap();
        let mut reporter = reporter(dir.path());
        reporter.answer(true);
        let batch = reporter.saved.events.clone();
        reporter.finish_send(Sent::Later, &batch);
        assert_eq!(reporter.saved.events, batch);

        reporter.record(Event::Started(Start::First));
        reporter.finish_send(Sent::Delivered, &batch);
        assert_eq!(reporter.saved.events.len(), 1, "Happened during the send");
        assert_eq!(reporter.saved.events[0].count, 1);

        let batch = reporter.saved.events.clone();
        reporter.finish_send(Sent::Refused, &batch);
        assert!(reporter.saved.events.is_empty());
        assert!(
            std::fs::read_to_string(dir.path().join(SAVED))
                .unwrap()
                .contains("\"events\": []")
        );
    }

    #[test]
    fn the_summary_leaves_out_what_does_not_fit() {
        let good = serde_json::json!({
            "version": "0.7.1", "os": "macos", "os_version": "15", "chip": "m1",
            "kind": "download", "subject": "youtube", "result": "ok", "count": 4,
        });
        let mut extra = good.clone();
        extra["title"] = "A video".into();
        let mut odd = good.clone();
        odd["subject"] = "has spaces\u{202e}".into();
        let body = serde_json::json!({
            "days": 30,
            "reach": [{"day": 1, "senders": 3}, {"day": 2, "senders": 5}],
            "rows": [good, extra, odd, "text"],
        });
        let summary = parse_summary(&body.to_string()).unwrap();
        assert_eq!(summary.rows.len(), 1);
        assert_eq!(summary.rows[0].count, 4);
        assert_eq!(summary.senders_by_day, vec![3, 5]);
        assert!(parse_summary("not json").is_err());
    }

    /// Goes through the real local service; start it as `docs/reference/public-feedback.md` says.
    #[test]
    #[ignore = "needs the local feedback service on 127.0.0.1:8787"]
    fn a_report_reaches_the_local_service_and_the_summary() {
        let endpoint = "http://127.0.0.1:8787";
        let events = [Event::Download(Site::Facebook, Outcome::Failed(ErrorKind::Drm)).entry()];
        assert_eq!(post(endpoint, &events), Sent::Delivered);
        let summary = summary(endpoint, "local-admin-token-local-admin-token").unwrap();
        assert!(
            summary.rows.iter().any(|row| row.kind == "download"
                && row.subject == "facebook"
                && row.result == "drm")
        );
        assert!(!summary.senders_by_day.is_empty());
    }
}
