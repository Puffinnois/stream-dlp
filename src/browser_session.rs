//! Native, offline Firefox import. No browser process, network, or broad export.
use crate::session::SessionCookies;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub path: PathBuf,
    pub default: bool,
}

pub fn profiles() -> Vec<Profile> {
    let Some(roaming) = dirs::config_dir() else {
        return Vec::new();
    };
    let root = roaming.join("Mozilla/Firefox");
    let Ok(text) = std::fs::read_to_string(root.join("profiles.ini")) else {
        return Vec::new();
    };
    parse_profiles(&root, &text)
}

fn parse_profiles(root: &Path, text: &str) -> Vec<Profile> {
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            sections.push((name.to_owned(), Vec::new()));
        } else if let Some((key, value)) = line.split_once('=')
            && let Some((_, fields)) = sections.last_mut()
        {
            fields.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    let field = |fields: &[(String, String)], key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let install_default = sections
        .iter()
        .filter(|(name, _)| name.starts_with("Install"))
        .map(|(_, fields)| field(fields, "Default"))
        .find(|v| !v.is_empty());
    let mut result = Vec::new();
    for (section, fields) in &sections {
        if !section.starts_with("Profile") {
            continue;
        }
        let raw = field(fields, "Path");
        if raw.is_empty() {
            continue;
        }
        let relative = field(fields, "IsRelative") == "1";
        let path = if relative {
            root.join(&raw)
        } else {
            PathBuf::from(&raw)
        };
        if !path.join("cookies.sqlite").is_file() {
            continue;
        }
        let name = field(fields, "Name");
        let default = if let Some(default) = &install_default {
            default == &raw
        } else {
            field(fields, "Default") == "1"
        };
        if !result.iter().any(|p: &Profile| p.path == path) {
            result.push(Profile {
                name: if name.is_empty() {
                    "Firefox profile".into()
                } else {
                    name
                },
                path,
                default,
            });
        }
    }
    result.sort_by_key(|p| !p.default);
    result
}

pub fn import(profile: &Profile, cancel: &AtomicBool) -> Result<SessionCookies, String> {
    #[cfg(windows)]
    {
        native::import(&profile.path.join("cookies.sqlite"), cancel)
    }
    #[cfg(not(windows))]
    {
        let _ = (profile, cancel);
        Err(
            "Automatic Firefox import is available on Windows. Use a cookies.txt export instead."
                .into(),
        )
    }
}

