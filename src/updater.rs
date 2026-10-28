//! Channel-specific GitHub release checks. Downloading and launching always require a UI click.
use crate::release_channel::Channel;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};

pub const RELEASES: &str = "https://github.com/Puffinnois/stream-dlp/releases";
const ASSET: &str = "stream-dlp-Setup-Windows-x64.exe";
const CHECK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    url: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

#[cfg(test)]
fn parse_release(json: &str, current: &str) -> Result<Option<Update>, String> {
    parse_releases(&format!("[{json}]"), current, Channel::Public, false)
}
fn parse_releases(
    json: &str,
    current: &str,
    channel: Channel,
    switching: bool,
) -> Result<Option<Update>, String> {
    let releases: Vec<Release> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let current = semver::Version::parse(current).map_err(|e| e.to_string())?;
    // Dev follows stable releases too, so only a switch to Public can offer an older build.
    let allow_older = switching && channel == Channel::Public;
    let mut candidates = Vec::new();
    for release in releases {
        if release.draft {
            continue;
        }
        // A tag that is not a version must not block every update check.
        let Ok(version) = semver::Version::parse(release.tag_name.trim_start_matches('v')) else {
            continue;
        };
        let stable = !release.prerelease && version.pre.is_empty();
        let eligible = match channel {
            Channel::Public => stable,
            Channel::Dev => {
                stable || (release.prerelease && version.pre.as_str().starts_with("dev."))
            }
        };
        if eligible && version != current && (allow_older || version > current) {
            candidates.push((version, release));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    let Some((version, release)) = candidates.into_iter().next() else {
        return Ok(None);
    };
    let asset = release
        .assets
        .into_iter()
        .find(|a| a.name == ASSET)
        .ok_or("The new release does not have a Windows installer yet.")?;
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("The installer does not have a valid SHA-256 digest yet.")?
        .to_string();
    // Only this repository's release assets may be passed to the downloader.
    let expected = format!(
        "https://github.com/Puffinnois/stream-dlp/releases/download/{}/{ASSET}",
        release.tag_name
    );
    if asset.browser_download_url != expected {
        return Err("The installer URL is not an official release asset.".into());
    }
    Ok(Some(Update {
        version: version.to_string(),
        url: expected,
        sha256,
    }))
}

fn powershell(script: &str, vars: &[(&str, &str)]) -> Result<String, String> {
    let mut command = Command::new("powershell.exe");
    // Keep network errors readable in the UI instead of showing a PowerShell stack dump.
    let script = format!(
        "try {{\n{script}\n}} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}"
    );
    command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    command.envs(vars.iter().copied());
    let output = crate::process::isolate(&mut command)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_start_matches('\u{feff}')
        .trim()
        .to_string())
}
fn check(channel: Channel) -> Result<Option<Update>, String> {
    let json = powershell(
        r#"
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$headers = @{ 'User-Agent' = 'stream-dlp'; 'Accept' = 'application/vnd.github+json' }
if ($env:STREAM_DLP_CHANNEL -eq 'Public') {
    try {
        $release = Invoke-RestMethod 'https://api.github.com/repos/Puffinnois/stream-dlp/releases/latest' -Headers $headers -TimeoutSec 30
        ConvertTo-Json -InputObject @($release) -Depth 12 -Compress
    } catch {
        if ($_.Exception.Response.StatusCode.value__ -eq 404) { '[]' } else { throw }
    }
} else {
    $releases = @()
    for ($page = 1; $page -le 5; $page++) {
        $batch = Invoke-RestMethod "https://api.github.com/repos/Puffinnois/stream-dlp/releases?per_page=100&page=$page" -Headers $headers -TimeoutSec 30
        foreach ($release in $batch) { $releases += $release }
        if ($batch.Count -lt 100) { break }
    }
    ConvertTo-Json -InputObject @($releases) -Depth 12 -Compress
}
"#,
        &[("STREAM_DLP_CHANNEL", channel.label())],
    )?;
    parse_releases(
        &json,
        env!("CARGO_PKG_VERSION"),
        channel,
        channel != Channel::installed(),
    )
}

pub struct Installer {
    directory: tempfile::TempDir,
    path: PathBuf,
}

impl Installer {
    pub fn launch(self) -> Result<(), String> {
        Command::new(&self.path)
            .spawn()
            .map_err(|e| format!("Cannot open installer: {e}"))?;
        // The installer is a separate process and still needs this file after the app exits.
        let _ = self.directory.keep();
        Ok(())
    }
}

const UPDATE_PREFIX: &str = "stream-dlp-update-";

/// Removes the installer folders that `Installer::launch` kept for setup a day or more ago.
pub fn remove_stale_installers() {
    remove_stale_in(&std::env::temp_dir(), Duration::from_secs(24 * 60 * 60));
}

fn remove_stale_in(directory: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .file_name()
            .to_string_lossy()
            .starts_with(UPDATE_PREFIX)
            && entry.file_type().is_ok_and(|t| t.is_dir())
            && entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|elapsed| elapsed > age);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn download(update: &Update) -> Result<Installer, String> {
    let directory = tempfile::Builder::new()
        .prefix(UPDATE_PREFIX)
        .tempdir()
        .map_err(|e| e.to_string())?;
    let path = directory.path().join(ASSET);
    powershell(
        r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -Uri $env:STREAM_DLP_UPDATE_URL -OutFile $env:STREAM_DLP_UPDATE_PATH -UseBasicParsing -TimeoutSec 300
$file = [IO.File]::OpenRead($env:STREAM_DLP_UPDATE_PATH)
$sha = [Security.Cryptography.SHA256]::Create()
try { $actual = [BitConverter]::ToString($sha.ComputeHash($file)).Replace('-', '') }
finally { $file.Dispose(); $sha.Dispose() }
if ($actual -ne $env:STREAM_DLP_UPDATE_HASH) { throw 'Installer checksum verification failed. Nothing was installed.' }
"#,
        &[
            ("STREAM_DLP_UPDATE_URL", &update.url),
            ("STREAM_DLP_UPDATE_HASH", &update.sha256),
            ("STREAM_DLP_UPDATE_PATH", &path.to_string_lossy()),
        ],
    )?;
    Ok(Installer { directory, path })
}

pub enum Event {
    Checked(u64, Result<Option<Update>, String>),
    Downloaded(Result<Installer, String>),
}

pub struct Updater {
    pub channel: Channel,
    generation: u64,
    pub available: Option<Update>,
    pub message: String,
    pub checking: bool,
    pub downloading: bool,
    next_check: Instant,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
}

impl Default for Updater {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            channel: Channel::load(),
            generation: 0,
            available: None,
            message: String::new(),
            checking: false,
            downloading: false,
            next_check: Instant::now(),
            tx,
            rx,
        }
    }
}

