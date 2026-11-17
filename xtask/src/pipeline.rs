//! `cargo xtask pipeline`: builds the app, then runs the `#[ignore]`d tests that need only the
//! bundled tools, the built app and localhost, one by one by their full name. Required for
//! changes under `src/engine/` and `src/media/` (AGENTS.md). Tests that need the network, an
//! account, a browser profile, the user's own files, a separately started service, or that
//! upload anything stay out.
use std::{
    env,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

use crate::{cargo, root, tools};

/// Full libtest paths, run with `--exact`.
pub const TESTS: &[&str] = &[
    "engine::ytdlp::tests::local_download_pipeline_saves_playable_file",
    "engine::ytdlp::tests::other_sites_local_fixtures_inspect_and_download",
    "engine::ytdlp::tests::scoped_cookies_follow_helper_request_rules",
    "media::local::tests::local_library_probe_checks_real_video_and_invalid_files",
    "media::playback::tests::local_playback_decodes_seeks_and_stops",
    "app::player::tests::local_player_plays_pauses_seeks_and_ends",
    "session::chrome::host::tests::built_app_host_mode_relays_framed_messages",
];

/// Builds the app itself: the host-mode test runs the built executable.
const BUILD: [&str; 6] = [
    "build",
    "--locked",
    "-p",
    "stream-dlp",
    "--bin",
    "stream-dlp",
];

/// The app's test binary; the pipeline tests live there.
const TARGET: [&str; 6] = [
    "test",
    "--locked",
    "-p",
    "stream-dlp",
    "--bin",
    "stream-dlp",
];

fn list_args() -> Vec<&'static str> {
    [&TARGET[..], &["--", "--list", "--ignored"]].concat()
}

/// Always one test by its exact name: never `--ignored` without one.
fn test_args(name: &str) -> Vec<&str> {
    [&TARGET[..], &["--", "--ignored", "--exact", name]].concat()
}

/// Names from `--list` output (`path::to::test: test`).
fn listed(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect()
}

/// The pipeline tests missing from the listing.
fn missing<'a>(tests: &[&'a str], listed: &[&str]) -> Vec<&'a str> {
    tests
        .iter()
        .copied()
        .filter(|name| !listed.contains(name))
        .collect()
}

/// `test result: ok. 1 passed; 0 failed; …` → 1.
fn passed(summary: &str) -> Option<u32> {
    summary
        .strip_prefix("test result: ")?
        .split(';')
        .find_map(|part| {
            part.trim()
                .strip_suffix(" passed")?
                .rsplit(' ')
                .next()?
                .parse()
                .ok()
        })
}

/// Variables the opt-in tests read (`STREAM_DLP_VIDEO_TEST_FOLDER` would make
/// `local_library_probe` read a real folder), removed so the pipeline never reaches outside data.
fn scrubbed(vars: impl Iterator<Item = String>) -> Vec<String> {
    vars.filter(|name| name.starts_with("STREAM_DLP_"))
        .collect()
}

fn command(args: &[&str]) -> Command {
    let mut command = Command::new(cargo());
    command.args(args).current_dir(root());
    for name in scrubbed(env::vars_os().filter_map(|(k, _)| k.into_string().ok())) {
        command.env_remove(name);
    }
    command
}

pub fn run() -> Result<(), String> {
    for name in ["yt-dlp", "ffmpeg", "ffprobe"] {
        tools::require(name)?;
    }
    println!("> cargo {}", BUILD.join(" "));
    if !command(&BUILD)
        .status()
        .map_err(|e| format!("cannot start cargo: {e}"))?
        .success()
    {
        return Err("building the app failed".into());
    }
    let args = list_args();
    println!("> cargo {}", args.join(" "));
    let output = command(&args)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot start cargo: {e}"))?;
    if !output.status.success() {
        return Err("listing the ignored tests failed".into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let gone = missing(TESTS, &listed(&stdout));
    if !gone.is_empty() {
        return Err(format!(
            "pipeline tests no longer exist as ignored tests: {}. Update TESTS in xtask/src/pipeline.rs",
            gone.join(", ")
        ));
    }
    for name in TESTS {
        run_one(name)?;
    }
    println!("All {} pipeline tests passed.", TESTS.len());
    Ok(())
}

/// Streams the test's output and requires exactly one passed test.
fn run_one(name: &str) -> Result<(), String> {
    let args = test_args(name);
    println!("> cargo {}", args.join(" "));
    let mut child = command(&args)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start cargo: {e}"))?;
    let mut count = None;
    for line in BufReader::new(child.stdout.take().expect("piped")).lines() {
        let line = line.map_err(|e| e.to_string())?;
        println!("{line}");
        if let Some(n) = passed(&line) {
            count = Some(n);
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    match (status.success(), count) {
        (true, Some(1)) => Ok(()),
        (true, n) => Err(format!(
            "{name} ran {} tests instead of one",
            n.unwrap_or(0)
        )),
        (false, _) => Err(format!("{name} failed")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_run_names_one_exact_test() {
        for name in TESTS {
            let args = test_args(name);
            let tail = &args[args.iter().position(|a| *a == "--").unwrap()..];
            assert_eq!(tail, ["--", "--ignored", "--exact", *name]);
        }
        assert_eq!(list_args().last(), Some(&"--ignored"));
        assert!(list_args().contains(&"--list"));
    }

    #[test]
    fn a_renamed_test_is_reported_missing() {
        let output = "engine::ytdlp::tests::local_download_pipeline_saves_playable_file: test\n\
                      media::local::tests::local_library_probe_checks_real_video_and_invalid_files: test\n\
                      \n2 tests, 0 benchmarks\n";
        let listed = listed(output);
        assert_eq!(listed.len(), 2);
        assert_eq!(
            missing(TESTS, &listed),
            [
                "engine::ytdlp::tests::other_sites_local_fixtures_inspect_and_download",
                "engine::ytdlp::tests::scoped_cookies_follow_helper_request_rules",
                "media::playback::tests::local_playback_decodes_seeks_and_stops",
                "app::player::tests::local_player_plays_pauses_seeks_and_ends",
                "session::chrome::host::tests::built_app_host_mode_relays_framed_messages"
            ]
        );
        assert!(missing(TESTS, TESTS).is_empty());
    }

    #[test]
    fn the_summary_counts_passed_tests() {
        assert_eq!(
            passed(
                "test result: ok. 1 passed; 0 failed; 15 ignored; 0 measured; 300 filtered out; finished in 3.21s"
            ),
            Some(1)
        );
        assert_eq!(
            passed("test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 316 filtered out"),
            Some(0)
        );
        assert_eq!(
            passed("test result: FAILED. 0 passed; 1 failed; 0 ignored"),
            Some(0)
        );
        assert_eq!(passed("running 1 test"), None);
    }

    #[test]
    fn opt_in_variables_are_removed() {
        let vars = [
            "PATH",
            "STREAM_DLP_VIDEO_TEST_FOLDER",
            "STREAM_DLP_TEST_URL",
            "CARGO",
        ]
        .map(String::from);
        assert_eq!(
            scrubbed(vars.into_iter()),
            ["STREAM_DLP_VIDEO_TEST_FOLDER", "STREAM_DLP_TEST_URL"]
        );
    }
}
