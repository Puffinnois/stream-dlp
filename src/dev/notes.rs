//! The dev notes file, `notes.md`: one `##` section per note, oldest first. Each section has a
//! `<!-- note {...} -->` line the app reads back; everything else is free text a person or Claude
//! may edit, and the app keeps it byte for byte. The format is FFC's
//! (`docs/reference/dev-notes.md`), so the same workflow serves both projects.
use super::errors::Recent;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const HEADER: &str = "# stream-dlp — dev notes

Notes written in the app's Dev mode. One `##` section per note, oldest first.
Each section has a machine-readable `<!-- note {...} -->` line; keep it when editing by hand.
To close a note, set `\"status\":\"done\"` in that line (or use the app). Images are in `img/`.
";
const TITLE_MAX: usize = 80;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    Bug,
    Polish,
    /// Also what an unknown tag in a hand edit reads as.
    #[default]
    #[serde(other)]
    Idea,
}

impl Tag {
    pub const ALL: [Tag; 3] = [Tag::Bug, Tag::Idea, Tag::Polish];
    pub fn label(self) -> &'static str {
        match self {
            Tag::Bug => "Bug",
            Tag::Idea => "Idea",
            Tag::Polish => "Polish",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Done,
    #[default]
    #[serde(other)]
    Open,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Done => "done",
        }
    }
}

/// The machine-readable line of a section. Field order is the file's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    #[serde(default)]
    pub tag: Tag,
    #[serde(default)]
    pub status: Status,
    /// Local time with its offset, e.g. `2026-10-30T14:03:11+01:00`.
    #[serde(default)]
    pub created: String,
    /// Relative to the notes folder, e.g. `img/N-014-1.png`.
    #[serde(default)]
    pub images: Vec<String>,
}

/// A note as the app lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub meta: Meta,
    pub title: String,
    /// The `**Where:**` line, or empty when it was removed.
    pub place: String,
}

/// What the panel saves.
pub struct Draft<'a> {
    pub text: &'a str,
    pub tag: Tag,
    /// The summary of where the app was, e.g. `Download · youtube.com/watch?v=… · Failed`.
    pub place: &'a str,
    pub context: &'a serde_json::Value,
    pub errors: &'a [Recent],
    pub time: DateTime<Local>,
}

/// The file split into the text before the first note and one part per `##` section.
#[derive(Debug, Default)]
pub struct File {
    header: String,
    sections: Vec<(String, Option<Note>)>,
}

/// A fence opener (CommonMark): three or more backticks or tildes, then an info string.
fn fence(line: &str) -> Option<(char, usize, &str)> {
    let mark = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let length = line.chars().take_while(|c| *c == mark).count();
    (length >= 3).then(|| (mark, length, &line[length..]))
}

/// Whether `line` closes a fence opened with `open`: same mark, at least as long, no info.
fn closes(line: &str, open: (char, usize)) -> bool {
    fence(line).is_some_and(|(mark, length, info)| {
        mark == open.0 && length >= open.1 && info.trim().is_empty()
    })
}

/// The fence still open after `lines`.
fn open_fence<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<(char, usize)> {
    let mut open = None;
    for line in lines {
        match open {
            None => open = fence(line).map(|(mark, length, _)| (mark, length)),
            Some(fence) if closes(line, fence) => open = None,
            Some(_) => {}
        }
    }
    open
}

fn meta_json(line: &str) -> Option<&str> {
    line.trim_end_matches([' ', '\t'])
        .strip_prefix("<!-- note ")?
        .strip_suffix(" -->")
        .filter(|json| json.starts_with('{') && json.ends_with('}'))
}

fn meta_line(meta: &Meta) -> String {
    format!(
        "<!-- note {} -->",
        serde_json::to_string(meta).expect("a note's meta serializes")
    )
}

fn heading(meta: &Meta, title: &str) -> String {
    format!(
        "## {} · {} · {} — {title}",
        meta.id,
        meta.tag.label(),
        meta.status.label()
    )
}

