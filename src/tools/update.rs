//! Installs or updates the helpers from their official GitHub releases, as `Setup tools.ps1`
//! does (Windows). Every download must match GitHub's SHA-256 digest, and a helper whose latest
//! release is the one recorded in `versions.json` is not downloaded again.
use crate::net;
use eframe::egui::Context;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

/// A helper's official release asset and what is taken from it.
struct Helper {
    name: &'static str,
    repository: &'static str,
    asset: &'static str,
    /// The asset itself, or files found by name anywhere in the zip.
    files: &'static [&'static str],
    /// License notices fetched from the project (URL, file name in `licenses/`).
    notices: &'static [(&'static str, &'static str)],
}

const HELPERS: [Helper; 3] = [
    Helper {
        name: "yt-dlp",
        repository: "yt-dlp/yt-dlp",
        asset: "yt-dlp.exe",
        files: &["yt-dlp.exe"],
        notices: &[
            (
                "https://raw.githubusercontent.com/yt-dlp/yt-dlp/master/LICENSE",
                "yt-dlp-LICENSE.txt",
            ),
            (
                "https://raw.githubusercontent.com/yt-dlp/yt-dlp/master/THIRD_PARTY_LICENSES.txt",
                "yt-dlp-THIRD_PARTY_LICENSES.txt",
            ),
        ],
    },
    Helper {
        name: "FFmpeg",
        repository: "yt-dlp/FFmpeg-Builds",
        asset: "ffmpeg-master-latest-win64-gpl.zip",
        files: &["ffmpeg.exe", "ffprobe.exe"],
        notices: &[],
    },
    Helper {
        name: "Deno",
        repository: "denoland/deno",
        asset: "deno-x86_64-pc-windows-msvc.zip",
        files: &["deno.exe"],
        notices: &[(
            "https://raw.githubusercontent.com/denoland/deno/main/LICENSE.md",
            "deno-LICENSE.md",
        )],
    },
];

/// An entry of `versions.json`, in the format `Setup tools.ps1` writes.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct Installed {
    repository: String,
    version: String,
    sha256: String,
    url: String,
    source: String,
}

