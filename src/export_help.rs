use std::path::{Path, PathBuf};

pub const EXPORT_NAME: &str = "youtube-cookies.txt";
pub const CHROME_EXPORTER: &str = "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc";
pub const FIREFOX_EXPORTER: &str = "https://addons.mozilla.org/en-US/firefox/addon/cookies-txt/";

pub fn downloads_folder() -> PathBuf {
    dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
        .unwrap_or_else(std::env::temp_dir)
}

pub fn export_folder() -> PathBuf {
    downloads_folder().join("stream-dlp")
}

// Start the picker in the most specific existing folder, even before the user
// has created the suggested export subfolder or if their last folder was removed.
pub fn picker_folder(preferred: &Path) -> PathBuf {
    preferred
        .ancestors()
        .find(|path| path.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(downloads_folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_picker_uses_existing_export_folder_or_its_nearest_parent() {
        let folder = tempfile::tempdir().unwrap();
        let missing = folder.path().join("stream-dlp").join("Exports");
        assert_eq!(picker_folder(&missing), folder.path());
        std::fs::create_dir_all(&missing).unwrap();
        assert_eq!(picker_folder(&missing), missing);
    }
}
