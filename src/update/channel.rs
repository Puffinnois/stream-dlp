//! Update preference shared by installed and portable builds. No credentials are stored.
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Channel {
    Public,
    Dev,
}

impl Channel {
    pub fn installed() -> Self {
        if env!("CARGO_PKG_VERSION").contains("-dev.") {
            Self::Dev
        } else {
            Self::Public
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Public => "Public",
            Self::Dev => "Dev",
        }
    }

    pub fn load() -> Self {
        settings_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(Self::installed)
    }

    pub fn save(self) -> Result<(), String> {
        let path = settings_path().ok_or("Cannot locate your app settings folder.")?;
        save_to(self, &path).map_err(|e| format!("Could not save update channel: {e}"))
    }
}

fn settings_path() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|p| p.join("stream-dlp").join("update-channel.json"))
}

fn save_to(channel: Channel, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path.parent().ok_or("Missing settings directory")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, &channel)?;
    file.flush()?;
    file.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_can_be_changed_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings/update-channel.json");
        for channel in [Channel::Dev, Channel::Public, Channel::Dev] {
            save_to(channel, &path).unwrap();
            let actual: Channel = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(actual, channel);
        }
        assert!(serde_json::from_str::<Channel>("\"unknown\"").is_err());
    }
}
