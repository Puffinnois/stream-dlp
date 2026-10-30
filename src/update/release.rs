//! The newest stable stream-dlp release on GitHub, and the installer this copy would use.
use serde::Deserialize;

pub const RELEASES: &str = "https://github.com/Puffinnois/stream-dlp/releases";
/// The full installer, with the tools. Installed versions up to 0.3.9 accept only this asset.
pub const SETUP: &str = "stream-dlp-Setup-Windows-x64.exe";
/// The app without the tools; it only updates an existing installation.
pub const UPDATE: &str = "stream-dlp-Update-Windows-x64.exe";

#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    /// The release notes, in Markdown.
    pub notes: String,
    pub page: String,
    /// None on the Mac, which updates by hand.
    pub installer: Option<Asset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: &'static str,
    pub url: String,
    pub sha256: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

/// `wanted` lists the installers this copy can use, the preferred one first; none means the
/// update is only announced.
pub fn parse(json: &str, current: &str, wanted: &[&'static str]) -> Result<Option<Update>, String> {
    let release: Release = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let current = semver::Version::parse(current).map_err(|e| e.to_string())?;
    // A tag that is not a version must not break every update check.
    let Ok(version) = semver::Version::parse(release.tag_name.trim_start_matches('v')) else {
        return Ok(None);
    };
    if release.draft || release.prerelease || !version.pre.is_empty() || version <= current {
        return Ok(None);
    }
    let installer = if wanted.is_empty() {
        None
    } else {
        let (name, asset) = wanted
            .iter()
            .find_map(|&name| {
                release
                    .assets
                    .iter()
                    .find(|a| a.name == name)
                    .map(|a| (name, a))
            })
            .ok_or("The new release does not have a Windows installer yet.")?;
        let sha256 = asset
            .digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
            .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("The installer does not have a valid SHA-256 digest yet.")?
            .to_string();
        // Only this repository's release assets may be downloaded.
        let url = format!(
            "https://github.com/Puffinnois/stream-dlp/releases/download/{}/{name}",
            release.tag_name
        );
        if asset.browser_download_url != url {
            return Err("The installer URL is not an official release asset.".into());
        }
        Some(Asset { name, url, sha256 })
    };
    Ok(Some(Update {
        version: version.to_string(),
        notes: release.body.unwrap_or_default().trim().to_string(),
        page: format!("{RELEASES}/tag/{}", release.tag_name),
        installer,
    }))
}

/// The latest release, when it is newer than this version. GitHub's "latest" is the newest
/// published release that is not a draft or a prerelease.
pub fn check(wanted: &[&'static str]) -> Result<Option<Update>, String> {
    match crate::net::github("repos/Puffinnois/stream-dlp/releases/latest")? {
        Some(json) => parse(&json, env!("CARGO_PKG_VERSION"), wanted),
        None => Ok(None),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn release(tag: &str, assets: &[&str]) -> serde_json::Value {
        let assets: Vec<_> = assets
            .iter()
            .map(|name| serde_json::json!({"name": name, "browser_download_url": format!("https://github.com/Puffinnois/stream-dlp/releases/download/{tag}/{name}"), "digest": format!("sha256:{}", "a".repeat(64))}))
            .collect();
        serde_json::json!({"tag_name": tag, "body": "## What's Changed\n* fix: things\n", "draft": false, "prerelease": false, "assets": assets})
    }

    fn version(value: &serde_json::Value) -> Option<String> {
        parse(&value.to_string(), "0.3.3", &[SETUP])
            .unwrap()
            .map(|u| u.version)
    }

    #[test]
    fn only_newer_stable_versions_are_offered() {
        for (tag, offered) in [
            ("v0.3.10", true),
            ("v0.3.3", false),
            ("v0.2.9", false),
            ("v0.4.0-beta.1", false),
            ("nightly", false),
        ] {
            assert_eq!(version(&release(tag, &[SETUP])).is_some(), offered, "{tag}");
        }
        for flag in ["prerelease", "draft"] {
            let mut value = release("v0.4.0", &[SETUP]);
            value[flag] = true.into();
            assert!(version(&value).is_none(), "{flag}");
        }
    }

    #[test]
    fn an_update_carries_its_notes_and_release_page() {
        let update = parse(&release("v0.4.0", &[SETUP]).to_string(), "0.3.9", &[])
            .unwrap()
            .unwrap();
        assert_eq!(update.notes, "## What's Changed\n* fix: things");
        assert_eq!(update.page, format!("{RELEASES}/tag/v0.4.0"));
        assert!(update.installer.is_none(), "The Mac only announces updates");
        let mut value = release("v0.4.0", &[]);
        value["body"] = serde_json::Value::Null;
        let update = parse(&value.to_string(), "0.3.9", &[]).unwrap().unwrap();
        assert_eq!(update.notes, "");
    }

    #[test]
    fn installed_copies_prefer_the_slim_installer() {
        let choose = |assets: &[&str], wanted: &[&'static str]| {
            parse(&release("v0.4.0", assets).to_string(), "0.3.9", wanted)
                .map(|u| u.unwrap().installer.unwrap().name)
        };
        assert_eq!(choose(&[SETUP, UPDATE], &[UPDATE, SETUP]), Ok(UPDATE));
        assert_eq!(choose(&[SETUP], &[UPDATE, SETUP]), Ok(SETUP));
        assert_eq!(choose(&[SETUP, UPDATE], &[SETUP]), Ok(SETUP));
        assert!(choose(&[UPDATE], &[SETUP]).is_err());
    }

    #[test]
    fn refuses_missing_installer_digest_and_foreign_urls() {
        for field in ["digest", "browser_download_url", "name"] {
            let mut value = release("v0.4.0", &[SETUP]);
            value["assets"][0][field] = "untrusted".into();
            assert!(
                parse(&value.to_string(), "0.3.3", &[SETUP]).is_err(),
                "{field}"
            );
        }
        assert!(parse("{}", "0.3.3", &[SETUP]).is_err());
    }

    #[test]
    #[ignore = "requires GitHub network access; checks release metadata only"]
    fn github_check_parses_live_metadata() {
        // An old version, so the latest release is always offered.
        let json = crate::net::github("repos/Puffinnois/stream-dlp/releases/latest")
            .unwrap()
            .unwrap();
        let update = parse(&json, "0.0.1", &[SETUP]).unwrap().unwrap();
        assert!(
            semver::Version::parse(&update.version)
                .unwrap()
                .pre
                .is_empty()
        );
    }
}
