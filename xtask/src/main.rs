//! Project commands, the counterpart of npm scripts:
//!
//! - `cargo xtask check`: formatting, Clippy, the Rust tests and the Deno script tests — what CI runs.
//! - `cargo xtask release <patch|minor|major|dev|X.Y.Z[-dev.N]>`: bumps Cargo.toml and Cargo.lock,
//!   commits `build: bump version to X`, and tags `vX`. `git push --follow-tags` then starts the
//!   Release workflow, which uploads the build to a draft GitHub release.
use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check"] => check(),
        ["release", bump] => release(bump),
        _ => Err(
            "usage: cargo xtask check | cargo xtask release <patch|minor|major|dev|X.Y.Z[-dev.N]>"
                .into(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the repository root")
        .to_owned()
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    println!("> {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(root())
        .status()
        .map_err(|e| format!("cannot start {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed", args.join(" ")))
    }
}

fn output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root())
        .output()
        .map_err(|e| format!("cannot start {program}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn cargo() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn check() -> Result<(), String> {
    let cargo = cargo();
    run(&cargo, &["fmt", "--all", "--", "--check"])?;
    run(
        &cargo,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(&cargo, &["test", "--workspace", "--locked"])?;
    let deno = deno().ok_or(
        "Deno was not found for the script tests. Run Setup tools.ps1 (tools/deno) or put deno on PATH.",
    )?;
    run(
        &deno,
        &[
            "test",
            "--no-config",
            "--no-remote",
            "--no-prompt",
            "scripts/",
        ],
    )?;
    println!("All checks passed.");
    Ok(())
}

/// The app's bundled Deno in `tools/`, else one on PATH (CI installs it there).
fn deno() -> Option<String> {
    let bundled = root()
        .join("tools")
        .join(format!("deno{}", env::consts::EXE_SUFFIX));
    if bundled.is_file() {
        return Some(bundled.to_string_lossy().into_owned());
    }
    Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
        .then(|| "deno".into())
}

fn release(bump: &str) -> Result<(), String> {
    if !output("git", &["status", "--porcelain"])?.is_empty() {
        return Err(
            "commit or stash your changes first; a release starts from a clean tree".into(),
        );
    }
    let branch = output("git", &["branch", "--show-current"])?;
    if branch != "main" {
        return Err(format!("releases are cut from main, not {branch}"));
    }
    let manifest_path = root().join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let current = package_version(&manifest)?;
    let next = next_version(&current, bump)?;
    let tag = format!("v{next}");
    if !output("git", &["tag", "--list", &tag])?.is_empty() {
        return Err(format!("tag {tag} already exists"));
    }
    std::fs::write(
        &manifest_path,
        set_package_version(&manifest, &current, &next)?,
    )
    .map_err(|e| e.to_string())?;
    // Refreshes only the workspace's own entries in Cargo.lock.
    run(&cargo(), &["update", "--workspace", "--offline"])?;
    run("git", &["add", "Cargo.toml", "Cargo.lock"])?;
    run(
        "git",
        &["commit", "-m", &format!("build: bump version to {next}")],
    )?;
    run("git", &["tag", "-a", &tag, "-m", &tag])?;
    println!(
        "Tagged {tag}. Next: git push --follow-tags (the Release workflow drafts the release)."
    );
    Ok(())
}

/// The `version` of the `[package]` table.
fn package_version(manifest: &str) -> Result<String, String> {
    package_version_line(manifest)
        .and_then(|line| line.split('"').nth(1))
        .map(str::to_owned)
        .ok_or_else(|| "cannot find the package version in Cargo.toml".into())
}

fn package_version_line(manifest: &str) -> Option<&str> {
    let mut in_package = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
        } else if in_package && trimmed.starts_with("version") && trimmed.contains('=') {
            return Some(line);
        }
    }
    None
}

/// Rewrites only the package version line, keeping every other byte (and line ending).
fn set_package_version(manifest: &str, current: &str, next: &str) -> Result<String, String> {
    let line = package_version_line(manifest).ok_or("cannot find the package version")?;
    let start = line.as_ptr() as usize - manifest.as_ptr() as usize;
    let replaced = line.replacen(&format!("\"{current}\""), &format!("\"{next}\""), 1);
    Ok(format!(
        "{}{replaced}{}",
        &manifest[..start],
        &manifest[start + line.len()..]
    ))
}

