//! Public feedback: a person sends a text note to the service in `services/feedback/`, and Dev
//! mode's Inbox reads it. What a note carries is typed here, so no path, title, link or raw error
//! can be sent; and what the Inbox receives comes from anyone, so it is checked again rather than
//! trusted. See `docs/reference/public-feedback.md`.
use crate::dev::notes::Tag;
use crate::engine::{EngineError, Source};
use serde::{Deserialize, Serialize};

/// Set when the service is deployed. While `None`, a release build shows no feedback at all.
const ENDPOINT: Option<&str> = Some("https://stream-dlp-feedback.puffinn.workers.dev");
pub const TEXT_MIN: usize = 15;
pub const TEXT_MAX: usize = 4000;
const LETTERS_MIN: usize = 10;
const DISTINCT_MIN: usize = 5;
const LINKS_MAX: usize = 2;

/// The service's address. A debug build can point at a local server.
pub fn endpoint() -> Option<String> {
    if cfg!(debug_assertions)
        && let Ok(url) = std::env::var("STREAM_DLP_FEEDBACK_URL")
        && !url.is_empty()
    {
        return Some(url);
    }
    ENDPOINT.map(String::from)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Page {
    Download,
    Videos,
    Gif,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Inspecting,
    Choosing,
    Picking,
    Downloading,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Site {
    YouTube,
    TikTok,
    X,
    Facebook,
    Instagram,
}

impl From<Source> for Site {
    fn from(source: Source) -> Self {
        match source {
            Source::YouTube => Self::YouTube,
            Source::TikTok => Self::TikTok,
            Source::X => Self::X,
            Source::Facebook => Self::Facebook,
            Source::Instagram => Self::Instagram,
        }
    }
}

/// What kind of error the page shows; its message, which can name a file, stays on the machine.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Unsupported,
    SignIn,
    RateLimited,
    Unavailable,
    Restricted,
    Network,
    TimedOut,
    ExpiredSession,
    MissingVideo,
    Drm,
    Helper,
    Other,
}

impl From<&EngineError> for ErrorKind {
    fn from(error: &EngineError) -> Self {
        match error {
            EngineError::Unsupported(_) => Self::Unsupported,
            EngineError::SignInNeeded(_) => Self::SignIn,
            EngineError::RateLimited(_) => Self::RateLimited,
            EngineError::Unavailable(_) => Self::Unavailable,
            EngineError::Restricted(_) => Self::Restricted,
            EngineError::Network(_) => Self::Network,
            EngineError::TimedOut(_) => Self::TimedOut,
            EngineError::ExpiredSession(_) => Self::ExpiredSession,
            EngineError::MissingVideo(_) => Self::MissingVideo,
            EngineError::Drm(_) => Self::Drm,
            EngineError::Helper(_) => Self::Helper,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    #[serde(rename = "macos")]
    Mac,
    Linux,
}

impl Os {
    fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Linux
        }
    }
}

/// All the context a public note carries, besides the text the person typed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    version: String,
    os: Os,
    page: Page,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phase: Option<Phase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<Site>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<ErrorKind>,
}

impl Context {
    pub fn new(
        page: Page,
        phase: Option<Phase>,
        source: Option<Site>,
        error: Option<ErrorKind>,
    ) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            os: Os::current(),
            page,
            phase,
            source,
            error,
        }
    }

    /// One line like `Public feedback · download · youtube · error: network`. Every part is a
    /// fixed word from the types above, so a sender cannot put text into it.
    pub fn place(&self) -> String {
        fn word(value: impl Serialize) -> String {
            match serde_json::to_value(value) {
                Ok(serde_json::Value::String(word)) => word,
                _ => String::new(),
            }
        }
        let mut parts = vec!["Public feedback".to_string(), word(self.page)];
        parts.extend(self.source.map(word));
        parts.extend(self.phase.map(word));
        parts.extend(self.error.map(|kind| format!("error: {}", word(kind))));
        parts.join(" · ")
    }

    pub fn version(&self) -> String {
        self.version.clone()
    }

    pub fn os(&self) -> &'static str {
        match self.os {
            Os::Windows => "windows",
            Os::Mac => "macos",
            Os::Linux => "linux",
        }
    }
}

#[derive(Serialize)]
struct Submission<'a> {
    text: &'a str,
    tag: Tag,
    context: &'a Context,
}

