//! The optional list of recent downloads (off by default): `history.json` in the app data folder,
//! newest first. Each entry is exactly a file path, a title, the platform as its fixed word and the
//! completion time; no link, ID, host, format or session. A missing or damaged file is an empty
//! list. The UI is on the My videos page (`app::videos`); see `docs/reference/download-history.md`.
use crate::feedback::Site;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub const FILE: &str = "history.json";
pub const MAX_ENTRIES: usize = 50;
/// A larger file is not read: it was not written by this app.
const MAX_BYTES: u64 = 1_000_000;
/// What a failed write shows. It names no file, so no path reaches the page or a note.
const SAVE_FAILED: &str = "The list of recent downloads could not be saved. Your download is fine.";
const ERASE_FAILED: &str = "The list of recent downloads could not be erased.";

/// One finished download. Unknown fields in the file are ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub title: String,
    pub platform: Site,
    /// Seconds since the Unix epoch.
    pub completed: i64,
}

#[derive(Serialize, Deserialize)]
struct Saved<T> {
    entries: Vec<T>,
}

impl Entry {
    pub fn new(path: PathBuf, title: &str, platform: Site) -> Self {
        Self {
            path,
            title: title.trim().to_owned(),
            platform,
            completed: Local::now().timestamp(),
        }
    }

    /// The title, or the file name when the site gave none.
    pub fn display_title(&self) -> String {
        if self.title.is_empty() {
            self.path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            self.title.clone()
        }
    }

    /// The platform as the Download page names it.
    pub fn platform_label(&self) -> &'static str {
        use crate::engine::Source;
        match self.platform {
            Site::YouTube => Source::YouTube,
            Site::TikTok => Source::TikTok,
            Site::X => Source::X,
            Site::Facebook => Source::Facebook,
            Site::Instagram => Source::Instagram,
            Site::Other => Source::Other,
        }
        .label()
    }

    /// When it finished: "Just now", "5 minutes ago", "Yesterday, 14:05" or a short local date.
    pub fn when(&self, now: DateTime<Local>) -> String {
        let Some(then) = DateTime::from_timestamp(self.completed, 0) else {
            return "Unknown time".into();
        };
        let then = then.with_timezone(&Local);
        let seconds = (now - then).num_seconds();
        match seconds {
            ..60 => "Just now".into(),
            60..3600 => plural(seconds / 60, "minute"),
            _ if then.date_naive() == now.date_naive() => then.format("Today, %H:%M").to_string(),
            _ if now.date_naive().pred_opt() == Some(then.date_naive()) => {
                then.format("Yesterday, %H:%M").to_string()
            }
            _ => then.format("%Y-%m-%d %H:%M").to_string(),
        }
    }
}

fn plural(count: i64, unit: &str) -> String {
    match count {
        1 => format!("1 {unit} ago"),
        _ => format!("{count} {unit}s ago"),
    }
}

/// `history.json` in the app data folder.
pub fn path() -> Option<PathBuf> {
    crate::dev::data_dir().map(|dir| dir.join(FILE))
}

/// The list, newest first. Anything wrong with the file gives an empty list, and an entry that does
/// not read is left out, so a damaged file never stops the app.
pub fn load(path: &Path) -> Vec<Entry> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if file.take(MAX_BYTES + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 > MAX_BYTES {
        return Vec::new();
    }
    let Ok(saved) = serde_json::from_slice::<Saved<serde_json::Value>>(&bytes) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for value in saved.entries {
        if let Ok(entry) = serde_json::from_value::<Entry>(value)
            && !entries.iter().any(|known: &Entry| known.path == entry.path)
            && entries.len() < MAX_ENTRIES
        {
            entries.push(entry);
        }
    }
    entries
}

/// Writes the whole list through a temporary file, so a reader never sees half of it.
pub fn save(path: &Path, entries: &[Entry]) -> Result<(), String> {
    let saved = Saved {
        entries: entries.to_vec(),
    };
    let json = serde_json::to_vec_pretty(&saved).map_err(|_| SAVE_FAILED.to_string())?;
    crate::dev::write_atomically(path, &json).map_err(|_| SAVE_FAILED.to_string())
}

/// Puts a download at the top; one entry per path, at most [`MAX_ENTRIES`].
pub fn add(entries: &mut Vec<Entry>, entry: Entry) {
    entries.retain(|known| known.path != entry.path);
    entries.insert(0, entry);
    entries.truncate(MAX_ENTRIES);
}

/// Adds a download to the file and returns the new list.
pub fn record(path: &Path, entry: Entry) -> Result<Vec<Entry>, String> {
    let mut entries = load(path);
    add(&mut entries, entry);
    save(path, &entries)?;
    Ok(entries)
}

/// Takes one file off the list; the file itself is untouched.
pub fn remove(path: &Path, file: &Path) -> Result<Vec<Entry>, String> {
    let mut entries = load(path);
    entries.retain(|entry| entry.path != file);
    save(path, &entries)?;
    Ok(entries)
}