struct Budget<'a> {
    cancel: &'a AtomicBool,
    start: Instant,
}
impl Budget<'_> {
    fn check(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            Err("Browser import cancelled.".into())
        } else if self.start.elapsed() > Duration::from_secs(15) {
            Err(
                "Browser import timed out. Close Firefox and retry, or use a cookies.txt export."
                    .into(),
            )
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        ffi::{CStr, CString, c_char, c_int, c_void},
        ptr,
    };

    // Windows 10/11 provide SQLite through the system component winsqlite3.
    // Handles stay on this worker thread. Statements borrow their connection.
    #[link(name = "winsqlite3")]
    unsafe extern "C" {
        fn sqlite3_open_v2(
            path: *const c_char,
            db: *mut *mut c_void,
            flags: c_int,
            vfs: *const c_char,
        ) -> c_int;
        fn sqlite3_close(db: *mut c_void) -> c_int;
        fn sqlite3_busy_timeout(db: *mut c_void, ms: c_int) -> c_int;
        fn sqlite3_progress_handler(
            db: *mut c_void,
            n: c_int,
            callback: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
            arg: *mut c_void,
        );
        fn sqlite3_prepare_v2(
            db: *mut c_void,
            sql: *const c_char,
            len: c_int,
            stmt: *mut *mut c_void,
            tail: *mut *const c_char,
        ) -> c_int;
        fn sqlite3_step(stmt: *mut c_void) -> c_int;
        fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
        fn sqlite3_column_text(stmt: *mut c_void, column: c_int) -> *const u8;
        fn sqlite3_column_bytes(stmt: *mut c_void, column: c_int) -> c_int;
        fn sqlite3_column_int64(stmt: *mut c_void, column: c_int) -> i64;
    }
    const ERROR: &str = "Could not read Firefox cookies. Close Firefox and retry. If it still fails, use a cookies.txt export.";
    struct Database(*mut c_void);
    impl Drop for Database {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    sqlite3_close(self.0);
                }
            }
        }
    }
    struct Statement<'a> {
        raw: *mut c_void,
        _db: &'a Database,
    }
    impl Drop for Statement<'_> {
        fn drop(&mut self) {
            unsafe {
                sqlite3_finalize(self.raw);
            }
        }
    }
    impl Database {
        fn open(path: &Path, flags: c_int) -> Result<Self, String> {
            let path = path.to_str().ok_or(ERROR)?;
            let path = CString::new(path).map_err(|_| ERROR)?;
            let mut raw = ptr::null_mut();
            // SAFETY: valid NUL-terminated path and out pointer; connection owns raw.
            let code = unsafe { sqlite3_open_v2(path.as_ptr(), &mut raw, flags, ptr::null()) };
            let db = Self(raw);
            if code != 0 {
                return Err(ERROR.into());
            }
            unsafe {
                sqlite3_busy_timeout(db.0, 100);
            }
            Ok(db)
        }
        fn prepare(&self, sql: &CStr) -> Result<Statement<'_>, String> {
            let mut raw = ptr::null_mut();
            // SAFETY: self is live, SQL is static NUL-terminated, out pointer valid.
            let code =
                unsafe { sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut raw, ptr::null_mut()) };
            let stmt = Statement { raw, _db: self };
            if code != 0 {
                Err(ERROR.into())
            } else {
                Ok(stmt)
            }
        }
    }
    impl Statement<'_> {
        fn next(&self) -> Result<bool, String> {
            match unsafe { sqlite3_step(self.raw) } {
                100 => Ok(true),
                101 => Ok(false),
                _ => Err(ERROR.into()),
            }
        }
        fn text(&self, column: c_int) -> Result<String, String> {
            // SAFETY: called only for a current row. Copy bytes before stepping again.
            let value = unsafe { sqlite3_column_text(self.raw, column) };
            let length = unsafe { sqlite3_column_bytes(self.raw, column) };
            if !(0..=65_536).contains(&length) {
                return Err(ERROR.into());
            }
            if value.is_null() {
                return Err(ERROR.into());
            }
            let bytes = unsafe { std::slice::from_raw_parts(value, length as usize) };
            String::from_utf8(bytes.to_vec()).map_err(|_| ERROR.into())
        }
        fn integer(&self, column: c_int) -> i64 {
            unsafe { sqlite3_column_int64(self.raw, column) }
        }
    }
    unsafe extern "C" fn progress(arg: *mut c_void) -> c_int {
        // SAFETY: Budget outlives db and every query; callback is synchronous.
        let budget = unsafe { &*arg.cast::<Budget<'_>>() };
        c_int::from(budget.check().is_err())
    }

    pub(super) fn import(path: &Path, cancel: &AtomicBool) -> Result<SessionCookies, String> {
        let mut budget = Budget {
            cancel,
            start: Instant::now(),
        };
        budget.check()?;
        if !path.is_file() {
            return Err("This Firefox profile is no longer available. Refresh the profile list or choose a cookies.txt export.".into());
        }
        // SQLITE_OPEN_READONLY: never copy/export the full database or write cookies.
        let db = Database::open(path, 1)?;
        unsafe {
            sqlite3_progress_handler(
                db.0,
                1000,
                Some(progress),
                (&mut budget as *mut Budget<'_>).cast(),
            );
        }
        let result = read(&db, &budget);
        // Remove the callback before Budget goes out of scope, including on error.
        unsafe {
            sqlite3_progress_handler(db.0, 0, None, ptr::null_mut());
        }
        budget.check()?;
        result
    }

    fn read(db: &Database, budget: &Budget<'_>) -> Result<SessionCookies, String> {
        let schema = db.prepare(c"PRAGMA user_version")?;
        if !schema.next()? {
            return Err(ERROR.into());
        }
        let version = schema.integer(0);
        if !(1..=17).contains(&version) {
            return Err("This Firefox cookie database version is not supported yet. Use a cookies.txt export.".into());
        }
        drop(schema);
        // Only the normal, unpartitioned context. Never combine container sessions.
        let rows = db.prepare(c"SELECT host, name, value, path, expiry, isSecure, isHttpOnly FROM moz_cookies WHERE (lower(host) = 'youtube.com' OR lower(host) GLOB '*.youtube.com') AND originAttributes = ''")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ERROR)?
            .as_secs();
        let mut text = String::from("# Netscape HTTP Cookie File\r\n");
        let mut count = 0;
        while rows.next()? {
            budget.check()?;
            count += 1;
            if count > 2000 || text.len() > 2_000_000 {
                return Err("Too many YouTube cookies. Use a cookies.txt export instead.".into());
            }
            let expiry = rows.integer(4);
            if expiry < 0 {
                continue;
            }
            let raw_expiry = expiry;
            let expiry = if version >= 16 {
                expiry as u64 / 1000
            } else {
                expiry as u64
            };
            if raw_expiry != 0 && expiry <= now {
                continue;
            }
            let domain = rows.text(0)?.to_ascii_lowercase();
            let name = rows.text(1)?;
            let value = rows.text(2)?;
            let path = rows.text(3)?;
            // Prevent a value/path from injecting another Netscape cookie record.
            if [&domain, &name, &value, &path]
                .iter()
                .any(|v| v.chars().any(char::is_control))
            {
                return Err(
                    "A Firefox cookie could not be imported. Use a fresh cookies.txt export."
                        .into(),
                );
            }
            text.push_str(&format!(
                "{}{}\t{}\t{}\t{}\t{}\t{}\t{}\r\n",
                if rows.integer(6) != 0 {
                    "#HttpOnly_"
                } else {
                    ""
                },
                domain,
                if domain.starts_with('.') {
                    "TRUE"
                } else {
                    "FALSE"
                },
                path,
                if rows.integer(5) != 0 {
                    "TRUE"
                } else {
                    "FALSE"
                },
                expiry,
                name,
                value
            ));
        }
        SessionCookies::parse(&text).map_err(|_| "No usable YouTube session was found. Sign in to YouTube in this Firefox profile and retry. For private windows or containers, use a cookies.txt export.".into())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        fn fixture(path: &Path, version: u32) -> Database {
            let db = Database::open(path, 6).unwrap();
            for sql in [format!("PRAGMA user_version={version}"), "CREATE TABLE moz_cookies (host TEXT, name TEXT, value TEXT, path TEXT, expiry INTEGER, isSecure INTEGER, isHttpOnly INTEGER, originAttributes TEXT)".into()] {
                db.prepare(&CString::new(sql).unwrap()).unwrap().next().unwrap();
            }
            db
        }
        fn execute(db: &Database, sql: &str) {
            db.prepare(&CString::new(sql).unwrap())
                .unwrap()
                .next()
                .unwrap();
        }
        #[test]
        fn imports_scoped_current_cookies_readonly_including_wal_and_expiry_units() {
            for version in [15, 16, 17] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("cookies.sqlite");
                let writer = fixture(&path, version);
                execute(&writer, "PRAGMA journal_mode=WAL");
                let future = if version >= 16 {
                    4102444800000_u64
                } else {
                    4102444800
                };
                execute(
                    &writer,
                    &format!(
                        "INSERT INTO moz_cookies VALUES ('.youtube.com','GOOD','fake','/',{future},1,1,'')"
                    ),
                );
                for (host, name, attrs, expiry) in [
                    (".example.com", "OTHER", "", future),
                    (".youtube.com.evil.test", "EVIL", "", future),
                    (".youtube.com", "CONTAINER", "^userContextId=1", future),
                    (".youtube.com", "EXPIRED", "", 1),
                ] {
                    execute(
                        &writer,
                        &format!(
                            "INSERT INTO moz_cookies VALUES ('{host}','{name}','fake','/',{expiry},1,0,'{attrs}')"
                        ),
                    );
                }
                let before = std::fs::read(&path).unwrap();
                let session = import(&path, &AtomicBool::new(false)).unwrap();
                let file = session.temporary_file().unwrap();
                let text = std::fs::read_to_string(file.path()).unwrap();
                assert!(text.contains("#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t4102444800\tGOOD"));
                for forbidden in ["OTHER", "EVIL", "CONTAINER", "EXPIRED"] {
                    assert!(!text.contains(forbidden));
                }
                assert_eq!(std::fs::read(&path).unwrap(), before);
                // The same connection-opening wrapper rejects writes in production mode.
                let reader = Database::open(&path, 1).unwrap();
                assert!(
                    reader
                        .prepare(c"DELETE FROM moz_cookies")
                        .unwrap()
                        .next()
                        .is_err()
                );
            }
        }
        #[test]
        fn empty_missing_unknown_and_cancelled_imports_fail_without_values() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("cookies.sqlite");
            assert!(import(&path, &AtomicBool::new(false)).is_err());
            let db = fixture(&path, 17);
            assert!(import(&path, &AtomicBool::new(false)).is_err());
            execute(
                &db,
                "INSERT INTO moz_cookies VALUES ('.youtube.com','SECRET','PRIVATE_VALUE','/',4102444800000,1,1,'')",
            );
            assert!(import(&path, &AtomicBool::new(true)).is_err());
            execute(&db, "PRAGMA user_version=99");
            let error = import(&path, &AtomicBool::new(false)).err().unwrap();
            assert!(!error.contains("PRIVATE_VALUE"));
        }

        #[test]
        fn lock_invalid_values_and_deadline_fail_without_partial_session() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("cookies.sqlite");
            let db = fixture(&path, 17);
            execute(
                &db,
                "INSERT INTO moz_cookies VALUES ('.youtube.com','GOOD','fake','/',4102444800000,1,1,'')",
            );
            execute(&db, "BEGIN EXCLUSIVE");
            assert!(import(&path, &AtomicBool::new(false)).is_err());
            execute(&db, "ROLLBACK");
            execute(
                &db,
                "INSERT INTO moz_cookies VALUES ('.youtube.com','BAD','private' || char(10) || 'injection','/',4102444800000,1,1,'')",
            );
            let error = import(&path, &AtomicBool::new(false)).err().unwrap();
            assert!(!error.contains("private"));
            let cancel = AtomicBool::new(false);
            let budget = Budget {
                cancel: &cancel,
                start: Instant::now() - Duration::from_secs(16),
            };
            assert!(budget.check().unwrap_err().contains("timed out"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_follow_install_default_and_skip_missing_and_background_profiles() {
        let dir = tempfile::tempdir().unwrap();
        for path in ["Profiles/one", "Profiles/two"] {
            std::fs::create_dir_all(dir.path().join(path)).unwrap();
            std::fs::write(dir.path().join(path).join("cookies.sqlite"), []).unwrap();
        }
        let profiles = parse_profiles(
            dir.path(),
            "[InstallABC]\nDefault=Profiles/two\n[Profile0]\nName=one\nIsRelative=1\nPath=Profiles/one\nDefault=1\n[Profile1]\nName=two\nIsRelative=1\nPath=Profiles/two\n[Profile2]\nPath=missing\nIsRelative=1\n[BackgroundTasksProfiles]\nPath=Profiles/one\n",
        );
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].name, "two");
        assert!(profiles[0].default);
        assert!(!profiles[1].default);
    }

    #[test]
    #[ignore = "explicit real-profile access; set STREAM_DLP_FIREFOX_PROFILE to the authorized profile directory"]
    fn authorized_firefox_profile_import() {
        let path = std::env::var_os("STREAM_DLP_FIREFOX_PROFILE").expect(
            "Set STREAM_DLP_FIREFOX_PROFILE explicitly; no default profile is read by tests",
        );
        let profile = Profile {
            name: "Explicit test profile".into(),
            path: path.into(),
            default: false,
        };
        let session =
            import(&profile, &AtomicBool::new(false)).unwrap_or_else(|error| panic!("{error}"));
        let temporary = session.temporary_file().unwrap();
        let path = temporary.path().to_owned();
        assert!(path.is_file());
        drop(temporary);
        assert!(!path.exists());
        // Deliberately do not print cookie names, values, or account details.
    }
}