#[derive(Debug, PartialEq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    dev: Option<u64>,
}

impl Version {
    /// `X.Y.Z` or `X.Y.Z-dev.N`; build.ps1 and the updater accept no other prerelease.
    fn parse(text: &str) -> Result<Self, String> {
        let invalid = || format!("{text} is not X.Y.Z or X.Y.Z-dev.N");
        let (core, dev) = match text.split_once('-') {
            Some((core, pre)) => (
                core,
                Some(
                    pre.strip_prefix("dev.")
                        .and_then(|n| n.parse().ok())
                        .ok_or_else(invalid)?,
                ),
            ),
            None => (text, None),
        };
        let parts: Vec<u64> = core
            .split('.')
            .map(|p| p.parse().map_err(|_| invalid()))
            .collect::<Result<_, _>>()?;
        let [major, minor, patch] = parts[..] else {
            return Err(invalid());
        };
        Ok(Self {
            major,
            minor,
            patch,
            dev,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(dev) = self.dev {
            write!(f, "-dev.{dev}")?;
        }
        Ok(())
    }
}

/// `npm version` semantics: a bump from a dev build first releases that dev build's version.
fn next_version(current: &str, bump: &str) -> Result<String, String> {
    let v = Version::parse(current)?;
    let pre = v.dev.is_some();
    let next = match bump {
        "patch" if pre => Version { dev: None, ..v },
        "patch" => Version {
            patch: v.patch + 1,
            dev: None,
            ..v
        },
        "minor" if pre && v.patch == 0 => Version { dev: None, ..v },
        "minor" => Version {
            minor: v.minor + 1,
            patch: 0,
            dev: None,
            ..v
        },
        "major" if pre && v.minor == 0 && v.patch == 0 => Version { dev: None, ..v },
        "major" => Version {
            major: v.major + 1,
            minor: 0,
            patch: 0,
            dev: None,
        },
        "dev" => match v.dev {
            Some(n) => Version {
                dev: Some(n + 1),
                ..v
            },
            None => Version {
                patch: v.patch + 1,
                dev: Some(1),
                ..v
            },
        },
        explicit => Version::parse(explicit)?,
    };
    if next == v {
        return Err(format!("{current} is already the current version"));
    }
    Ok(next.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bumps_follow_npm_version() {
        for (current, bump, next) in [
            ("0.3.8", "patch", "0.3.9"),
            ("0.3.8", "minor", "0.4.0"),
            ("0.3.8", "major", "1.0.0"),
            ("0.3.8", "dev", "0.3.9-dev.1"),
            ("0.3.9-dev.1", "dev", "0.3.9-dev.2"),
            ("0.3.9-dev.2", "patch", "0.3.9"),
            ("0.4.0-dev.1", "minor", "0.4.0"),
            ("0.3.9-dev.1", "minor", "0.4.0"),
            ("0.3.8", "0.5.0-dev.3", "0.5.0-dev.3"),
        ] {
            assert_eq!(
                next_version(current, bump).unwrap(),
                next,
                "{current} {bump}"
            );
        }
        for (current, bump) in [
            ("0.3.8", "0.3.8"),
            ("0.3.8", "0.4.0-beta.1"),
            ("0.3.8", "0.4"),
            ("0.3.8", "next"),
        ] {
            assert!(next_version(current, bump).is_err(), "{current} {bump}");
        }
    }

    #[test]
    fn only_the_package_version_line_changes() {
        let manifest = "[package]\r\nname = \"stream-dlp\"\r\nversion = \"0.3.8\"\r\nrust-version = \"1.93\"\r\n\r\n[dependencies]\r\nsemver = { version = \"1\" }\r\n";
        assert_eq!(package_version(manifest).unwrap(), "0.3.8");
        let updated = set_package_version(manifest, "0.3.8", "0.3.9").unwrap();
        assert_eq!(updated, manifest.replace("\"0.3.8\"", "\"0.3.9\""));
        assert!(package_version("[dependencies]\nversion = \"1\"\n").is_err());
    }
}
