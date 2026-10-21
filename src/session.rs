use std::{
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

// Intentionally no Debug implementation: authentication values must not enter logs.
#[derive(Clone)]
pub struct SessionCookies(Arc<str>);

impl SessionCookies {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| {
                "Could not open the session file. Choose an exported cookies.txt file.".to_owned()
            })?
            .take(2_097_153)
            .read_to_end(&mut bytes)
            .map_err(|_| "Could not read the session file.".to_owned())?;
        if bytes.len() > 2_097_152 {
            return Err("The session file is too large. Export only youtube.com cookies.".into());
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            "The session file must be a Netscape cookies.txt file in UTF-8.".to_owned()
        })?;
        Self::parse(&text)
    }

    fn parse(text: &str) -> Result<Self, String> {
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
            if domain != "youtube.com" && !domain.ends_with(".youtube.com") {
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
                    "Invalid YouTube cookie on line {}. Export the session again.",
                    index + 2
                ));
            }
            filtered.push_str(line);
            filtered.push_str("\r\n");
            count += 1;
        }
        if count == 0 {
            return Err("No youtube.com cookies were found. Export the session from a signed-in YouTube tab.".into());
        }
        Ok(Self(filtered.into()))
    }

    pub fn temporary_file(&self) -> Result<tempfile::NamedTempFile, String> {
        let mut file = tempfile::Builder::new()
            .prefix("stream-dlp-session-")
            .suffix(".txt")
            .tempfile()
            .map_err(|_| "Could not create a temporary session file.".to_owned())?;
        file.write_all(self.0.as_bytes())
            .and_then(|_| file.flush())
            .map_err(|_| "Could not prepare the YouTube session.".to_owned())?;
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
        SessionCookies::parse(SAMPLE).unwrap()
    }

    #[test]
    fn keeps_only_youtube_domains_and_normalizes_windows_newlines() {
        let input = format!(
            "{SAMPLE}.example.com\tTRUE\t/\tTRUE\t4102444800\tSECRET\tunrelated\n.youtube.com.evil.test\tTRUE\t/\tTRUE\t4102444800\tBAD\tunrelated\n#HttpOnly_.www.youtube.com\tTRUE\t/\tTRUE\t4102444800\tTEST_HTTPONLY\tfake\n"
        );
        let session = SessionCookies::parse(&input).unwrap();
        assert!(session.0.contains("TEST_SESSION"));
        assert!(session.0.contains("#HttpOnly_.www.youtube.com"));
        assert!(!session.0.contains("unrelated"));
        assert!(!session.0.replace("\r\n", "").contains('\n'));
    }
    #[test]
    fn rejects_invalid_exports_without_echoing_cookie_values() {
        for input in [
            "[]",
            "# Netscape HTTP Cookie File\n",
            "# Netscape HTTP Cookie File\n.youtube.com\tTOP_SECRET",
            "# HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\tinvalid\tSESSION\tTOP_SECRET",
        ] {
            let error = match SessionCookies::parse(input) {
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
        let session = SessionCookies::load(&original).unwrap();
        let copy = session.temporary_file().unwrap();
        let path = copy.path().to_owned();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), session.0.as_ref());
        drop(copy);
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(original).unwrap(), SAMPLE);
    }
}