/// The first non-empty line, cut to 80 characters.
pub fn title(text: &str) -> String {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty());
    let first = first.unwrap_or_default();
    if first.chars().count() > TITLE_MAX {
        let cut: String = first.chars().take(TITLE_MAX - 1).collect();
        format!("{}…", cut.trim_end())
    } else {
        first.into()
    }
}

/// Lines that would start a section or a title are escaped, and a fence the text leaves open is
/// closed, so the text cannot change the file's shape.
fn escape(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = text
        .trim()
        .lines()
        .map(|line| {
            let hashes = line.chars().take_while(|c| *c == '#').count();
            let rest = &line[hashes..];
            if (1..=2).contains(&hashes) && (rest.is_empty() || rest.starts_with([' ', '\t'])) {
                format!("\\{line}")
            } else {
                line.into()
            }
        })
        .collect();
    if let Some((mark, length)) = open_fence(lines.iter().map(String::as_str)) {
        lines.push(mark.to_string().repeat(length));
    }
    lines.join("\n")
}

fn read_note(raw: &str) -> Option<Note> {
    let meta: Meta = raw
        .lines()
        .find_map(meta_json)
        .and_then(|json| serde_json::from_str(json).ok())
        .filter(|meta: &Meta| !meta.id.is_empty())?;
    let first = raw.lines().next().unwrap_or_default();
    let title = match first.find(" — ") {
        Some(dash) => first[dash + " — ".len()..].trim().into(),
        None => first.trim_start_matches('#').trim().into(),
    };
    let place = raw
        .lines()
        .find_map(|line| line.strip_prefix("**Where:** "))
        .unwrap_or_default()
        .into();
    Some(Note { meta, title, place })
}

/// A new note's section, ending with one newline.
pub fn section(meta: &Meta, draft: &Draft) -> String {
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let when = format!(
        "{} · v{} · {build} · {}",
        draft.time.format("%Y-%m-%d %H:%M"),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    );
    let mut lines = vec![
        heading(meta, &title(draft.text)),
        String::new(),
        meta_line(meta),
        String::new(),
        format!("**Where:** {}", draft.place),
        format!("**When:** {when}"),
        String::new(),
        escape(draft.text),
        String::new(),
    ];
    if !meta.images.is_empty() {
        for (n, path) in meta.images.iter().enumerate() {
            lines.push(format!("![Screenshot {}]({path})", n + 1));
        }
        lines.push(String::new());
    }
    let context = serde_json::to_string_pretty(draft.context).unwrap_or_default();
    lines.extend(
        [
            "<details><summary>Context</summary>",
            "",
            "```json",
            &context,
            "```",
            "",
            "</details>",
            "",
        ]
        .map(String::from),
    );
    if !draft.errors.is_empty() {
        let errors: Vec<String> = draft
            .errors
            .iter()
            .map(|error| {
                let repeats = if error.count > 1 {
                    format!(" (×{})", error.count)
                } else {
                    String::new()
                };
                format!(
                    "{} {:<8} {}{repeats}",
                    error.at.format("%H:%M:%S"),
                    error.origin,
                    error.message.replace('\n', "\n    ")
                )
            })
            .collect();
        lines.extend([
            format!(
                "<details><summary>Recent errors ({})</summary>",
                draft.errors.len()
            ),
            String::new(),
            "```text".into(),
            errors.join("\n"),
            "```".into(),
            String::new(),
            "</details>".into(),
            String::new(),
        ]);
    }
    lines.join("\n")
}

/// The text ends with exactly one newline, so a following `##` has a blank line before it.
fn with_blank_end(raw: &str) -> String {
    format!("{}\n", raw.trim_end_matches('\n'))
}