fn installed(dir: &Path) -> Vec<Installed> {
    fs::read_to_string(dir.join("versions.json"))
        .ok()
        .and_then(|text| serde_json::from_str(text.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

fn save(dir: &Path, records: &[Installed]) -> io::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(&mut file, records)?;
    file.persist(dir.join("versions.json"))?;
    Ok(())
}

fn label(name: &str, version: &str) -> String {
    // The FFmpeg builds are always tagged "latest".
    if version == "latest" {
        format!("{name} (latest build)")
    } else {
        format!("{name} {version}")
    }
}

/// The installed versions for Settings, such as "yt-dlp 2026.08.19 · FFmpeg (latest build)".
fn describe(records: &[Installed]) -> String {
    HELPERS
        .iter()
        .filter_map(|helper| {
            let record = records.iter().find(|r| r.repository == helper.repository)?;
            Some(label(helper.name, &record.version))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

fn parse_latest(helper: &Helper, json: &str) -> Result<Installed, String> {
    let release: Release = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let asset = release
        .assets
        .into_iter()
        .find(|a| a.name == helper.asset)
        .ok_or_else(|| format!("The {} release has no {}.", helper.name, helper.asset))?;
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| {
            format!(
                "GitHub did not supply a SHA-256 digest for {}, so it was not downloaded.",
                helper.asset
            )
        })?
        .to_uppercase();
    let official = format!(
        "https://github.com/{}/releases/download/",
        helper.repository
    );
    if !asset.browser_download_url.starts_with(&official) {
        return Err(format!(
            "The {} download is not an official release file.",
            helper.name
        ));
    }
    Ok(Installed {
        repository: helper.repository.into(),
        source: format!(
            "https://github.com/{}/releases/tag/{}",
            helper.repository, release.tag_name
        ),
        version: release.tag_name,
        sha256,
        url: asset.browser_download_url,
    })
}

/// Whether the helper's files are present and came from this release asset.
fn current(helper: &Helper, latest: &Installed, records: &[Installed], dir: &Path) -> bool {
    records
        .iter()
        .any(|r| r.repository == latest.repository && r.sha256.eq_ignore_ascii_case(&latest.sha256))
        && helper.files.iter().all(|file| dir.join(file).is_file())
}

/// Extracts `files` and the license notices (saved as `licenses/<prefix>-<name>`) into `out`,
/// finding them by file name anywhere in the archive.
fn unpack(archive: &Path, files: &[&str], prefix: &str, out: &Path) -> Result<(), String> {
    let mut zip = fs::File::open(archive)
        .map_err(|e| e.to_string())
        .and_then(|file| zip::ZipArchive::new(file).map_err(|e| e.to_string()))?;
    let mut found = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let Some(name) = entry
            .enclosed_name()
            .and_then(|path| Some(path.file_name()?.to_string_lossy().into_owned()))
            .filter(|_| entry.is_file())
        else {
            continue;
        };
        let upper = name.to_ascii_uppercase();
        let target = if files.contains(&name.as_str()) && !found.contains(&name) {
            found.push(name.clone());
            out.join(&name)
        } else if ["LICENSE", "COPYING", "README"]
            .iter()
            .any(|n| upper.starts_with(n))
        {
            out.join("licenses").join(format!("{prefix}-{name}"))
        } else {
            continue;
        };
        let mut file = fs::File::create(target).map_err(|e| e.to_string())?;
        io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
    }
    match files.iter().find(|file| !found.iter().any(|n| n == *file)) {
        Some(missing) => Err(format!("The archive does not contain {missing}.")),
        None => Ok(()),
    }
}

/// Moves `new` over `target`. A running helper cannot be overwritten but can be renamed, so the
/// old file is moved aside first and removed once nothing uses it.
fn replace(new: &Path, target: &Path) -> io::Result<()> {
    let mut aside = target.as_os_str().to_owned();
    aside.push(".old");
    let _ = fs::remove_file(&aside);
    if target.exists() {
        fs::rename(target, &aside)?;
    }
    if let Err(error) = fs::rename(new, target) {
        let _ = fs::rename(&aside, target);
        return Err(error);
    }
    let _ = fs::remove_file(&aside);
    Ok(())
}

enum Event {
    Status(String),
    /// The share of the current download, when its size is known.
    Progress(Option<f32>),
    Done(Result<String, String>),
}

fn install(
    helper: &Helper,
    latest: &Installed,
    dir: &Path,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let staging = tempfile::Builder::new()
        .prefix(".update-")
        .tempdir_in(dir)
        .map_err(|e| e.to_string())?;
    let notices = staging.path().join("licenses");
    fs::create_dir(&notices).map_err(|e| e.to_string())?;
    let download = staging.path().join(helper.asset);
    net::download(
        &latest.url,
        &latest.sha256,
        &download,
        cancel,
        |received, total| {
            report(Event::Progress(
                total.filter(|&t| t > 0).map(|t| received as f32 / t as f32),
            ))
        },
    )?;
    if helper.asset.ends_with(".zip") {
        let prefix = helper.name.to_ascii_lowercase();
        unpack(&download, helper.files, &prefix, staging.path())?;
    }
    for (url, name) in helper.notices {
        fs::write(notices.join(name), net::get(url)?).map_err(|e| e.to_string())?;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled.".into());
    }
    // Everything is downloaded and verified; only now are the installed files replaced.
    let put = |from: &Path, to: &Path| {
        replace(from, to).map_err(|e| format!("Cannot replace {}: {e}", to.display()))
    };
    for file in helper.files {
        put(&staging.path().join(file), &dir.join(file))?;
    }
    for notice in fs::read_dir(&notices).map_err(|e| e.to_string())?.flatten() {
        put(
            &notice.path(),
            &dir.join("licenses").join(notice.file_name()),
        )?;
    }
    Ok(())
}

/// Brings every helper up to date and says what changed.
fn run(dir: &Path, cancel: &AtomicBool, report: &mut dyn FnMut(Event)) -> Result<String, String> {
    fs::create_dir_all(dir.join("licenses")).map_err(|e| e.to_string())?;
    let mut records = installed(dir);
    let mut updated = Vec::new();
    for helper in &HELPERS {
        report(Event::Status(format!("Checking {}…", helper.name)));
        let json = net::github(&format!("repos/{}/releases/latest", helper.repository))?
            .ok_or_else(|| format!("{} has no release.", helper.name))?;
        let latest = parse_latest(helper, &json)?;
        if current(helper, &latest, &records, dir) {
            continue;
        }
        report(Event::Status(format!(
            "Downloading {}…",
            label(helper.name, &latest.version)
        )));
        install(helper, &latest, dir, cancel, report)?;
        updated.push(label(helper.name, &latest.version));
        records.retain(|r| r.repository != latest.repository);
        records.push(latest);
        save(dir, &records).map_err(|e| format!("Cannot save versions.json: {e}"))?;
    }
    Ok(if updated.is_empty() {
        "All download tools are up to date.".into()
    } else {
        format!("Installed {}.", updated.join(", "))
    })
}

/// Settings' Install / Update download tools.
pub struct ToolUpdate {
    pub active: bool,
    pub status: String,
    pub progress: Option<f32>,
    /// The installed versions, from `versions.json`.
    pub versions: String,
    cancel: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
}

impl Default for ToolUpdate {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            active: false,
            status: String::new(),
            progress: None,
            versions: describe(&installed(super::dir())),
            cancel: Default::default(),
            tx,
            rx,
        }
    }
}

impl ToolUpdate {
    pub fn start(&mut self, ctx: &Context) {
        if self.active {
            return;
        }
        self.active = true;
        self.progress = None;
        self.status = "Checking for new versions…".into();
        self.cancel = Default::default();
        let (tx, ctx, cancel) = (self.tx.clone(), ctx.clone(), self.cancel.clone());
        std::thread::spawn(move || {
            let mut report = |event: Event| {
                let _ = tx.send(event);
                ctx.request_repaint();
            };
            let result = run(super::dir(), &cancel, &mut report);
            report(Event::Done(result));
        });
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Status(status) => {
                    self.status = status;
                    self.progress = None;
                }
                Event::Progress(progress) => self.progress = progress,
                Event::Done(result) => {
                    self.active = false;
                    self.progress = None;
                    self.status = result.unwrap_or_else(|error| {
                        format!("Could not update the download tools: {error}")
                    });
                    self.versions = describe(&installed(super::dir()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn release(helper: &Helper, digest: &str, url: &str) -> String {
        serde_json::json!({"tag_name": "2026.10.01", "assets": [
            {"name": "other.zip", "browser_download_url": url, "digest": digest},
            {"name": helper.asset, "browser_download_url": url, "digest": digest},
        ]})
        .to_string()
    }

    #[test]
    fn latest_release_needs_an_official_asset_with_a_digest() {
        let helper = &HELPERS[0];
        let url = "https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.01/yt-dlp.exe";
        let digest = format!("sha256:{}", "a".repeat(64));
        let latest = parse_latest(helper, &release(helper, &digest, url)).unwrap();
        assert_eq!(latest.sha256, "A".repeat(64), "Stored like Get-FileHash");
        assert_eq!(latest.version, "2026.10.01");
        assert_eq!(
            latest.source,
            "https://github.com/yt-dlp/yt-dlp/releases/tag/2026.10.01"
        );
        assert!(parse_latest(helper, &release(helper, "sha256:short", url)).is_err());
        let foreign = "https://example.com/yt-dlp/yt-dlp/releases/download/x/yt-dlp.exe";
        assert!(parse_latest(helper, &release(helper, &digest, foreign)).is_err());
        assert!(parse_latest(&HELPERS[1], &release(helper, &digest, url)).is_err());
    }

    #[test]
    fn a_recorded_helper_is_not_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        // Setup tools.ps1 writes versions.json with a byte order mark.
        let json = r#"[{"Repository":"yt-dlp/yt-dlp","Version":"2026.08.19","Sha256":"AB","Url":"u","Source":"s"},
            {"Repository":"yt-dlp/FFmpeg-Builds","Version":"latest","Sha256":"CD","Url":"u","Source":"s"}]"#;
        fs::write(dir.path().join("versions.json"), format!("\u{feff}{json}")).unwrap();
        let records = installed(dir.path());
        assert_eq!(
            describe(&records),
            "yt-dlp 2026.08.19 · FFmpeg (latest build)"
        );
        let mut latest = records[0].clone();
        latest.sha256 = "ab".into();
        assert!(
            !current(&HELPERS[0], &latest, &records, dir.path()),
            "File missing"
        );
        fs::write(dir.path().join("yt-dlp.exe"), b"exe").unwrap();
        assert!(current(&HELPERS[0], &latest, &records, dir.path()));
        latest.sha256 = "ef".into();
        assert!(
            !current(&HELPERS[0], &latest, &records, dir.path()),
            "New build"
        );
        save(dir.path(), &records).unwrap();
        assert_eq!(installed(dir.path()), records);
    }

    #[test]
    fn archives_give_their_helpers_and_notices() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("ffmpeg.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in [
            ("ffmpeg-master/bin/ffmpeg.exe", "ffmpeg"),
            ("ffmpeg-master/bin/ffprobe.exe", "ffprobe"),
            ("ffmpeg-master/bin/ffplay.exe", "ffplay"),
            ("ffmpeg-master/LICENSE.txt", "license"),
            ("ffmpeg-master/doc/readme.html", "readme"),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let out = dir.path().join("out");
        fs::create_dir_all(out.join("licenses")).unwrap();
        unpack(&archive, HELPERS[1].files, "ffmpeg", &out).unwrap();
        assert_eq!(
            fs::read_to_string(out.join("ffprobe.exe")).unwrap(),
            "ffprobe"
        );
        assert!(!out.join("ffplay.exe").exists());
        assert!(out.join("licenses/ffmpeg-LICENSE.txt").is_file());
        assert!(out.join("licenses/ffmpeg-readme.html").is_file());
        let error = unpack(&archive, &["deno.exe"], "deno", &out).unwrap_err();
        assert!(error.contains("deno.exe"));
    }

    #[test]
    #[ignore = "downloads about 250 MB of helpers from GitHub"]
    fn live_helpers_install_into_an_empty_folder_once() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let summary = run(dir.path(), &cancel, &mut |_| {}).unwrap();
        assert!(summary.starts_with("Installed yt-dlp"), "{summary}");
        for name in ["yt-dlp", "ffmpeg", "ffprobe", "deno"] {
            assert!(dir.path().join(format!("{name}.exe")).is_file(), "{name}");
        }
        assert!(dir.path().join("licenses/ffmpeg-LICENSE.txt").is_file());
        assert!(dir.path().join("licenses/deno-LICENSE.md").is_file());
        assert_eq!(installed(dir.path()).len(), 3);
        let again = run(dir.path(), &cancel, &mut |_| {}).unwrap();
        assert_eq!(again, "All download tools are up to date.");
    }

    #[test]
    fn replacing_a_helper_leaves_no_old_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (new, target) = (dir.path().join("new"), dir.path().join("yt-dlp.exe"));
        fs::write(&target, b"old").unwrap();
        fs::write(&new, b"new").unwrap();
        replace(&new, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!new.exists());
        assert!(!dir.path().join("yt-dlp.exe.old").exists());
        fs::write(&new, b"first").unwrap();
        replace(&new, &dir.path().join("deno.exe")).unwrap();
        assert_eq!(fs::read(dir.path().join("deno.exe")).unwrap(), b"first");
    }
}