/// Control characters except tab and newline, and invisible or text-reversing characters, which
/// could hide what a note really says from the person reviewing it. The service removes the same.
fn hidden(c: char) -> bool {
    matches!(c as u32,
        0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f..=0x9f | 0xad
        | 0x200b..=0x200f | 0x2028..=0x202e | 0x2060..=0x206f | 0xfeff)
}

pub fn clean(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    text.chars()
        .filter(|c| !hidden(*c))
        .collect::<String>()
        .trim()
        .into()
}

/// Why a text cannot be sent yet, in words for the person writing it. The service checks the same
/// rules; this only saves a request that would be refused.
pub fn text_problem(text: &str) -> Option<&'static str> {
    let text = clean(text);
    if text.chars().count() < TEXT_MIN {
        return Some("Please describe what happened in a little more detail.");
    }
    if text.chars().count() > TEXT_MAX {
        return Some("This note is too long. Please shorten it.");
    }
    let letters: Vec<char> = text
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    if letters.len() < LETTERS_MIN {
        return Some("Please describe what happened in a little more detail.");
    }
    let mut distinct = letters;
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() < DISTINCT_MIN {
        return Some("This does not look like a description. Please say what happened.");
    }
    let lower = text.to_lowercase();
    let links = lower.matches("http://").count()
        + lower.matches("https://").count()
        + lower.matches("www.").count();
    if links > LINKS_MAX {
        return Some("Please keep to two links at most.");
    }
    None
}

fn explain(status: u16, body: &str) -> String {
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["error"].as_str().map(String::from))
        .unwrap_or_default();
    match (status, code.as_str()) {
        (429, _) => "You have sent several notes recently. Please try again later.".into(),
        (503, "busy") => "We have received a lot of feedback today. Please try again later.".into(),
        (_, "too_short" | "low_quality") => {
            "Please describe what happened in a little more detail.".into()
        }
        (_, "too_long") => "This note is too long. Please shorten it.".into(),
        (_, "too_many_links") => "Please keep to two links at most.".into(),
        (401, _) => "The service refused this token.".into(),
        _ => format!(
            "The feedback service could not take this (HTTP {status}). Please try again later."
        ),
    }
}

/// Sends a note. The service answers the same for a new note, a repeat and a blocked sender.
pub fn send(endpoint: &str, text: &str, tag: Tag, context: &Context) -> Result<(), String> {
    let text = clean(text);
    let body = serde_json::to_string(&Submission {
        text: &text,
        tag,
        context,
    })
    .map_err(|error| error.to_string())?;
    let url = format!("{}/v1/notes", endpoint.trim_end_matches('/'));
    match crate::net::api_post(&url, None, Some(&body))? {
        (202, _) => Ok(()),
        (status, body) => Err(explain(status, &body)),
    }
}

/// A note waiting in the Inbox. Written by anyone, so only the fields below are read, and a note
/// that does not fit is left out of the list.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub id: String,
    pub tag: Tag,
    pub text: String,
    pub context: Context,
    pub created: i64,
    pub reports: u32,
}

impl Pending {
    fn well_formed(mut self) -> Option<Self> {
        let hex = |part: &str, length| {
            part.len() == length && part.bytes().all(|b| b.is_ascii_hexdigit())
        };
        let parts: Vec<&str> = self.id.split('-').collect();
        let id_ok =
            parts.len() == 5 && [8, 4, 4, 4, 12].iter().zip(&parts).all(|(n, p)| hex(p, *n));
        self.text = clean(&self.text);
        let text_ok = !self.text.is_empty() && self.text.chars().count() <= TEXT_MAX;
        let version_ok = semver::Version::parse(&self.context.version).is_ok();
        (id_ok && text_ok && version_ok).then_some(self)
    }
}

/// The Inbox's first page: the notes, how many wait in all, and how many were left out.
#[derive(Debug, Default, PartialEq)]
pub struct InboxPage {
    pub notes: Vec<Pending>,
    pub total: u64,
    pub skipped: usize,
}

