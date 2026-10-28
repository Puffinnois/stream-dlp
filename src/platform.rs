//! Small OS boundary shared by the UI and download engine.
use std::{path::Path, process::Command};

pub fn open_folder(folder: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(windows)]
    let mut command = Command::new("explorer.exe");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    crate::process::isolate(command.arg(folder))
        .spawn()
        .map(|_| ())
}
