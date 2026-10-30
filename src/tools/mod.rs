//! The bundled helpers in `tools/`: where they are, whether they are all there, the
//! `--check-tools` self-test, and updating them.
mod update;

pub use update::ToolUpdate;

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};

const ALL: [&str; 4] = ["yt-dlp", "ffmpeg", "ffprobe", "deno"];

fn bundled(exe: &Path) -> PathBuf {
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

/// `tools` next to the executable (`Contents/Resources/tools` in a Mac bundle).
pub fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let bundled = std::env::current_exe()
            .map(|p| bundled(&p))
            .unwrap_or_else(|_| PathBuf::from("tools"));
        // A packaged app never falls back to a developer's source directory.
        if bundled.is_dir() || !cfg!(debug_assertions) {
            bundled
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools")
        }
    })
}

pub fn path(name: &str) -> PathBuf {
    dir().join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// Whether every helper is present. The UI asks several times per frame, so an answer is
/// reused for two seconds; tools installed while the app is open still show up.
pub fn ready() -> bool {
    static CHECKED: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut checked = CHECKED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((at, ready)) = *checked
        && at.elapsed() < Duration::from_secs(2)
    {
        return ready;
    }
    let ready = ALL.iter().all(|name| path(name).is_file());
    *checked = Some((Instant::now(), ready));
    ready
}

pub fn help() -> &'static str {
    if cfg!(target_os = "macos") {
        "Download a fresh Mac app package from Releases to install or update the bundled tools."
    } else {
        "Use Settings → Download tools to install or update them, or close stream-dlp and run Setup tools.ps1 from the app folder."
    }
}

/// Runs each helper's version command. CI uses it (`--check-tools`) to exercise the installed
/// app's bundle lookup without a desktop session, browser profile, or video service.
pub fn check() -> Result<(), String> {
    for name in ALL {
        let path = path(name);
        let flag = if matches!(name, "yt-dlp" | "deno") {
            "--version"
        } else {
            "-version"
        };
        let status = crate::process::isolate(Command::new(&path).arg(flag)).status();
        if !status.is_ok_and(|status| status.success()) {
            return Err(format!("Bundled tool check failed: {}", path.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_tools_are_found_without_the_working_directory() {
        assert_eq!(
            bundled(Path::new(
                "/Applications/stream-dlp.app/Contents/MacOS/stream-dlp"
            )),
            PathBuf::from("/Applications/stream-dlp.app/Contents/Resources/tools")
        );
        assert_eq!(
            bundled(Path::new("portable/stream-dlp")),
            PathBuf::from("portable/tools")
        );
    }
}
