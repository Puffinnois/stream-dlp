//! The experimental Mac release uses manual updates; no PowerShell or installer.
use crate::release_channel::Channel;

pub const RELEASES: &str = "https://github.com/Puffinnois/stream-dlp/releases";

pub struct Update {
    pub version: String,
}

pub struct Updater {
    pub channel: Channel,
    pub available: Option<Update>,
    pub message: String,
    pub checking: bool,
    pub downloading: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            channel: Channel::load(),
            available: None,
            message: "Mac updates are manual. Download the latest Mac package from Releases."
                .into(),
            checking: false,
            downloading: false,
        }
    }
}

impl Updater {
    pub fn set_channel(&mut self, channel: Channel, _ctx: &eframe::egui::Context) {
        if let Err(error) = channel.save() {
            self.message = error;
            return;
        }
        self.channel = channel;
        self.message = format!(
            "Mac updates are manual. Choose a {} Mac package from Releases.",
            channel.label()
        );
    }

    pub fn check(&mut self, _ctx: &eframe::egui::Context) {}

    pub fn check_automatically(&mut self, _ctx: &eframe::egui::Context) {}

    pub fn download(&mut self, _ctx: &eframe::egui::Context) {
        self.message = "Use the Releases link to download the latest Mac package.".into();
    }

    pub fn poll(&mut self) -> bool {
        false
    }
}
