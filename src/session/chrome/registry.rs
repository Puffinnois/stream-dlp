//! Registers this executable as the extension's native messaging host for the current user,
//! only when the person presses Connect, and removes the registration on Disconnect.
//!
//! Chrome reads `HKCU\Software\Google\Chrome\NativeMessagingHosts\<name>`; Edge reads its own
//! key first and falls back to Chrome's. Both point at one manifest in the per-user folder
//! (`super::folder`) that allows exactly our extension's origin. The uninstaller does not know
//! about either (an installer change); see `docs/reference/chrome-extension.md`.
use super::{HOST_NAME, origin};
use std::{
    io,
    path::{Path, PathBuf},
};

/// The browsers the host is registered with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Edge,
}

impl Browser {
    pub const ALL: [Self; 2] = [Self::Chrome, Self::Edge];

    /// The key under `HKEY_CURRENT_USER` whose default value names the manifest.
    pub fn key(self) -> String {
        let vendor = match self {
            Self::Chrome => r"Google\Chrome",
            Self::Edge => r"Microsoft\Edge",
        };
        format!(r"Software\{vendor}\NativeMessagingHosts\{HOST_NAME}")
    }
}

/// The default values of keys under `HKEY_CURRENT_USER`. Tests use a fake.
pub trait Registry {
    fn read(&self, key: &str) -> io::Result<Option<String>>;
    fn write(&mut self, key: &str, value: &str) -> io::Result<()>;
    /// Deletes the key; a missing key is not an error.
    fn delete(&mut self, key: &str) -> io::Result<()>;
}

pub fn manifest_path(folder: &Path) -> PathBuf {
    folder.join(format!("{HOST_NAME}.json"))
}

/// The host manifest: this executable, standard I/O, and only our extension's origin.
pub fn manifest(exe: &Path) -> serde_json::Value {
    serde_json::json!({
        "name": HOST_NAME,
        "description": "stream-dlp YouTube session import (experimental)",
        "path": exe,
        "type": "stdio",
        "allowed_origins": [origin()],
    })
}

/// Writes the manifest and points both browsers' keys at it.
pub fn register(registry: &mut dyn Registry, folder: &Path, exe: &Path) -> Result<(), String> {
    if !exe.is_absolute() {
        return Err("The app's location is unknown, so it cannot be registered.".into());
    }
    let path = manifest_path(folder);
    let text = serde_json::to_vec_pretty(&manifest(exe))
        .map_err(|_| "Could not prepare the registration.")?;
    let staged = path.with_extension("json.tmp");
    std::fs::create_dir_all(folder)
        .and_then(|_| std::fs::write(&staged, text))
        .and_then(|_| std::fs::rename(&staged, &path))
        .map_err(|_| "Could not write the browser registration file.".to_owned())?;
    let value = path.to_string_lossy();
    for browser in Browser::ALL {
        if registry.write(&browser.key(), &value).is_err() {
            let _ = unregister(registry, folder);
            return Err("Could not register stream-dlp with Chrome and Edge.".into());
        }
    }
    Ok(())
}

/// Removes both keys and the manifest. Safe to repeat.
pub fn unregister(registry: &mut dyn Registry, folder: &Path) -> Result<(), String> {
    let mut failed = false;
    for browser in Browser::ALL {
        failed |= registry.delete(&browser.key()).is_err();
    }
    match std::fs::remove_file(manifest_path(folder)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => failed = true,
        _ => {}
    }
    if failed {
        Err("Could not remove the browser registration completely. Try Disconnect again.".into())
    } else {
        Ok(())
    }
}

/// The browsers whose key points at our manifest, while that manifest exists.
pub fn registered(registry: &dyn Registry, folder: &Path) -> Vec<Browser> {
    let path = manifest_path(folder);
    if !path.is_file() {
        return Vec::new();
    }
    let value = path.to_string_lossy();
    Browser::ALL
        .into_iter()
        .filter(|browser| {
            registry
                .read(&browser.key())
                .is_ok_and(|v| v.as_deref() == Some(&*value))
        })
        .collect()
}

/// The real `HKEY_CURRENT_USER`. Only on Windows; elsewhere every call fails.
pub struct UserRegistry;

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;

    type Hkey = *mut c_void;
    // A predefined handle: the 32-bit value 0x80000001, sign-extended like the SDK's macro.
    const HKEY_CURRENT_USER: Hkey = 0x8000_0001_u32 as i32 as isize as Hkey;
    const KEY_SET_VALUE: u32 = 0x0002;
    const REG_SZ: u32 = 1;
    const RRF_RT_REG_SZ: u32 = 0x0002;
    const ERROR_FILE_NOT_FOUND: i32 = 2;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegCreateKeyExW(
            key: Hkey,
            subkey: *const u16,
            reserved: u32,
            class: *const u16,
            options: u32,
            access: u32,
            security: *const c_void,
            result: *mut Hkey,
            disposition: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            key: Hkey,
            name: *const u16,
            reserved: u32,
            kind: u32,
            data: *const u8,
            length: u32,
        ) -> i32;
        fn RegCloseKey(key: Hkey) -> i32;
        fn RegDeleteKeyW(key: Hkey, subkey: *const u16) -> i32;
        fn RegGetValueW(
            key: Hkey,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            length: *mut u32,
        ) -> i32;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    fn check(code: i32) -> io::Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code))
        }
    }

    impl Registry for UserRegistry {
        fn read(&self, key: &str) -> io::Result<Option<String>> {
            let subkey = wide(key);
            let mut buffer = vec![0u16; 1024];
            let mut length = (buffer.len() * 2) as u32;
            // SAFETY: valid NUL-terminated key, buffer and length; the default value has no name.
            let code = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    subkey.as_ptr(),
                    std::ptr::null(),
                    RRF_RT_REG_SZ,
                    std::ptr::null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &mut length,
                )
            };
            if code == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            check(code)?;
            let units = (length as usize / 2).min(buffer.len());
            let text = &buffer[..units];
            let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
            Ok(Some(String::from_utf16_lossy(&text[..end])))
        }

        fn write(&mut self, key: &str, value: &str) -> io::Result<()> {
            let subkey = wide(key);
            let data = wide(value);
            let mut handle: Hkey = std::ptr::null_mut();
            // SAFETY: valid NUL-terminated key and out pointer; the handle is closed below.
            check(unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    subkey.as_ptr(),
                    0,
                    std::ptr::null(),
                    0,
                    KEY_SET_VALUE,
                    std::ptr::null(),
                    &mut handle,
                    std::ptr::null_mut(),
                )
            })?;
            // SAFETY: an open key; the data includes its NUL terminator as REG_SZ requires.
            let result = check(unsafe {
                RegSetValueExW(
                    handle,
                    std::ptr::null(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            });
            unsafe {
                RegCloseKey(handle);
            }
            result
        }

        fn delete(&mut self, key: &str) -> io::Result<()> {
            let subkey = wide(key);
            // SAFETY: valid NUL-terminated key. Our keys have no subkeys.
            match unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, subkey.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => Ok(()),
                code => check(code),
            }
        }
    }
}

