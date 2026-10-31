//! Dev mode: developer tools inside the one app (today the dev notes), its setting, and the
//! recent-errors log a note can include. The UI is `app::dev`; see `docs/reference/dev-notes.md`.
pub mod errors;
pub mod notes;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The app's data folder: `%LOCALAPPDATA%\stream-dlp` (Mac: `~/Library/Application Support/stream-dlp`).
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("stream-dlp"))
}

/// `settings.json` in the data folder. Unset fields keep their defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_notes_dir: Option<PathBuf>,
}

impl Settings {
    pub fn load() -> Self {
        data_dir()
            .map(|dir| Self::load_from(&dir.join("settings.json")))
            .unwrap_or_default()
    }
    fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        let dir = data_dir().ok_or("The app data folder is unknown.")?;
        self.save_to(&dir.join("settings.json"))
    }
    fn save_to(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        write_atomically(path, &json)
    }
    /// On in a debug build (`cargo run`) and off in a release build, until it is switched.
    pub fn dev_mode(&self) -> bool {
        self.dev_mode.unwrap_or(cfg!(debug_assertions))
    }
    pub fn notes_dir(&self) -> Option<PathBuf> {
        self.dev_notes_dir
            .clone()
            .or_else(|| data_dir().map(|dir| dir.join("dev-notes")))
    }
}

/// Writes a file through a temporary file beside it, so a reader never sees half of it.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let describe =
        |error: &dyn std::fmt::Display| format!("Cannot write {}: {error}", path.display());
    let parent = path.parent().ok_or_else(|| describe(&"no folder"))?;
    std::fs::create_dir_all(parent).map_err(|error| describe(&error))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| describe(&error))?;
    std::io::Write::write_all(&mut file, bytes).map_err(|error| describe(&error))?;
    file.persist(path).map_err(|error| describe(&error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_keep_unset_fields_and_survive_a_bad_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data/settings.json");
        assert_eq!(Settings::load_from(&path), Settings::default());
        assert_eq!(Settings::default().dev_mode(), cfg!(debug_assertions));
        let settings = Settings {
            dev_mode: Some(true),
            dev_notes_dir: None,
        };
        settings.save_to(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\n  \"dev_mode\": true\n}"
        );
        assert_eq!(Settings::load_from(&path), settings);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
    }
}