impl File {
    /// Splits on lines starting with `## ` outside code fences. A heading followed by a meta line
    /// starts a note even inside a fence a hand edit left open.
    pub fn parse(content: &str) -> Self {
        let text = content.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = text.split('\n').collect();
        let meta_follows = |i: usize| {
            lines[i + 1..]
                .iter()
                .find(|line| !line.trim().is_empty())
                .is_some_and(|line| meta_json(line).is_some())
        };
        let mut starts = Vec::new();
        let mut open = None;
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("## ") && (open.is_none() || meta_follows(i)) {
                starts.push(i);
                open = None;
            } else if let Some(opened) = open {
                if closes(line, opened) {
                    open = None;
                }
            } else {
                open = fence(line).map(|(mark, length, _)| (mark, length));
            }
        }
        let Some(&first) = starts.first() else {
            return Self {
                header: text,
                sections: Vec::new(),
            };
        };
        let header = lines[..first].join("\n");
        let sections = starts
            .iter()
            .enumerate()
            .map(|(k, &start)| {
                let end = starts.get(k + 1).copied().unwrap_or(lines.len());
                let raw = lines[start..end].join("\n");
                let note = read_note(&raw);
                (raw, note)
            })
            .collect();
        Self { header, sections }
    }

    /// The file again; untouched sections byte for byte.
    pub fn format(&self) -> String {
        std::iter::once(self.header.as_str())
            .chain(self.sections.iter().map(|(raw, _)| raw.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn notes(&self) -> impl DoubleEndedIterator<Item = &Note> {
        self.sections.iter().filter_map(|(_, note)| note.as_ref())
    }

    /// `N-` and three digits, one more than the highest id in the file.
    pub fn next_id(&self) -> String {
        let number = |id: &str| id.strip_prefix("N-").and_then(|n| n.parse::<u32>().ok());
        let highest = self
            .sections
            .iter()
            .filter_map(|(raw, note)| {
                let heading = raw.strip_prefix("## ").and_then(|h| h.split(' ').next());
                heading
                    .and_then(number)
                    .or_else(|| note.as_ref().and_then(|note| number(&note.meta.id)))
            })
            .max()
            .unwrap_or(0);
        format!("N-{:03}", highest + 1)
    }

    pub fn append(&mut self, raw: &str) {
        if let Some((last, _)) = self.sections.last_mut() {
            *last = with_blank_end(last);
        } else {
            let header = if self.header.trim().is_empty() {
                HEADER
            } else {
                &self.header
            };
            self.header = with_blank_end(header);
        }
        let raw = with_blank_end(raw);
        let note = read_note(&raw);
        self.sections.push((raw, note));
    }

    /// Regenerates a note's heading and meta line with the new status; the rest stays as it is.
    pub fn set_status(&mut self, id: &str, status: Status) -> bool {
        let Some((raw, note)) = self
            .sections
            .iter_mut()
            .find(|(_, note)| note.as_ref().is_some_and(|note| note.meta.id == id))
        else {
            return false;
        };
        let old = note.take().expect("found by its note");
        let meta = Meta { status, ..old.meta };
        let lines: Vec<String> = raw
            .split('\n')
            .enumerate()
            .map(|(i, line)| match i {
                0 => heading(&meta, &old.title),
                _ if meta_json(line).is_some() => meta_line(&meta),
                _ => line.into(),
            })
            .collect();
        *raw = lines.join("\n");
        *note = read_note(raw);
        true
    }

    /// Removes a note's section and returns the note, whose images are the caller's to delete.
    pub fn remove(&mut self, id: &str) -> Option<Note> {
        let index = self
            .sections
            .iter()
            .position(|(_, note)| note.as_ref().is_some_and(|note| note.meta.id == id))?;
        self.sections.remove(index).1
    }

    /// The notes' sections as one Markdown text to paste into a Claude session, oldest first,
    /// with each image link made absolute.
    pub fn for_claude(&self, ids: &[&str], absolute: impl Fn(&str) -> String) -> String {
        let texts: Vec<String> = self
            .sections
            .iter()
            .filter_map(|(raw, note)| {
                let note = note
                    .as_ref()
                    .filter(|note| ids.contains(&note.meta.id.as_str()))?;
                let lines: Vec<String> = raw
                    .split('\n')
                    .map(|line| {
                        let image = line
                            .strip_suffix(')')
                            .and_then(|l| l.split_once("]("))
                            .filter(|(alt, path)| {
                                alt.starts_with("![") && note.meta.images.iter().any(|p| p == path)
                            });
                        match image {
                            Some((alt, path)) => format!("{alt}]({})", absolute(path)),
                            None => line.into(),
                        }
                    })
                    .collect();
                Some(lines.join("\n").trim_end().to_string())
            })
            .collect();
        format!("{}\n", texts.join("\n\n"))
    }
}

/// The notes folder: `notes.md` and `img/`.
pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    fn path(&self) -> PathBuf {
        self.dir.join("notes.md")
    }

    pub fn read(&self) -> Result<File, String> {
        match std::fs::read_to_string(self.path()) {
            Ok(text) => Ok(File::parse(&text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
            Err(error) => Err(format!("Cannot read {}: {error}", self.path().display())),
        }
    }

    fn write(&self, file: &File) -> Result<(), String> {
        super::write_atomically(&self.path(), file.format().as_bytes())
    }

    /// Saves a note and its screenshots (PNG files), and returns its id. The file is read again
    /// first, so a hand edit made meanwhile is kept.
    pub fn add(&self, draft: &Draft, images: &[Vec<u8>]) -> Result<String, String> {
        let mut file = self.read()?;
        let id = file.next_id();
        let mut paths = Vec::new();
        for (n, png) in images.iter().enumerate() {
            let path = format!("img/{id}-{}.png", n + 1);
            super::write_atomically(&self.dir.join(&path), png)?;
            paths.push(path);
        }
        let meta = Meta {
            id: id.clone(),
            tag: draft.tag,
            status: Status::Open,
            created: draft
                .time
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            images: paths,
        };
        file.append(&section(&meta, draft));
        self.write(&file)?;
        Ok(id)
    }

    pub fn set_status(&self, id: &str, status: Status) -> Result<(), String> {
        let mut file = self.read()?;
        if file.set_status(id, status) {
            self.write(&file)?;
        }
        Ok(())
    }

    /// Removes a note and its images. Only images inside `img/` are deleted, whatever a hand
    /// edit wrote in the meta line.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut file = self.read()?;
        let Some(note) = file.remove(id) else {
            return Ok(());
        };
        self.write(&file)?;
        for image in &note.meta.images {
            let path = Path::new(image);
            let inside = path.starts_with("img")
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)));
            if inside {
                let _ = std::fs::remove_file(self.dir.join(path));
            }
        }
        Ok(())
    }

    pub fn for_claude(&self, ids: &[&str]) -> Result<String, String> {
        let file = self.read()?;
        Ok(file.for_claude(ids, |path| {
            self.dir.join(path).to_string_lossy().replace('\\', "/")
        }))
    }
}