/// Deletes the list. No list is already erased.
pub fn clear(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(ERASE_FAILED.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, completed: i64) -> Entry {
        Entry {
            path: PathBuf::from(format!("videos/{name}.mp4")),
            title: format!("Synthetic {name}"),
            platform: Site::YouTube,
            completed,
        }
    }

    #[test]
    fn a_repeat_moves_to_the_top_and_the_list_is_capped() {
        let mut entries = Vec::new();
        for n in 0..60 {
            add(&mut entries, entry(&n.to_string(), n));
        }
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].completed, 59);
        assert_eq!(entries.last().unwrap().completed, 10, "The oldest go");
        add(&mut entries, entry("30", 100));
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].completed, 100);
        assert_eq!(entries.iter().filter(|e| e.completed == 30).count(), 0);
    }

    #[test]
    fn records_save_atomically_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join(FILE);
        assert!(load(&path).is_empty());
        record(&path, entry("a", 1)).unwrap();
        let entries = record(&path, entry("b", 2)).unwrap();
        assert_eq!(load(&path), entries);
        assert_eq!(entries[0].title, "Synthetic b");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|item| item.unwrap().file_name())
            .collect();
        assert_eq!(names, [FILE], "No temporary file is left behind");
    }

    #[test]
    fn a_damaged_oversized_or_missing_file_is_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        assert!(load(&path).is_empty());
        for bad in ["", "not json", "[1, 2]", r#"{"entries": 5}"#] {
            std::fs::write(&path, bad).unwrap();
            assert!(load(&path).is_empty(), "{bad}");
        }
        let mut big = br#"{"entries": [], "padding": ""#.to_vec();
        big.resize(MAX_BYTES as usize + 10, b'x');
        big.extend_from_slice(b"\"}");
        std::fs::write(&path, big).unwrap();
        assert!(load(&path).is_empty());
        // A damaged file is replaced by the next download.
        assert_eq!(record(&path, entry("a", 1)).unwrap().len(), 1);
        assert_eq!(load(&path).len(), 1);
    }

    #[test]
    fn unknown_fields_and_bad_entries_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        std::fs::write(
            &path,
            r#"{"version": 9, "entries": [
                {"path": "videos/a.mp4", "title": "A", "platform": "tiktok", "completed": 5, "extra": true},
                {"path": "videos/b.mp4", "title": "B", "platform": "nowhere", "completed": 5},
                {"title": "No path"},
                {"path": "videos/a.mp4", "title": "Again", "platform": "x", "completed": 1}
            ]}"#,
        )
        .unwrap();
        let entries = load(&path);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].platform, Site::TikTok);
        assert_eq!(entries[0].platform_label(), "TikTok");
    }

    #[test]
    fn remove_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        record(&path, entry("a", 1)).unwrap();
        record(&path, entry("b", 2)).unwrap();
        let entries = remove(&path, Path::new("videos/b.mp4")).unwrap();
        assert_eq!(entries, [entry("a", 1)]);
        assert_eq!(load(&path), entries);
        clear(&path).unwrap();
        assert!(!path.exists());
        clear(&path).unwrap();
    }

    #[test]
    fn entries_for_missing_files_survive_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let gone = Entry::new(dir.path().join("gone.mp4"), "Gone", Site::Other);
        record(&path, gone.clone()).unwrap();
        assert!(!gone.path.exists());
        assert_eq!(load(&path), [gone]);
    }

    #[test]
    fn an_entry_has_exactly_four_fields() {
        let value = serde_json::to_value(entry("a", 7)).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["completed", "path", "platform", "title"]);
        assert_eq!(value["platform"], "youtube");
        assert_eq!(value["completed"], 7);
    }

    #[test]
    fn errors_name_no_file() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be cannot be replaced or removed as a file.
        let path = dir.path().join(FILE);
        std::fs::create_dir(&path).unwrap();
        let error = record(&path, entry("a", 1)).unwrap_err();
        assert!(
            !error.contains(FILE) && !error.contains("videos"),
            "{error}"
        );
        assert!(clear(&path).is_err());
    }

    #[test]
    fn times_read_naturally() {
        let now = Local::now();
        let at = |seconds_ago: i64| Entry {
            completed: now.timestamp() - seconds_ago,
            ..entry("a", 0)
        };
        assert_eq!(at(5).when(now), "Just now");
        assert_eq!(at(60).when(now), "1 minute ago");
        assert_eq!(at(150).when(now), "2 minutes ago");
        assert!(at(400 * 86_400).when(now).starts_with(char::is_numeric));
        assert_eq!(
            Entry {
                completed: i64::MAX,
                ..entry("a", 0)
            }
            .when(now),
            "Unknown time"
        );
        assert_eq!(entry("a", 0).display_title(), "Synthetic a");
        let untitled = Entry {
            title: String::new(),
            ..entry("a", 0)
        };
        assert_eq!(untitled.display_title(), "a.mp4");
    }
}
