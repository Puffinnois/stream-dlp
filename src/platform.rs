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

const MOVED: &str = "This video was moved or deleted. Choose it again.";
const FILE_MOVED: &str = "This file was moved or deleted. Choose it again.";

/// Opens a file with the app the system opens it with, as a double click in the file manager
/// does. Unlike `open_target`, which hands Windows a URL, any file name works (`url.dll` loses
/// letters outside the system's code page), and a missing file or one that no app opens is an
/// error rather than nothing happening.
pub fn open_file(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(FILE_MOVED.into());
    }
    #[cfg(windows)]
    return shell::open(path).map_err(|error| open_error(error.raw_os_error()));
    #[cfg(not(windows))]
    open_target(path.as_os_str()).map_err(|_| open_error(None))
}

/// What the person reads when the system did not open a file, from the Windows error code.
fn open_error(code: Option<i32>) -> String {
    match code {
        // ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND
        Some(2 | 3) => FILE_MOVED.into(),
        // ERROR_NO_ASSOCIATION
        Some(1155) => "No app on this computer is set to open this kind of file. Choose one in the file’s Open with menu.".into(),
        // ERROR_ACCESS_DENIED
        Some(5) => "Windows did not allow opening this file. Open it from its folder.".into(),
        _ => "Could not open the file in your player. Open it from its folder.".into(),
    }
}

#[cfg(windows)]
mod shell {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path, ptr::null_mut};

    /// `SHELLEXECUTEINFOW`.
    #[repr(C)]
    pub(super) struct ExecuteInfo {
        size: u32,
        mask: u32,
        window: *mut c_void,
        verb: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
        instance: *mut c_void,
        id_list: *mut c_void,
        class: *const u16,
        class_key: *mut c_void,
        hot_key: u32,
        icon_or_monitor: *mut c_void,
        process: *mut c_void,
    }

    /// Returns only once the app was started, and fails instead of asking with a dialog.
    const SEE_MASK_NOASYNC: u32 = 0x100;
    const SEE_MASK_FLAG_NO_UI: u32 = 0x400;
    const SW_SHOWNORMAL: i32 = 1;

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteExW(info: *mut ExecuteInfo) -> i32;
    }

    /// The file's default action, the one a double click runs.
    pub fn open(path: &Path) -> std::io::Result<()> {
        let file: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut info = ExecuteInfo {
            size: size_of::<ExecuteInfo>() as u32,
            mask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            window: null_mut(),
            // No verb: the default one.
            verb: std::ptr::null(),
            file: file.as_ptr(),
            parameters: std::ptr::null(),
            directory: std::ptr::null(),
            show: SW_SHOWNORMAL,
            instance: null_mut(),
            id_list: null_mut(),
            class: std::ptr::null(),
            class_key: null_mut(),
            hot_key: 0,
            icon_or_monitor: null_mut(),
            process: null_mut(),
        };
        // SAFETY: `info` is a valid SHELLEXECUTEINFOW whose strings outlive the call, and no
        // mask asks for a process handle that would need closing.
        if unsafe { ShellExecuteExW(&mut info) } != 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

pub fn reveal(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(MOVED.into());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_says_so_before_anything_is_started() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["gone.mp4", "café ñ 日本 #2 & 100%.mp4"] {
            assert_eq!(open_file(&dir.path().join(name)).unwrap_err(), FILE_MOVED);
        }
        assert_eq!(open_file(dir.path()).unwrap_err(), FILE_MOVED, "A folder");
    }

    #[test]
    fn system_errors_read_as_what_to_do() {
        assert_eq!(open_error(Some(2)), FILE_MOVED);
        assert_eq!(open_error(Some(3)), FILE_MOVED);
        assert!(open_error(Some(1155)).contains("Open with"));
        assert!(open_error(None).contains("folder"));
    }

    #[cfg(all(windows, target_pointer_width = "64"))]
    #[test]
    fn the_shell_structure_has_the_system_size() {
        // sizeof(SHELLEXECUTEINFOW) on 64-bit Windows.
        assert_eq!(size_of::<shell::ExecuteInfo>(), 112);
    }
}
