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

pub fn open_target(target: &std::ffi::OsStr) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    crate::process::isolate(command.arg(target))
        .spawn()
        .map(|_| ())
        .map_err(|_| {
            "Could not open the browser or video player. Open the file or streamable.com manually."
                .into()
        })
}
pub fn reveal(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("This video was moved or deleted. Choose it again.".into());
    }
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("explorer.exe");
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        c.arg(arg);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = Command::new("/usr/bin/open");
        c.arg("-R").arg(path);
        c
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    return crate::platform::open_folder(path.parent().unwrap_or(Path::new(".")))
        .map_err(|_| "Could not open the video folder.".into());
    #[cfg(any(windows, target_os = "macos"))]
    crate::process::isolate(&mut command)
        .spawn()
        .map(|_| ())
        .map_err(|_| "Could not reveal the video. Open its folder manually.".into())
}