/// An RGBA image as PNG.
pub fn png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(rgba)
        .map_err(|error| error.to_string())?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft<'a>(text: &'a str, context: &'a serde_json::Value, errors: &'a [Recent]) -> Draft<'a> {
        Draft {
            text,
            tag: Tag::Bug,
            place: "Download · Failed",
            context,
            errors,
            time: Local::now(),
        }
    }

    #[test]
    fn notes_are_added_closed_and_removed_keeping_hand_edits() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store {
            dir: dir.path().join("dev-notes"),
        };
        let context = serde_json::json!({ "page": "download" });
        let text = "Title cut off\n## not a section\n```\nunclosed";
        let first = store
            .add(&draft(text, &context, &[]), &[vec![1, 2, 3]])
            .unwrap();
        assert_eq!(first, "N-001");
        assert!(store.dir.join("img/N-001-1.png").is_file());

        // Hand edits: a line under the first note and a section the app does not know.
        let path = store.dir.join("notes.md");
        let edited = std::fs::read_to_string(&path).unwrap().replace(
            "\n\nTitle cut off\n",
            "\n\nTitle cut off\nDone in v0.4.0 (abc123)\n",
        ) + "\n## Scratch\nfree text\n";
        std::fs::write(&path, &edited).unwrap();
        let errors = [Recent {
            at: Local::now(),
            origin: "download",
            message: "HTTP 403".into(),
            count: 2,
        }];
        let second = store.add(&draft("Second", &context, &errors), &[]).unwrap();
        assert_eq!(second, "N-002");
        store.set_status("N-001", Status::Done).unwrap();

        let file = store.read().unwrap();
        let notes: Vec<&Note> = file.notes().collect();
        assert_eq!(notes.len(), 2, "the closed fence does not swallow N-002");
        assert_eq!(notes[0].title, "Title cut off");
        assert_eq!(notes[0].meta.status, Status::Done);
        assert_eq!(notes[0].meta.tag, Tag::Bug);
        assert_eq!(notes[0].meta.images, ["img/N-001-1.png"]);
        assert_eq!(notes[0].place, "Download · Failed");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(HEADER));
        assert!(text.contains("## N-001 · Bug · done — Title cut off\n\n<!-- note {\"id\":\"N-001\",\"tag\":\"bug\",\"status\":\"done\","));
        assert!(text.contains(
            "Title cut off\nDone in v0.4.0 (abc123)\n\\## not a section\n```\nunclosed\n```\n"
        ));
        assert!(text.contains("\n## Scratch\nfree text\n\n## N-002 · Bug · open — Second\n"));
        assert!(text.contains(" download HTTP 403 (×2)\n"));
        assert!(text.contains("```json\n{\n  \"page\": \"download\"\n}\n```"));

        let pasted = store.for_claude(&["N-001"]).unwrap();
        let image = store.dir.join("img/N-001-1.png");
        let image = image.to_string_lossy().replace('\\', "/");
        assert!(pasted.contains(&format!("![Screenshot 1]({image})")));
        assert!(!pasted.contains("N-002"));

        store.remove("N-001").unwrap();
        assert!(!store.dir.join("img/N-001-1.png").exists());
        let file = store.read().unwrap();
        assert_eq!(
            file.notes().map(|n| n.meta.id.as_str()).collect::<Vec<_>>(),
            ["N-002"]
        );
        assert!(file.format().contains("## Scratch\nfree text\n"));
        assert_eq!(file.next_id(), "N-003");
    }

    #[test]
    fn hand_written_notes_are_read_leniently() {
        let file = File::parse(
            "# notes\r\n\r\n## N-007 · Odd · open — Hand title\r\n\r\n<!-- note {\"id\":\"N-007\",\"tag\":\"weird\",\"status\":\"??\"} -->  \r\n",
        );
        let note = file.notes().next().unwrap();
        assert_eq!(note.title, "Hand title");
        assert_eq!((note.meta.tag, note.meta.status), (Tag::Idea, Status::Open));
        assert_eq!(file.next_id(), "N-008");
        assert!(!file.format().contains('\r'));
        assert_eq!(File::parse("no notes yet").notes().count(), 0);
    }

    #[test]
    fn titles_and_text_keep_the_file_shape() {
        assert_eq!(title("\n  First line  \nsecond"), "First line");
        assert_eq!(title(&"x".repeat(100)).chars().count(), TITLE_MAX);
        assert_eq!(escape("# a\n##\n### b\n#tag"), "\\# a\n\\##\n### b\n#tag");
        assert_eq!(escape("~~~~ rust\ncode"), "~~~~ rust\ncode\n~~~~");
    }

    #[test]
    fn screenshots_encode_as_png() {
        let png = png(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
