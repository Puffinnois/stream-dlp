//! Experimental Chrome and Edge session import through the companion extension in `extension/`.
//!
//! The extension reads YouTube cookies with the browser's cookies API and sends them over
//! native messaging to this executable started in host mode (`host`). The host relays them to
//! the running app over an authenticated loopback channel (`channel`); the app checks every
//! cookie again (`cookies`) and keeps the session in memory (`bridge`). `registry` registers
//! the host with Chrome and Edge for the current user, only when the person presses Connect.
//! Shown only in Dev mode. See `docs/reference/chrome-extension.md`.
pub mod bridge;
pub mod channel;
pub mod cookies;
pub mod framing;
pub mod host;
pub mod messages;
pub mod registry;

use std::path::PathBuf;

/// The host's name in the browsers' registry keys and in `connectNative` (`extension/bridge.js`).
pub const HOST_NAME: &str = "com.stream_dlp.youtube_session";

/// The extension's ID, fixed by the public `key` in `extension/manifest.json` (a test checks it).
pub const EXTENSION_ID: &str = "edngekgkkjhaoohkfddejaoohfaggiac";

/// The only caller the host and the registered manifest accept.
pub fn origin() -> String {
    format!("chrome-extension://{EXTENSION_ID}/")
}

/// Overrides the folder below; only the host-mode end-to-end test sets it.
pub const FOLDER_VARIABLE: &str = "STREAM_DLP_NATIVE_HOST_DIR";

/// Where the host manifest and the channel file live: `%LOCALAPPDATA%\stream-dlp\native-host`.
pub fn folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os(FOLDER_VARIABLE).filter(|v| !v.is_empty()) {
        return Some(folder.into());
    }
    crate::dev::data_dir().map(|dir| dir.join("native-host"))
}

/// `--unregister-extension`, run by the Windows uninstaller before it deletes the app: removes
/// this executable's registration (`registry::unregister_copy`) and returns the exit code. Opens
/// no window and writes no log; nothing registered is success.
pub fn unregister_this_copy() -> i32 {
    let result = folder()
        .ok_or_else(|| "The app data folder is unknown.".to_owned())
        .and_then(|folder| {
            let exe = std::env::current_exe().map_err(|_| "The app's location is unknown.")?;
            registry::unregister_copy(&mut registry::UserRegistry, &folder, &exe)
        });
    match result {
        Ok(_) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    const MANIFEST: &str = include_str!("../../../extension/manifest.json");
    const BRIDGE: &str = include_str!("../../../extension/bridge.js");

    fn base64(text: &str) -> Vec<u8> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bits = 0u32;
        let mut count = 0;
        let mut out = Vec::new();
        for byte in text.bytes().filter(|&b| b != b'=') {
            let value = ALPHABET.iter().position(|&a| a == byte).expect("base64") as u32;
            bits = bits << 6 | value;
            count += 6;
            if count >= 8 {
                count -= 8;
                out.push((bits >> count) as u8);
            }
        }
        out
    }

    /// Chrome derives an unpacked extension's ID from its `key`: the first 16 bytes of the
    /// SHA-256 of the public key, each hex digit mapped to a letter from a to p.
    #[test]
    fn the_extension_id_follows_from_the_committed_public_key() {
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        let key = base64(manifest["key"].as_str().unwrap());
        let digest = Sha256::digest(&key);
        let id: String = digest[..16]
            .iter()
            .flat_map(|b| [b >> 4, b & 15])
            .map(|n| (b'a' + n) as char)
            .collect();
        assert_eq!(id, EXTENSION_ID);
        assert_eq!(origin(), format!("chrome-extension://{id}/"));
        assert!(!MANIFEST.contains("PRIVATE KEY"));
    }

    #[test]
    fn the_extension_asks_only_for_cookies_messaging_and_youtube() {
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest["manifest_version"], 3);
        assert_eq!(
            manifest["permissions"],
            serde_json::json!(["cookies", "nativeMessaging"])
        );
        // `*.youtube.com` covers youtube.com and its subdomains: the YouTube session policy.
        assert_eq!(
            manifest["host_permissions"],
            serde_json::json!(["*://*.youtube.com/*"])
        );
        for absent in [
            "optional_permissions",
            "optional_host_permissions",
            "content_scripts",
            "externally_connectable",
            "web_accessible_resources",
            "content_security_policy",
            "update_url",
        ] {
            assert!(manifest.get(absent).is_none(), "{absent}");
        }
        assert!(BRIDGE.contains(&format!("HOST = \"{HOST_NAME}\"")));
        assert!(BRIDGE.contains(&format!("PROTOCOL = {}", messages::PROTOCOL)));
    }
}
