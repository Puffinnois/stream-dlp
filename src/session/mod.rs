//! Per-platform cookie imports, YouTube Firefox profiles, and the manual export guide.
pub mod export;
pub mod firefox;

use crate::engine::Source;
use std::{
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

// Intentionally no Debug implementation: authentication values must not enter logs.
#[derive(Clone)]
pub struct SessionCookies {
    text: Arc<str>,
    pub(crate) source: Source,
}

impl SessionCookies {
    pub fn load(path: &Path, source: Source) -> Result<Self, String> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| {
                "Could not open the session file. Choose an exported cookies.txt file.".to_owned()
            })?
            .take(2_097_153)
            .read_to_end(&mut bytes)
            .map_err(|_| "Could not read the session file.".to_owned())?;
        if bytes.len() > 2_097_152 {
            return Err(format!(
                "The session file is too large. Export only {} cookies.",
                source.label()
            ));
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            "The session file must be a Netscape cookies.txt file in UTF-8.".to_owned()
        })?;
        Self::parse_for(&text, source)
    }

    pub(crate) fn parse_for(text: &str, source: Source) -> Result<Self, String> {
        let text = text.trim_start_matches('\u{feff}');
        let mut lines = text.lines();
        if !matches!(
            lines.next(),
            Some("# Netscape HTTP Cookie File" | "# HTTP Cookie File")
        ) {
            return Err(
                "Choose a Netscape cookies.txt export. JSON cookie exports are not supported."
                    .into(),
            );
        }
        let mut filtered = String::from("# Netscape HTTP Cookie File\r\n");
        let mut count = 0;
        for (index, line) in lines.enumerate() {
            if line.trim().is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_"))
            {
                continue;
            }
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 7 {
                return Err(format!(
                    "Invalid cookie entry on line {}. Export cookies in Netscape format again.",
                    index + 2
                ));
            }
            let raw_domain = fields[0].strip_prefix("#HttpOnly_").unwrap_or(fields[0]);
            let domain = raw_domain
                .strip_prefix('.')
                .unwrap_or(raw_domain)
                .to_ascii_lowercase();
            let domains: &[&str] = match source {
                Source::YouTube => &["youtube.com"],
                Source::TikTok => &["tiktok.com"],
                Source::Facebook => &["facebook.com"],
                Source::Instagram => &["instagram.com"],
                Source::X => &["x.com", "twitter.com"],
            };
            if !domains.iter().any(|base| {
                domain == *base
                    || domain
                        .strip_suffix(base)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            }) {
                continue;
            }
            if !matches!(fields[1], "TRUE" | "FALSE")
                || !fields[2].starts_with('/')
                || !matches!(fields[3], "TRUE" | "FALSE")
                || fields[4].parse::<u64>().is_err()
                || fields[5].is_empty()
                || (fields[1] == "TRUE") != raw_domain.starts_with('.')
                || domain.split('.').any(str::is_empty)
                || line.chars().any(|c| c.is_control() && c != '\t')
            {
                return Err(format!(
                    "Invalid {} cookie on line {}. Export the session again.",
                    source.label(),
                    index + 2
                ));
            }
            filtered.push_str(line);
            filtered.push_str("\r\n");
            count += 1;
        }
        if count == 0 {
            return Err(format!(
                "No {} cookies were found. Export the session from a signed-in tab.",
                source.label()
            ));
        }
        Ok(Self {
            text: filtered.into(),
            source,
        })
    }

    pub fn temporary_file(&self) -> Result<tempfile::NamedTempFile, String> {
        let mut file = tempfile::Builder::new()
            .prefix("stream-dlp-session-")
            .suffix(".txt")
            .tempfile()
            .map_err(|_| "Could not create a temporary session file.".to_owned())?;
        file.write_all(self.text.as_bytes())
            .and_then(|_| file.flush())
            .map_err(|_| "Could not prepare the session.".to_owned())?;
        Ok(file)
    }
}

#[cfg(test)]
pub fn fake_session() -> SessionCookies {
    tests::fake_session()
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str = "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t4102444800\tTEST_SESSION\tfake-test-value\n";
    pub fn fake_session() -> SessionCookies {
        SessionCookies::parse_for(SAMPLE, Source::YouTube).unwrap()
    }

    #[test]
    fn keeps_only_youtube_domains_and_normalizes_windows_newlines() {
        let input = format!(
            "{SAMPLE}.example.com\tTRUE\t/\tTRUE\t4102444800\tSECRET\tunrelated\n.youtube.com.evil.test\tTRUE\t/\tTRUE\t4102444800\tBAD\tunrelated\n#HttpOnly_.www.youtube.com\tTRUE\t/\tTRUE\t4102444800\tTEST_HTTPONLY\tfake\n"
        );
        let session = SessionCookies::parse_for(&input, Source::YouTube).unwrap();
        assert!(session.text.contains("TEST_SESSION"));
        assert!(session.text.contains("#HttpOnly_.www.youtube.com"));
        assert!(!session.text.contains("unrelated"));
        assert!(!session.text.replace("\r\n", "").contains('\n'));
    }
    #[test]
    fn mixed_export_keeps_only_the_selected_platform() {
        let domains = [
            "youtube.com",
            "tiktok.com",
            "x.com",
            "twitter.com",
            "facebook.com",
            "m.facebook.com",
            "instagram.com",
            "www.instagram.com",
            "notinstagram.com",
            "instagram.com.invalid",
            "notfacebook.com",
            "notx.com",
            "x.com.invalid",
        ];
        let mut input = String::from("# HTTP Cookie File\n");
        for domain in domains {
            input.push_str(&format!(".{domain}\tTRUE\t/\tTRUE\t0\tTEST\tfake\n"));
        }
        for (source, allowed) in [
            (Source::YouTube, &domains[0..1]),
            (Source::TikTok, &domains[1..2]),
            (Source::X, &domains[2..4]),
            (Source::Facebook, &domains[4..6]),
            (Source::Instagram, &domains[6..8]),
        ] {
            let jar = SessionCookies::parse_for(&input, source).unwrap();
            for domain in domains {
                assert_eq!(
                    jar.text.contains(&format!("\n.{domain}\t")),
                    allowed.contains(&domain)
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_exports_without_echoing_cookie_values() {
        for input in [
            "[]",
            "# Netscape HTTP Cookie File\n",
            "# Netscape HTTP Cookie File\n.youtube.com\tTOP_SECRET",
            "# HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\tinvalid\tSESSION\tTOP_SECRET",
        ] {
            let error = match SessionCookies::parse_for(input, Source::YouTube) {
                Ok(_) => panic!("Unexpected valid session"),
                Err(error) => error,
            };
            assert!(!error.contains("TOP_SECRET"));
        }
    }
    #[test]
    fn temporary_copy_is_deleted_and_original_export_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("cookies.txt");
        std::fs::write(&original, SAMPLE).unwrap();
        let session = SessionCookies::load(&original, Source::YouTube).unwrap();
        let copy = session.temporary_file().unwrap();
        let path = copy.path().to_owned();
        assert!(path.exists());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            session.text.as_ref()
        );
        drop(copy);
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(original).unwrap(), SAMPLE);
    }
}
