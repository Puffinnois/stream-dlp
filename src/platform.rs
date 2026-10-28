//! Small OS boundary shared by the UI and download engine.
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub fn executable(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn bundled_tools(exe: &Path) -> PathBuf {
    let parent = exe.parent().unwrap_or(Path::new("."));
    if parent.file_name().is_some_and(|name| name == "MacOS")
        && parent
            .parent()
            .is_some_and(|p| p.file_name().is_some_and(|n| n == "Contents"))
    {
        parent.parent().unwrap().join("Resources/tools")
    } else {
        parent.join("tools")
    }
}

pub fn tools_dir() -> PathBuf {
    let bundled = std::env::current_exe()
        .map(|p| bundled_tools(&p))
        .unwrap_or_else(|_| PathBuf::from("tools"));
    // A packaged app never falls back to a developer's source directory.
    if bundled.is_dir() || !cfg!(debug_assertions) {
        bundled
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools")
    }
}

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

pub fn tools_help() -> &'static str {
    if cfg!(target_os = "macos") {
        "Download a fresh Mac app package from Releases to install or update the bundled tools."
    } else {
        "Close stream-dlp and run Setup tools.ps1 from the app folder to install or update tools."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_tools_are_found_without_the_working_directory() {
        assert_eq!(
            bundled_tools(Path::new(
                "/Applications/stream-dlp.app/Contents/MacOS/stream-dlp"
            )),
            PathBuf::from("/Applications/stream-dlp.app/Contents/Resources/tools")
        );
        assert_eq!(
            bundled_tools(Path::new("portable/stream-dlp")),
            PathBuf::from("portable/tools")
        );
    }
}
