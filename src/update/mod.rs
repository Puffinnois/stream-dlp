//! App updates: a check for a newer stable release at launch and every hour. An installed
//! Windows copy downloads the slim update installer in the background, then installs it
//! silently and reopens when the user chooses Update & restart. A portable copy downloads the
//! full installer when asked and opens setup. The experimental Mac version only shows that an
//! update exists.
mod installer;
mod release;

pub use installer::remove_stale_installers;
pub use release::{RELEASES, Update};

use eframe::egui::Context;
use installer::Installer;
use std::{
    sync::{atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};

const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

enum Event {
    Checked(Result<Option<Update>, String>),
    Progress(u64, Option<u64>),
    Downloaded(Result<Installer, String>),
}

/// The installers this copy can use, the preferred first: an installed copy takes the slim
/// update (or the full installer), a portable copy the full installer, the Mac none.
fn wanted(silent: bool) -> &'static [&'static str] {
    if !cfg!(windows) {
        &[]
    } else if silent {
        &[release::UPDATE, release::SETUP]
    } else {
        &[release::SETUP]
    }
}

pub struct Updater {
    /// The newest release, while it is newer than this version.
    pub available: Option<Update>,
    pub message: String,
    pub checking: bool,
    pub downloading: bool,
    /// Bytes received and expected while downloading.
    pub progress: (u64, Option<u64>),
    ready: Option<Installer>,
    /// Installed copies update without the wizard and download as soon as an update is found.
    silent: bool,
    download_due: bool,
    next_check: Instant,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
}

impl Default for Updater {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            available: None,
            message: String::new(),
            checking: false,
            downloading: false,
            progress: (0, None),
            ready: None,
            silent: installer::installed(),
            download_due: false,
            next_check: Instant::now(),
            tx,
            rx,
        }
    }
}

impl Updater {
    /// Whether the update installs without the wizard and reopens the app.
    pub fn silent(&self) -> bool {
        self.silent
    }

    /// Whether the available update's installer is downloaded and verified.
    pub fn ready(&self) -> bool {
        self.ready.is_some()
    }

    /// Runs every frame: applies finished work, starts a due check or download, and wakes the
    /// otherwise idle UI when the next check is due.
    pub fn update(&mut self, ctx: &Context) {
        self.poll();
        if self.check_due(Instant::now()) {
            self.check(ctx);
        }
        if self.download_due {
            self.download(ctx);
        }
        if !self.checking && !self.downloading {
            ctx.request_repaint_after(self.next_check.saturating_duration_since(Instant::now()));
        }
    }

    pub fn check(&mut self, ctx: &Context) {
        if self.checking || self.downloading {
            return;
        }
        self.checking = true;
        self.next_check = Instant::now() + CHECK_INTERVAL;
        self.message = "Checking for updates…".into();
        let wanted = wanted(self.silent);
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Event::Checked(release::check(wanted)));
            ctx.request_repaint();
        });
    }

    fn check_due(&self, now: Instant) -> bool {
        !self.checking && !self.downloading && now >= self.next_check
    }

    /// Downloads the available update's installer; installed copies start this themselves.
    pub fn download(&mut self, ctx: &Context) {
        self.download_due = false;
        if self.downloading || self.ready.is_some() {
            return;
        }
        let Some(asset) = self.available.as_ref().and_then(|u| u.installer.clone()) else {
            return;
        };
        self.downloading = true;
        self.progress = (0, None);
        self.message = "Downloading the update…".into();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let progress = |received, total| {
                let _ = tx.send(Event::Progress(received, total));
                ctx.request_repaint();
            };
            let result = installer::download(&asset, &AtomicBool::new(false), progress);
            let _ = tx.send(Event::Downloaded(result));
            ctx.request_repaint();
        });
    }

    /// Starts the downloaded installer. Returns true when it started and the app should close.
    pub fn install(&mut self) -> bool {
        let Some(installer) = self.ready.take() else {
            return false;
        };
        match installer.launch(self.silent) {
            Ok(()) => true,
            Err(error) => {
                self.message = error;
                false
            }
        }
    }

    fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Checked(result) => {
                    self.checking = false;
                    match result {
                        Ok(Some(update)) => {
                            if self
                                .available
                                .as_ref()
                                .is_none_or(|known| known.version != update.version)
                            {
                                self.ready = None;
                            }
                            self.message = if self.ready.is_some() {
                                format!("Version {} is ready to install.", update.version)
                            } else {
                                format!("Version {} is available.", update.version)
                            };
                            // A failed download is retried here, at the next check.
                            self.download_due = self.silent && self.ready.is_none();
                            self.available = Some(update);
                        }
                        Ok(None) => {
                            self.message = "You’re up to date.".into();
                            self.available = None;
                            self.ready = None;
                        }
                        Err(error) => {
                            crate::dev::errors::record("update", &error);
                            self.message = format!("Could not check for updates: {error}")
                        }
                    }
                }
                Event::Progress(received, total) => self.progress = (received, total),
                Event::Downloaded(result) => {
                    self.downloading = false;
                    match result {
                        Ok(installer) => {
                            self.ready = Some(installer);
                            self.message = "The update is ready to install.".into();
                        }
                        Err(error) => {
                            crate::dev::errors::record("update", &error);
                            self.message = format!("Could not download the update: {error}")
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use release::{SETUP, UPDATE, tests::release};

    fn found(updater: &mut Updater, tag: &str) {
        let json = release(tag, &[SETUP, UPDATE]).to_string();
        updater.checking = true;
        updater
            .tx
            .send(Event::Checked(release::parse(
                &json,
                "0.3.3",
                &[UPDATE, SETUP],
            )))
            .unwrap();
        updater.poll();
    }

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
        let mut updater = Updater::default();
        found(&mut updater, "v9.0.0");
        updater.checking = true;
        updater.next_check = Instant::now() + CHECK_INTERVAL;
        let deadline = updater.next_check;
        updater
            .tx
            .send(Event::Checked(Err("Offline".into())))
            .unwrap();
        updater.poll();
        assert!(!updater.checking);
        assert!(updater.available.is_some());
        assert_eq!(updater.next_check, deadline);
        assert!(!updater.check_due(Instant::now()));
    }

    #[test]
    fn installed_copies_download_a_found_update_and_portable_copies_wait() {
        for silent in [true, false] {
            let mut updater = Updater {
                silent,
                ..Default::default()
            };
            found(&mut updater, "v9.0.0");
            assert_eq!(updater.available.as_ref().unwrap().version, "9.0.0");
            assert!(!updater.checking);
            assert_eq!(updater.download_due, silent);
            assert!(
                !updater.downloading,
                "update() starts the download, not poll()"
            );
        }
    }

    #[test]
    fn a_failed_download_is_retried_at_the_next_check() {
        let mut updater = Updater {
            silent: true,
            ..Default::default()
        };
        found(&mut updater, "v9.0.0");
        (updater.download_due, updater.downloading) = (false, true);
        updater
            .tx
            .send(Event::Downloaded(Err("Offline".into())))
            .unwrap();
        updater.poll();
        assert!(!updater.downloading && !updater.download_due && !updater.ready());
        assert!(updater.message.contains("Offline"));
        found(&mut updater, "v9.0.0");
        assert!(updater.download_due);
    }

    #[test]
    fn an_up_to_date_check_withdraws_the_update() {
        let mut updater = Updater::default();
        found(&mut updater, "v9.0.0");
        updater.checking = true;
        updater.tx.send(Event::Checked(Ok(None))).unwrap();
        updater.poll();
        assert!(updater.available.is_none());
        assert!(!updater.install(), "Nothing to install");
    }
}
