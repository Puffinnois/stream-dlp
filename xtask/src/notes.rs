//! The tested yt-dlp line that ends every release's notes, `Tested with yt-dlp 2026.08.19.`,
//! and the check `cargo xtask release` makes before it changes anything.
use std::path::{Path, PathBuf};

const PREFIX: &str = "Tested with yt-dlp ";

/// `docs/releases/release-X.Y.Z.md`.
pub fn path(root: &Path, version: &str) -> PathBuf {
    root.join("docs")
        .join("releases")
        .join(format!("release-{version}.md"))
}

/// The line the notes must end with.
pub fn line(ytdlp: &str) -> String {
    format!("{PREFIX}{ytdlp}.")
}

/// The yt-dlp version named by the notes' last non-blank line, if it is the tested line.
fn tested_version(notes: &str) -> Option<&str> {
    let last = notes.lines().rev().find(|l| !l.trim().is_empty())?.trim();
    let version = last.strip_prefix(PREFIX)?.strip_suffix('.')?;
    is_version(version).then_some(version)
}

/// yt-dlp's `--version`: date-like numbers separated by dots (2026.08.19, 2026.08.19.1).
pub fn is_version(text: &str) -> bool {
    !text.is_empty()
        && text
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Refuses a release whose notes (`None`: no file) do not end with the line for `bundled`.
pub fn check(next: &str, notes: Option<&str>, bundled: &str) -> Result<(), String> {
    let file = format!("docs/releases/release-{next}.md");
    let wanted = line(bundled);
    let Some(notes) = notes else {
        return Err(format!(
            "{file} is missing. Write and merge the release notes for {next} first, \
             ending with the line `{wanted}`"
        ));
    };
    match tested_version(notes) {
        None => Err(format!(
            "{file} must end with the tested yt-dlp line. Add `{wanted}` as its last line \
             (`cargo xtask tools` prints it)"
        )),
        Some(named) if named != bundled => Err(format!(
            "{file} says it was tested with yt-dlp {named}, but tools/ has yt-dlp {bundled}. \
             Test the release with the bundled yt-dlp and change the last line to `{wanted}`"
        )),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &str =
        "# stream-dlp 0.9.0\n\n- **A feature.** Text.\n\nTested with yt-dlp 2026.08.19.\n";

    #[test]
    fn the_last_line_names_the_tested_version() {
        assert_eq!(tested_version(NOTES), Some("2026.08.19"));
        assert_eq!(
            tested_version(&NOTES.replace('\n', "\r\n")),
            Some("2026.08.19")
        );
        assert_eq!(
            tested_version("- Item.\nTested with yt-dlp 2026.08.19.1.\n\n  \n"),
            Some("2026.08.19.1")
        );
        for notes in [
            "",
            "# stream-dlp 0.9.0\n- Item.\n",
            "Tested with yt-dlp 2026.08.19.\n- An item after it.\n",
            "Tested with yt-dlp 2026.08.19\n",
            "Tested with yt-dlp .\n",
            "Tested with yt-dlp nightly.\n",
            "Tested with yt-dlp 2026..19.\n",
            "- Tested with yt-dlp 2026.08.19.\n",
            "tested with yt-dlp 2026.08.19.\n",
        ] {
            assert_eq!(tested_version(notes), None, "{notes:?}");
        }
        assert_eq!(line("2026.08.19"), "Tested with yt-dlp 2026.08.19.");
        assert_eq!(tested_version(&line("2026.08.19")), Some("2026.08.19"));
    }

    #[test]
    fn a_release_needs_notes_that_name_the_bundled_yt_dlp() {
        assert_eq!(check("0.9.0", Some(NOTES), "2026.08.19"), Ok(()));

        let missing = check("0.9.0", None, "2026.08.19").unwrap_err();
        assert!(missing.contains("docs/releases/release-0.9.0.md is missing"));
        assert!(missing.contains("`Tested with yt-dlp 2026.08.19.`"));

        let no_line = check("0.9.0", Some("# stream-dlp 0.9.0\n- Item.\n"), "2026.08.19");
        let no_line = no_line.unwrap_err();
        assert!(no_line.contains("must end with the tested yt-dlp line"));
        assert!(no_line.contains("`Tested with yt-dlp 2026.08.19.`"));

        let other = check("0.9.0", Some(NOTES), "2026.09.01").unwrap_err();
        assert!(other.contains("tested with yt-dlp 2026.08.19, but tools/ has yt-dlp 2026.09.01"));
        assert!(other.contains("`Tested with yt-dlp 2026.09.01.`"));
    }

    #[test]
    fn versions_are_dotted_numbers() {
        assert!(is_version("2026.08.19"));
        assert!(is_version("2026.08.19.232"));
        for text in [
            "",
            "2026.08.",
            ".08.19",
            "v2026.08.19",
            "2026.08.19-dev",
            "nightly",
        ] {
            assert!(!is_version(text), "{text}");
        }
    }

    #[test]
    fn the_notes_path_follows_the_version() {
        assert_eq!(
            path(Path::new("repo"), "0.9.0"),
            Path::new("repo/docs/releases/release-0.9.0.md")
        );
    }
}
