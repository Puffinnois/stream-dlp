//! The experimental Mac release uses manual updates; no PowerShell or installer.
pub const RELEASES: &str = "https://github.com/Puffinnois/stream-dlp/releases";

pub struct Update {
    pub version: String,
}

pub struct Updater {
    pub available: Option<Update>,
    pub message: String,
    pub checking: bool,
    pub downloading: bool,
    pub prompt: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            available: None,
            message: "Mac updates are manual. Download the latest Mac package from Releases."
                .into(),
            checking: false,
            downloading: false,
            prompt: false,
        }
    }
}

impl Updater {
    pub fn check(&mut self, _ctx: &eframe::egui::Context) {
        self.prompt = true;
    }

    pub fn download(&mut self, _ctx: &eframe::egui::Context) {
        self.message = "Use the Releases link to download the latest Mac package.".into();
    }

    pub fn poll(&mut self) -> bool {
        false
    }
}
