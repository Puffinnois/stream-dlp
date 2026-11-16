//! The bundled helpers in the gitignored `tools/`: `cargo xtask tools` prints their versions.
use std::{env, path::PathBuf, process::Command};

use crate::{notes, root};

pub const MISSING_HINT: &str =
    "Use Settings → Download tools in the app or run Setup tools.ps1 to set up tools/.";

/// `tools/<name>` with the platform's executable suffix.
pub fn path(name: &str) -> PathBuf {
    root()
        .join("tools")
        .join(format!("{name}{}", env::consts::EXE_SUFFIX))
}

/// `tools/<name>` if it exists, else an error that says how to set up the tools.
pub fn require(name: &str) -> Result<PathBuf, String> {
    let path = path(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("{} is missing. {MISSING_HINT}", path.display()))
    }
}

/// The first line a helper prints for its version argument.
fn first_line(name: &str, arg: &str) -> Result<String, String> {
    let output = Command::new(require(name)?)
        .arg(arg)
        .output()
        .map_err(|e| format!("cannot start tools/{name}: {e}"))?;
    if !output.status.success() {
        return Err(format!("tools/{name} {arg} failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned())
}

/// What the bundled `tools/yt-dlp --version` reports.
pub fn ytdlp_version() -> Result<String, String> {
    let version = first_line("yt-dlp", "--version")?;
    if notes::is_version(&version) {
        Ok(version)
    } else {
        Err(format!(
            "tools/yt-dlp --version printed {version:?}, not a version"
        ))
    }
}

/// `ffmpeg version N-127911-… Copyright …` → `N-127911-…`; `deno 2.9.8 (…)` → `2.9.8`.
fn version_word(first_line: &str, index: usize) -> String {
    first_line
        .split_whitespace()
        .nth(index)
        .unwrap_or(first_line)
        .to_owned()
}

pub fn print_versions() -> Result<(), String> {
    let ytdlp = ytdlp_version()?;
    let ffmpeg = first_line("ffmpeg", "-version").map(|l| version_word(&l, 2));
    let ffprobe = first_line("ffprobe", "-version").map(|l| version_word(&l, 2));
    let deno = first_line("deno", "--version").map(|l| version_word(&l, 1));
    println!("yt-dlp   {ytdlp}");
    let mut missing = false;
    for (name, version) in [("FFmpeg", ffmpeg), ("FFprobe", ffprobe), ("Deno", deno)] {
        match version {
            Ok(version) => println!("{name:<8} {version}"),
            Err(error) => {
                missing = true;
                println!("{name:<8} {error}");
            }
        }
    }
    println!();
    println!("Last line of the release notes:");
    println!("{}", notes::line(&ytdlp));
    if missing {
        return Err("some tools are missing or broken".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_words_come_from_the_first_line() {
        assert_eq!(
            version_word(
                "ffmpeg version N-127911-gadfe258a6a-20261021 Copyright (c) 2000-2026",
                2
            ),
            "N-127911-gadfe258a6a-20261021"
        );
        assert_eq!(
            version_word("deno 2.9.8 (stable, release, x86_64-pc-windows-msvc)", 1),
            "2.9.8"
        );
        assert_eq!(version_word("odd", 2), "odd");
    }
}