#[cfg(not(windows))]
impl Registry for UserRegistry {
    fn read(&self, _: &str) -> io::Result<Option<String>> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn write(&mut self, _: &str, _: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn delete(&mut self, _: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// An in-memory registry; tests never touch the real `HKEY_CURRENT_USER`.
    #[derive(Default)]
    pub(crate) struct FakeRegistry {
        pub values: BTreeMap<String, String>,
        pub fail_writes: bool,
    }

    impl Registry for FakeRegistry {
        fn read(&self, key: &str) -> io::Result<Option<String>> {
            Ok(self.values.get(key).cloned())
        }
        fn write(&mut self, key: &str, value: &str) -> io::Result<()> {
            if self.fail_writes {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            self.values.insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&mut self, key: &str) -> io::Result<()> {
            self.values.remove(key);
            Ok(())
        }
    }

    fn exe() -> PathBuf {
        if cfg!(windows) {
            r"C:\Apps\stream-dlp\stream-dlp.exe".into()
        } else {
            "/Applications/stream-dlp.app/Contents/MacOS/stream-dlp".into()
        }
    }

    #[test]
    fn registration_writes_both_keys_and_a_manifest_for_our_extension_only() {
        let folder = tempfile::tempdir().unwrap();
        let mut registry = FakeRegistry::default();
        register(&mut registry, folder.path(), &exe()).unwrap();
        let manifest_file = manifest_path(folder.path());
        assert_eq!(
            registry.values.keys().collect::<Vec<_>>(),
            [
                r"Software\Google\Chrome\NativeMessagingHosts\com.stream_dlp.youtube_session",
                r"Software\Microsoft\Edge\NativeMessagingHosts\com.stream_dlp.youtube_session",
            ]
        );
        for value in registry.values.values() {
            assert_eq!(value, &manifest_file.to_string_lossy());
        }
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_file).unwrap()).unwrap();
        assert_eq!(written["name"], HOST_NAME);
        assert_eq!(written["type"], "stdio");
        assert_eq!(written["path"], exe().to_string_lossy().as_ref());
        assert_eq!(
            written["allowed_origins"],
            serde_json::json!(["chrome-extension://edngekgkkjhaoohkfddejaoohfaggiac/"])
        );
        assert_eq!(written.as_object().unwrap().len(), 5);
        assert_eq!(
            registered(&registry, folder.path()),
            [Browser::Chrome, Browser::Edge]
        );
    }

    #[test]
    fn unregister_removes_everything_and_can_repeat() {
        let folder = tempfile::tempdir().unwrap();
        let mut registry = FakeRegistry::default();
        register(&mut registry, folder.path(), &exe()).unwrap();
        unregister(&mut registry, folder.path()).unwrap();
        assert!(registry.values.is_empty());
        assert!(!manifest_path(folder.path()).exists());
        assert!(registered(&registry, folder.path()).is_empty());
        unregister(&mut registry, folder.path()).unwrap();
    }

    #[test]
    fn a_failed_or_relative_registration_leaves_nothing_behind() {
        let folder = tempfile::tempdir().unwrap();
        let mut registry = FakeRegistry {
            fail_writes: true,
            ..Default::default()
        };
        assert!(register(&mut registry, folder.path(), &exe()).is_err());
        assert!(registry.values.is_empty());
        assert!(!manifest_path(folder.path()).exists());
        let mut registry = FakeRegistry::default();
        assert!(register(&mut registry, folder.path(), Path::new("stream-dlp.exe")).is_err());
        assert!(registry.values.is_empty());
    }

    #[test]
    fn a_key_pointing_elsewhere_or_a_missing_manifest_is_not_registered() {
        let folder = tempfile::tempdir().unwrap();
        let mut registry = FakeRegistry::default();
        register(&mut registry, folder.path(), &exe()).unwrap();
        registry
            .values
            .insert(Browser::Edge.key(), r"C:\elsewhere\host.json".into());
        assert_eq!(registered(&registry, folder.path()), [Browser::Chrome]);
        std::fs::remove_file(manifest_path(folder.path())).unwrap();
        assert!(registered(&registry, folder.path()).is_empty());
    }
}
