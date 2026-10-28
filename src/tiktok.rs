//! Bounded, anonymous short-link resolution using the bundled Deno runtime.
use crate::process;
use std::{
    io::Write,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const NOT_RESOLVED: &str = "Could not resolve this TikTok share link. Open it in your browser and copy the full video link.";

pub fn resolve(link: &str, cancel: &AtomicBool) -> Result<Option<String>, String> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut script = tempfile::Builder::new()
        .suffix(".mjs")
        .tempfile()
        .map_err(|_| "Could not prepare TikTok link resolver.")?;
    script
        .write_all(include_bytes!("../scripts/tiktok-resolve.js"))
        .map_err(|_| "Could not prepare TikTok link resolver.")?;
    let output = match process::Helper::new(
        Command::new(crate::tools::path("deno"))
            .args(["run", "--no-config", "--no-lock", "--no-prompt", "--allow-net=tiktok.com:443,www.tiktok.com:443,m.tiktok.com:443,vm.tiktok.com:443,vt.tiktok.com:443"])
            .arg(script.path())
            .arg(link),
        cancel,
    )
    .time_limit(Duration::from_secs(20))
    .output_limit(8192)
    .ignore_stderr()
    .output()
    {
        Ok(output) => output,
        Err(process::Error::Cancelled) => return Ok(None),
        Err(process::Error::TimedOut) => {
            return Err("TikTok share-link resolution timed out. Try the full video link.".into());
        }
        Err(process::Error::Start(_)) => {
            return Err(
                "Could not start TikTok link resolver. Check the download tools in Settings."
                    .into(),
            );
        }
        Err(process::Error::TooMuchOutput) => return Err(NOT_RESOLVED.into()),
        Err(process::Error::Wait(_)) => {
            return Err("Could not resolve the TikTok share link.".into());
        }
    };
    if !output.status.success() {
        return Err(NOT_RESOLVED.into());
    }
    let url = crate::backend::normalize_url(String::from_utf8_lossy(&output.stdout).trim())?;
    if crate::backend::source(&url) != Some(crate::backend::Source::TikTok)
        || crate::backend::is_tiktok_short(&url)
    {
        return Err("The TikTok share link did not lead to an individual video.".into());
    }
    Ok(Some(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_resolution_does_not_start_a_helper() {
        assert!(
            resolve("https://vm.tiktok.com/ABC/", &AtomicBool::new(true))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn embedded_resolver_runs_without_network_for_direct_video() {
        if !crate::tools::ready() {
            return;
        }
        let link = "https://www.tiktok.com/@creator/video/123";
        assert_eq!(
            resolve(link, &AtomicBool::new(false)).unwrap().as_deref(),
            Some(link)
        );
    }
}