fn parse_inbox(body: &str) -> Result<InboxPage, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "The service sent an unreadable answer.")?;
    let items = value["notes"]
        .as_array()
        .ok_or("The service sent an unreadable answer.")?;
    let notes: Vec<Pending> = items
        .iter()
        .filter_map(|item| serde_json::from_value::<Pending>(item.clone()).ok())
        .filter_map(Pending::well_formed)
        .collect();
    Ok(InboxPage {
        skipped: items.len() - notes.len(),
        total: value["total"].as_u64().unwrap_or_default(),
        notes,
    })
}

pub fn inbox(endpoint: &str, token: &str) -> Result<InboxPage, String> {
    let url = format!("{}/v1/admin/notes?limit=50", endpoint.trim_end_matches('/'));
    match crate::net::api_get(&url, token)? {
        (200, body) => parse_inbox(&body),
        (status, body) => Err(explain(status, &body)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision {
    Accept,
    Reject,
    Block,
}

/// Records a decision on the service, which then deletes the note's text.
pub fn decide(endpoint: &str, token: &str, id: &str, decision: Decision) -> Result<(), String> {
    let action = match decision {
        Decision::Accept => "accept",
        Decision::Reject => "reject",
        Decision::Block => "block",
    };
    let url = format!(
        "{}/v1/admin/notes/{id}/{action}",
        endpoint.trim_end_matches('/')
    );
    match crate::net::api_post(&url, Some(token), None)? {
        (200, _) => Ok(()),
        (status, body) => Err(explain(status, &body)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_context_has_only_fixed_words() {
        let context = Context::new(
            Page::Download,
            Some(Phase::Choosing),
            Some(Site::YouTube),
            Some(ErrorKind::SignIn),
        );
        let json = serde_json::to_value(&context).unwrap();
        assert_eq!(json["page"], "download");
        assert_eq!(json["phase"], "choosing");
        assert_eq!(json["source"], "youtube");
        assert_eq!(json["error"], "sign_in");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            context.place(),
            "Public feedback · download · youtube · choosing · error: sign_in"
        );
        let plain = Context::new(Page::Settings, None, None, None);
        let json = serde_json::to_value(&plain).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 3);
    }

    #[test]
    fn text_rules_match_the_service() {
        for bad in [
            "hello",
            "!!!!!!!!!!!!!!!!!!!!!!!!",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "ababababababababababab",
            "buy here https://a.example https://b.example www.c.example",
        ] {
            assert!(text_problem(bad).is_some(), "{bad}");
        }
        assert!(text_problem(&"word ".repeat(1000)).is_some());
        assert!(text_problem("The format list cuts off long codec names").is_none());
        assert!(text_problem("格式列表把很长的编码名称截断了，看不到完整内容").is_none());
    }

    #[test]
    fn hidden_characters_are_removed() {
        let reversed = format!("  safe{}text{}\r\nline ", '\u{202e}', '\u{200b}');
        assert_eq!(clean(&reversed), "safetext\nline");
    }

    #[test]
    fn the_inbox_leaves_out_what_does_not_fit() {
        let good = serde_json::json!({
            "id": "00000000-0000-4000-8000-000000000001",
            "tag": "bug",
            "text": "Format list is cut off",
            "context": {"version": "0.6.1", "os": "windows", "page": "gif"},
            "created": 1_800_000_000,
            "reports": 2,
        });
        let mut bad_id = good.clone();
        bad_id["id"] = "../../settings".into();
        let mut bad_context = good.clone();
        bad_context["context"]["path"] = "C:\\Users\\someone".into();
        let mut bad_field = good.clone();
        bad_field["images"] = serde_json::json!(["../../x"]);
        let mut long = good.clone();
        long["text"] = "x".repeat(TEXT_MAX + 1).into();
        let body = serde_json::json!({
            "total": 9,
            "notes": [good, bad_id, bad_context, bad_field, long, "text"],
        });
        let page = parse_inbox(&body.to_string()).unwrap();
        assert_eq!((page.notes.len(), page.skipped, page.total), (1, 5, 9));
        assert_eq!(page.notes[0].reports, 2);
        assert!(parse_inbox("not json").is_err());
    }

    #[test]
    fn refusals_are_explained_without_echoing_the_answer() {
        assert!(explain(429, "").contains("try again later"));
        assert!(explain(503, r#"{"error":"busy"}"#).contains("a lot of feedback"));
        assert!(explain(400, r#"{"error":"too_long"}"#).contains("too long"));
        assert!(!explain(500, r#"{"error":"<script>"}"#).contains("script"));
    }
}
