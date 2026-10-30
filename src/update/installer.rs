//! Downloading a release's installer and starting it (Windows).
use super::release::Asset;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
    time::Duration,
};

const UPDATE_PREFIX: &str = "stream-dlp-update-";

/// Whether setup installed this copy (its uninstaller is next to the app) rather than it being
/// unpacked from the ZIP. Installed copies update silently; portable copies open setup.
pub fn installed() -> bool {
    cfg!(windows)
        && std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join("unins000.exe").is_file()))
            .unwrap_or(false)
}

/// A downloaded installer whose SHA-256 matched the release's digest.
pub struct Installer {
    directory: tempfile::TempDir,
    path: PathBuf,
}

pub fn download(
    asset: &Asset,
    cancel: &AtomicBool,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<Installer, String> {
    let directory = tempfile::Builder::new()
        .prefix(UPDATE_PREFIX)
        .tempdir()
        .map_err(|e| e.to_string())?;
    let path = directory.path().join(asset.name);
    crate::net::download(&asset.url, &asset.sha256, &path, cancel, progress)?;
    Ok(Installer { directory, path })
}

impl Installer {
    /// `silent` installs without the wizard and reopens the app afterwards (the installer's
    /// `/RELAUNCH=1`); otherwise setup opens for the user to review.
    pub fn launch(self, silent: bool) -> Result<(), String> {
        let mut command = Command::new(&self.path);
        if silent {
            command.args([
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/RELAUNCH=1",
            ]);
        }
        command
            .spawn()
            .map_err(|e| format!("Cannot open installer: {e}"))?;
        // The installer is a separate process and still needs this file after the app exits.
        let _ = self.directory.keep();
        Ok(())
    }
}

/// Removes the installer folders that `Installer::launch` kept for setup a day or more ago.
pub fn remove_stale_installers() {
    remove_stale_in(&std::env::temp_dir(), Duration::from_secs(24 * 60 * 60));
}

fn remove_stale_in(directory: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .file_name()
            .to_string_lossy()
            .starts_with(UPDATE_PREFIX)
            && entry.file_type().is_ok_and(|t| t.is_dir())
            && entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|elapsed| elapsed > age);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_installer_folders_are_removed_and_others_kept() {
        let temp = tempfile::tempdir().unwrap();
        let stale = temp.path().join(format!("{UPDATE_PREFIX}old"));
        let unrelated = temp.path().join("other-update-old");
        let file = temp.path().join(format!("{UPDATE_PREFIX}file"));
        std::fs::create_dir(&stale).unwrap();
        std::fs::write(stale.join("setup.exe"), b"setup").unwrap();
        std::fs::create_dir(&unrelated).unwrap();
        std::fs::write(&file, b"not a folder").unwrap();
        remove_stale_in(temp.path(), Duration::from_secs(3600));
        assert!(stale.exists(), "Recent installers stay for setup");
        std::thread::sleep(Duration::from_millis(50));
        remove_stale_in(temp.path(), Duration::ZERO);
        assert!(!stale.exists());
        assert!(unrelated.exists());
        assert!(file.exists());
    }
}