impl Updater {
    pub fn set_channel(&mut self, channel: Channel, ctx: &eframe::egui::Context) {
        if self.downloading || self.channel == channel {
            return;
        }
        if let Err(error) = channel.save() {
            self.message = error;
            return;
        }
        self.channel = channel;
        self.generation += 1;
        self.available = None;
        self.checking = false;
        self.check(ctx);
    }
    pub fn check(&mut self, ctx: &eframe::egui::Context) {
        if self.checking || self.downloading {
            return;
        }
        self.checking = true;
        self.next_check = Instant::now() + CHECK_INTERVAL;
        self.message = format!("Checking {} releases...", self.channel.label());
        self.generation += 1;
        let (generation, channel) = (self.generation, self.channel);
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Event::Checked(generation, check(channel)));
            ctx.request_repaint();
        });
    }

    fn check_due(&self, now: Instant) -> bool {
        !self.checking && !self.downloading && now >= self.next_check
    }

    /// Wake the otherwise idle UI only when another background check is due.
    pub fn check_automatically(&mut self, ctx: &eframe::egui::Context) {
        if self.check_due(Instant::now()) {
            self.check(ctx);
        }
        if !self.checking && !self.downloading {
            ctx.request_repaint_after(self.next_check.saturating_duration_since(Instant::now()));
        }
    }

    pub fn download(&mut self, ctx: &eframe::egui::Context) {
        if self.downloading {
            return;
        }
        let Some(update) = self.available.clone() else {
            return;
        };
        self.downloading = true;
        self.message = "Downloading and verifying installer…".into();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Event::Downloaded(download(&update)));
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self) -> bool {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Checked(generation, result) => {
                    if generation != self.generation {
                        continue;
                    }
                    self.checking = false;
                    match result {
                        Ok(update) => {
                            self.message = if update.is_some() {
                                "A build is available for the selected channel."
                            } else {
                                "You’re up to date."
                            }
                            .into();
                            self.available = update;
                        }
                        Err(error) => {
                            self.message = format!("Could not check for updates: {error}")
                        }
                    }
                }
                Event::Downloaded(result) => {
                    self.downloading = false;
                    match result.and_then(Installer::launch) {
                        Ok(()) => return true,
                        Err(error) => self.message = format!("Update failed: {error}"),
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_checks_run_on_startup_and_respect_interval_and_active_work() {
        let mut updater = Updater::default();
        let now = Instant::now();
        assert!(updater.check_due(now));
        updater.next_check = now + CHECK_INTERVAL;
        assert!(!updater.check_due(now));
        assert!(!updater.check_due(updater.next_check - Duration::from_secs(1)));
        assert!(updater.check_due(updater.next_check));
        updater.checking = true;
        assert!(!updater.check_due(updater.next_check));
        updater.checking = false;
        updater.downloading = true;
        assert!(!updater.check_due(updater.next_check));
    }

    #[test]
    fn failed_background_check_keeps_available_update_and_retry_deadline() {
        let mut updater = Updater {
            available: parse_release(&release("v9.0.0").to_string(), "0.3.3").unwrap(),
            checking: true,
            next_check: Instant::now() + CHECK_INTERVAL,
            ..Default::default()
        };
        let deadline = updater.next_check;
        updater
            .tx
            .send(Event::Checked(0, Err("Offline".into())))
            .unwrap();
        assert!(!updater.poll());
        assert!(!updater.checking);
        assert!(updater.available.is_some());
        assert_eq!(updater.next_check, deadline);
        assert!(!updater.check_due(Instant::now()));
    }

    fn release(version: &str) -> serde_json::Value {
        serde_json::json!({"tag_name": version, "draft": false, "prerelease": false, "assets": [{"name": ASSET, "browser_download_url": format!("https://github.com/Puffinnois/stream-dlp/releases/download/{version}/{ASSET}"), "digest": format!("sha256:{}", "a".repeat(64))}]})
    }

    #[test]
    fn stable_versions_compare_numerically_and_never_downgrade() {
        for (tag, expected) in [
            ("v0.3.10", true),
            ("v0.3.3", false),
            ("v0.2.9", false),
            ("v0.4.0-beta.1", false),
        ] {
            assert_eq!(
                parse_release(&release(tag).to_string(), "0.3.3")
                    .unwrap()
                    .is_some(),
                expected
            );
        }
        let mut value = release("v0.4.0");
        value["prerelease"] = true.into();
        assert!(
            parse_release(&value.to_string(), "0.3.3")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn refuses_missing_installer_digest_and_foreign_urls() {
        for field in ["digest", "browser_download_url", "name"] {
            let mut value = release("v0.4.0");
            value["assets"][0][field] = "untrusted".into();
            assert!(parse_release(&value.to_string(), "0.3.3").is_err());
        }
        assert!(parse_release("{}", "0.3.3").is_err());
    }

    #[test]
    fn tags_that_are_not_versions_are_skipped() {
        assert!(
            parse_release(&release("bad-version").to_string(), "0.3.3")
                .unwrap()
                .is_none()
        );
        let releases = serde_json::json!([release("nightly"), release("v0.3.4")]).to_string();
        assert_eq!(
            parse_releases(&releases, "0.3.3", Channel::Dev, false)
                .unwrap()
                .map(|u| u.version)
                .as_deref(),
            Some("0.3.4")
        );
    }

    #[test]
    fn dev_follows_newer_stable_releases_without_offering_older_builds() {
        let mut dev = release("v0.3.5-dev.1");
        dev["prerelease"] = true.into();
        let releases = serde_json::json!([dev, release("v0.3.7")]).to_string();
        let choose = |current, channel, switching| {
            parse_releases(&releases, current, channel, switching)
                .unwrap()
                .map(|u| u.version)
        };
        assert_eq!(
            choose("0.3.5-dev.1", Channel::Dev, false).as_deref(),
            Some("0.3.7")
        );
        // Switching Public → Dev must not offer the older Dev build.
        assert!(choose("0.3.7", Channel::Dev, true).is_none());
        assert!(choose("0.3.7", Channel::Dev, false).is_none());
    }

    #[test]
    fn stale_installer_folders_are_removed_and_others_kept() {
        let temp = tempfile::tempdir().unwrap();
        let stale = temp.path().join(format!("{UPDATE_PREFIX}old"));
        let unrelated = temp.path().join("other-update-old");
        let file = temp.path().join(format!("{UPDATE_PREFIX}file"));
        std::fs::create_dir(&stale).unwrap();
        std::fs::write(stale.join(ASSET), b"setup").unwrap();
        std::fs::create_dir(&unrelated).unwrap();
        std::fs::write(&file, b"not a folder").unwrap();
        remove_stale_in(temp.path(), Duration::from_secs(3600));
        assert!(stale.exists(), "Recent installers stay for setup");
        std::thread::sleep(Duration::from_millis(50));
        remove_stale_in(temp.path(), Duration::ZERO);
        assert!(!stale.exists());
        assert!(unrelated.exists());
        assert!(file.exists());
    }

    #[test]
    fn channels_are_isolated_and_switching_can_return_to_public() {
        let mut dev = release("v0.3.5-dev.2");
        dev["prerelease"] = true.into();
        let mut previous_dev = release("v0.3.5-dev.1");
        previous_dev["prerelease"] = true.into();
        let mut beta = release("v9.0.0-beta.1");
        beta["prerelease"] = true.into();
        let mut draft = release("v10.0.0-dev.1");
        draft["prerelease"] = true.into();
        draft["draft"] = true.into();
        let releases =
            serde_json::json!([previous_dev, beta, release("v0.3.4"), dev, draft]).to_string();
        let choose = |current, channel, switching| {
            parse_releases(&releases, current, channel, switching)
                .unwrap()
                .map(|u| u.version)
        };
        assert_eq!(
            choose("0.3.3", Channel::Public, false).as_deref(),
            Some("0.3.4")
        );
        assert_eq!(
            choose("0.3.4", Channel::Dev, true).as_deref(),
            Some("0.3.5-dev.2")
        );
        assert_eq!(
            choose("0.3.5-dev.2", Channel::Public, true).as_deref(),
            Some("0.3.4")
        );
        assert!(choose("0.3.5-dev.2", Channel::Dev, false).is_none());
        assert!(choose("0.3.5-dev.3", Channel::Dev, false).is_none());
        assert!(choose("0.3.5-dev.2", Channel::Public, false).is_none());
    }

    #[test]
    fn stale_checks_cannot_offer_the_wrong_channel() {
        let mut updater = Updater {
            generation: 2,
            checking: true,
            ..Default::default()
        };
        updater
            .tx
            .send(Event::Checked(
                1,
                parse_release(&release("v9.0.0").to_string(), "0.3.3"),
            ))
            .unwrap();
        updater.poll();
        assert!(updater.available.is_none());
        assert!(updater.checking);
        updater.tx.send(Event::Checked(2, Ok(None))).unwrap();
        updater.poll();
        assert!(!updater.checking);
    }

    #[test]
    #[ignore = "requires GitHub network access; checks release metadata only"]
    fn github_channel_checks_parse_live_metadata() {
        if let Some(public) = check(Channel::Public).unwrap() {
            assert!(
                semver::Version::parse(&public.version)
                    .unwrap()
                    .pre
                    .is_empty()
            );
        }
        if let Some(dev) = check(Channel::Dev).unwrap() {
            assert!(
                semver::Version::parse(&dev.version)
                    .unwrap()
                    .pre
                    .as_str()
                    .starts_with("dev.")
            );
        }
    }

    #[test]
    fn discovering_an_update_never_starts_a_download() {
        let mut updater = Updater {
            checking: true,
            ..Default::default()
        };
        updater
            .tx
            .send(Event::Checked(
                0,
                parse_release(&release("v9.0.0").to_string(), "0.3.3"),
            ))
            .unwrap();
        assert!(!updater.poll());
        assert!(updater.available.is_some());
        assert!(!updater.checking);
        assert!(!updater.downloading);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires localhost sockets accessible to Windows PowerShell"]
    fn download_verifies_bytes_and_rejects_checksum_mismatch() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        for valid in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world").unwrap();
            });
            let update = Update {
                version: "9.0.0".into(),
                url: format!("http://{address}/installer.exe"),
                sha256: if valid {
                    "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".into()
                } else {
                    "0".repeat(64)
                },
            };
            let result = download(&update);
            if valid {
                let installer = result.unwrap_or_else(|e| panic!("{e}"));
                let path = installer.path.clone();
                assert_eq!(std::fs::read(&path).unwrap(), b"hello world");
                drop(installer);
                assert!(!path.exists());
            } else {
                assert!(
                    result
                        .err()
                        .unwrap()
                        .contains("checksum verification failed")
                );
            }
            server.join().unwrap();
        }
    }
}
