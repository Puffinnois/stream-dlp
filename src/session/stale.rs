//! Removes session working files that a crashed or killed run left behind.
//!
//! `SessionCookies::temporary_file` is the only place that writes session data to disk (the
//! Firefox import reads the browser database in place and never copies it). A normal drop
//! removes that file; a crash or a killed process does not, so the next start sweeps the
//! folder it is created in. The sweep decides by name and age only: it never opens a file,
//! never follows a link or reparse point, and logs nothing.
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// The name every session working file and folder starts with.
pub(crate) const PREFIX: &str = "stream-dlp-session-";

/// How old an entry must be before the sweep removes it.
///
/// Another running instance (the installed app and a development build can run at once)
/// may still be using a file, so only files older than the longest a helper can hold one
/// are removed. Inspection stops after 150 seconds, but a download has no time limit: a
/// long video on a slow connection keeps its session file for as long as yt-dlp runs. As
/// with stale update installers, a day is far beyond any realistic download.
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// The folder session working files are created in.
pub(crate) fn folder() -> PathBuf {
    std::env::temp_dir()
}

/// Removes session working files and folders older than a day. Errors are ignored.
pub fn remove_stale_session_files() {
    if let Some(cutoff) = SystemTime::now().checked_sub(MAX_AGE) {
        remove_older_than(&folder(), cutoff);
    }
}

fn is_session_name(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| name.starts_with(PREFIX))
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_: &std::fs::Metadata) -> bool {
    false
}

/// Removes matching entries of `directory` last modified before `cutoff`.
fn remove_older_than(directory: &Path, cutoff: SystemTime) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if !is_session_name(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        // Not followed: a link's own metadata, never its target's.
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let kind = metadata.file_type();
        if kind.is_symlink() || is_reparse_point(&metadata) {
            continue;
        }
        if !metadata.modified().is_ok_and(|modified| modified < cutoff) {
            continue;
        }
        // `remove_dir_all` removes links inside the folder without following them.
        let _ = if kind.is_dir() {
            std::fs::remove_dir_all(&path)
        } else if kind.is_file() {
            std::fs::remove_file(&path)
        } else {
            continue;
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::Source, session::SessionCookies};

    const HOUR: Duration = Duration::from_secs(3600);

    fn later() -> SystemTime {
        SystemTime::now() + HOUR
    }

    #[test]
    fn old_matching_entries_are_removed_and_others_kept() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let old_file = dir.join(format!("{PREFIX}old.txt"));
        let old_folder = dir.join(format!("{PREFIX}folder"));
        let fresh = dir.join(format!("{PREFIX}fresh.txt"));
        let kept = [
            "stream-dlp-sessions-old.txt",
            "stream-dlp-session.txt",
            "stream-dlp-Setup-old.exe",
            "other-stream-dlp-session-old.txt",
            "unrelated.txt",
        ]
        .map(|name| dir.join(name));
        std::fs::write(&old_file, "fake").unwrap();
        std::fs::create_dir(&old_folder).unwrap();
        std::fs::write(old_folder.join("cookies.txt"), "fake").unwrap();
        std::fs::write(&fresh, "fake").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&fresh)
            .unwrap()
            .set_modified(later() + HOUR)
            .unwrap();
        for path in &kept {
            std::fs::write(path, "fake").unwrap();
        }

        // Everything but `fresh` was modified before the cutoff.
        remove_older_than(dir, later());

        assert!(!old_file.exists());
        assert!(!old_folder.exists());
        assert!(fresh.exists(), "A file in use by another instance stays");
        for path in &kept {
            assert!(path.exists(), "{} was removed", path.display());
        }
    }

    #[test]
    fn entries_newer_than_the_cutoff_are_kept() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join(format!("{PREFIX}live.txt"));
        std::fs::write(&file, "fake").unwrap();
        remove_older_than(temp.path(), SystemTime::now() - HOUR);
        assert!(file.exists());
    }

    #[test]
    fn a_missing_folder_is_not_an_error() {
        let temp = tempfile::tempdir().unwrap();
        remove_older_than(&temp.path().join("missing"), later());
    }

    #[test]
    fn links_are_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "fake").unwrap();
        let swept = temp.path().join("swept");
        std::fs::create_dir(&swept).unwrap();
        let link = swept.join(format!("{PREFIX}link"));
        #[cfg(windows)]
        let created = std::os::windows::fs::symlink_dir(&outside, &link);
        #[cfg(unix)]
        let created = std::os::unix::fs::symlink(&outside, &link);
        if created.is_err() {
            // Windows allows symbolic links only with Developer Mode or elevation.
            return;
        }
        remove_older_than(&swept, later());
        assert!(outside.join("keep.txt").exists());
        assert!(
            std::fs::symlink_metadata(&link).is_ok(),
            "Links are skipped"
        );
    }

    // Junctions need no privilege, so this runs on every Windows machine.
    #[cfg(windows)]
    #[test]
    fn junctions_are_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "fake").unwrap();
        let swept = temp.path().join("swept");
        std::fs::create_dir(&swept).unwrap();
        let junction = swept.join(format!("{PREFIX}junction"));
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&junction)
            .arg(&outside)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        remove_older_than(&swept, later());
        assert!(outside.join("keep.txt").exists());
        assert!(junction.exists(), "Junctions are skipped");
    }

    #[test]
    fn session_working_files_are_created_where_the_sweep_looks_with_its_prefix() {
        let session = SessionCookies::parse_for(
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tTEST\tfake\n",
            Source::YouTube,
        )
        .unwrap();
        let file = session.temporary_file().unwrap();
        assert_eq!(file.path().parent(), Some(folder().as_path()));
        assert!(is_session_name(file.path().file_name().unwrap()));

        let temp = tempfile::tempdir().unwrap();
        let file = session.temporary_file_in(temp.path()).unwrap();
        let path = file.path().to_owned();
        // Leaked as after a crash, so only the sweep can remove it.
        file.keep().unwrap();
        remove_older_than(temp.path(), later());
        assert!(!path.exists());
    }
}
