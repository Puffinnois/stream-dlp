//! Bounded, anonymous short-link resolution using the bundled Deno runtime.
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

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
    let mut child = crate::backend::hidden(Command::new(crate::backend::tools_dir().join(crate::platform::executable("deno")))
        .args(["run", "--no-config", "--no-lock", "--no-prompt", "--allow-net=tiktok.com:443,www.tiktok.com:443,m.tiktok.com:443,vm.tiktok.com:443,vt.tiktok.com:443"])
        .arg(script.path()).arg(link))
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| "Could not start TikTok link resolver. Check the download tools in Settings.")?;
    let stdout = child.stdout.take().unwrap();
    let reader = thread::spawn(move || {
        let mut text = String::new();
        stdout.take(8193).read_to_string(&mut text).map(|_| text)
    });
    let start = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(20) {
            crate::backend::stop(&mut child);
            let _ = reader.join();
            return if cancel.load(Ordering::Relaxed) {
                Ok(None)
            } else {
                Err("TikTok share-link resolution timed out. Try the full video link.".into())
            };
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                crate::backend::stop(&mut child);
                let _ = reader.join();
                return Err("Could not resolve the TikTok share link.".into());
            }
        }
    };
    let output = reader.join().ok().and_then(Result::ok).unwrap_or_default();
    if !status.success() || output.len() > 8192 {
        return Err("Could not resolve this TikTok share link. Open it in your browser and copy the full video link.".into());
    }
    let url = crate::backend::normalize_url(output.trim())?;
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
        if !crate::backend::tools_ready() {
            return;
        }
        let link = "https://www.tiktok.com/@creator/video/123";
        assert_eq!(
            resolve(link, &AtomicBool::new(false)).unwrap().as_deref(),
            Some(link)
        );
    }
}
