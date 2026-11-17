//! Turns the extension's cookies into a YouTube session, checking every one again: the
//! extension is outside the app's trust boundary. The domain policy is the one every other
//! import uses (`SessionCookies::parse_for` with `Source::YouTube`).
use super::messages::ChromeCookie;
use crate::{engine::Source, session::SessionCookies};

/// More cookies than a YouTube session ever has; a larger list is refused whole.
pub const MAX_COOKIES: usize = 2000;
/// Longer than any real cookie name, value, domain or path (browsers cap a cookie at 4 KB).
const MAX_FIELD: usize = 8192;

const INVALID: &str =
    "The browser extension sent an invalid YouTube session. Press Connect in the extension again.";

/// The YouTube session in `cookies` at `now` (seconds since the Unix epoch). Expired cookies,
/// cookies without a name and cookies of other domains are dropped; any malformed field
/// refuses the whole session. Errors never contain cookie names or values.
pub fn to_session(cookies: &[ChromeCookie], now: u64) -> Result<SessionCookies, String> {
    if cookies.len() > MAX_COOKIES {
        return Err(INVALID.into());
    }
    let mut text = String::from("# Netscape HTTP Cookie File\r\n");
    for cookie in cookies {
        let fields = [&cookie.domain, &cookie.path, &cookie.name, &cookie.value];
        if fields
            .iter()
            .any(|field| field.len() > MAX_FIELD || field.chars().any(char::is_control))
        {
            return Err(INVALID.into());
        }
        if cookie.name.is_empty() {
            continue;
        }
        let expiry = match cookie.expiration_date {
            None => 0,
            Some(date) if !date.is_finite() => return Err(INVALID.into()),
            // A browser removes expired cookies, but the list may be old by now.
            Some(date) if date <= now as f64 => continue,
            // Saturates beyond `u64::MAX`; the floor keeps a cookie valid through its last second.
            Some(date) => date.floor() as u64,
        };
        let bare = cookie
            .domain
            .strip_prefix('.')
            .unwrap_or(&cookie.domain)
            .to_ascii_lowercase();
        // A host-only cookie is sent to that host alone; a domain cookie to its subdomains too.
        let (domain, subdomains) = if cookie.host_only {
            (bare, "FALSE")
        } else {
            (format!(".{bare}"), "TRUE")
        };
        text.push_str(&format!(
            "{}{domain}\t{subdomains}\t{}\t{}\t{expiry}\t{}\t{}\r\n",
            if cookie.http_only { "#HttpOnly_" } else { "" },
            cookie.path,
            if cookie.secure { "TRUE" } else { "FALSE" },
            cookie.name,
            cookie.value,
        ));
    }
    const SIGNED_OUT: &str = "No usable YouTube session was found. Sign in to YouTube in this browser profile, then press Connect in the extension again.";
    match SessionCookies::parse_for(&text, Source::YouTube) {
        // A signed-out profile has YouTube cookies too; they are not a session.
        Ok(session) if session.has_youtube_sign_in() => Ok(session),
        Ok(_) => Err(SIGNED_OUT.into()),
        Err(error) if error.starts_with("No ") => Err(SIGNED_OUT.into()),
        Err(_) => Err(INVALID.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn cookie(domain: &str, name: &str) -> ChromeCookie {
        ChromeCookie {
            domain: domain.into(),
            host_only: !domain.starts_with('.'),
            path: "/".into(),
            secure: true,
            http_only: false,
            expiration_date: Some(4_102_444_800.0),
            name: name.into(),
            value: "fake-test-value".into(),
        }
    }

    fn text(session: &SessionCookies) -> String {
        let file = session.temporary_file().unwrap();
        std::fs::read_to_string(file.path()).unwrap()
    }

    #[test]
    fn keeps_youtube_cookies_and_drops_other_domains_and_lookalikes() {
        let mut list = vec![
            cookie(".youtube.com", "LOGIN_INFO"),
            cookie("www.youtube.com", "GOOD_HOST"),
            cookie(".M.YouTube.com", "GOOD_UPPER"),
        ];
        for domain in [
            ".example.com",
            ".notyoutube.com",
            "youtube.com.evil.test",
            ".youtube.co",
            ".google.com",
            ".youtu.be",
            "youtube.com.",
        ] {
            list.push(cookie(domain, "FOREIGN"));
        }
        list[1].http_only = true;
        list[1].expiration_date = None;
        let text = text(&to_session(&list, NOW).unwrap());
        assert!(text.contains(".youtube.com\tTRUE\t/\tTRUE\t4102444800\tLOGIN_INFO\t"));
        assert!(text.contains("#HttpOnly_www.youtube.com\tFALSE\t/\tTRUE\t0\tGOOD_HOST\t"));
        assert!(text.contains(".m.youtube.com\tTRUE\t"));
        assert!(!text.contains("FOREIGN"));
    }

    #[test]
    fn expired_and_nameless_cookies_are_dropped() {
        let mut expired = cookie(".youtube.com", "EXPIRED");
        expired.expiration_date = Some(NOW as f64);
        let mut fraction = cookie(".youtube.com", "LAST_SECOND");
        fraction.expiration_date = Some(NOW as f64 + 0.5);
        let nameless = cookie(".youtube.com", "");
        let text = text(
            &to_session(
                &[
                    expired,
                    fraction,
                    nameless,
                    cookie(".youtube.com", "LOGIN_INFO"),
                ],
                NOW,
            )
            .unwrap(),
        );
        assert!(!text.contains("EXPIRED"));
        assert!(text.contains(&format!("\t{NOW}\tLAST_SECOND\t")));
        assert_eq!(text.lines().count(), 3);
    }

    #[test]
    fn hostile_fields_refuse_the_whole_session_without_echoing_values() {
        let cases: [fn(&mut ChromeCookie); 10] = [
            |c| c.value = "PRIVATE\n.youtube.com\tTRUE\t/\tTRUE\t0\tINJECTED\tx".into(),
            |c| c.value = "PRIVATE\tsplit".into(),
            |c| c.name = "PRIVATE\r".into(),
            |c| c.path = "/\u{0}".into(),
            |c| c.domain = ".youtube.com\n".into(),
            |c| c.value = "PRIVATE".repeat(2000),
            |c| c.path = "PRIVATE-relative".into(),
            |c| c.expiration_date = Some(f64::NAN),
            |c| c.expiration_date = Some(f64::INFINITY),
            |c| c.domain = "..youtube.com".into(),
        ];
        for change in cases {
            let mut bad = cookie(".youtube.com", "PRIVATE_NAME");
            change(&mut bad);
            let error = match to_session(&[cookie(".youtube.com", "LOGIN_INFO"), bad], NOW) {
                Ok(_) => panic!("A hostile cookie was accepted"),
                Err(error) => error,
            };
            assert!(!error.contains("PRIVATE"), "{error}");
        }
    }

    #[test]
    fn too_many_cookies_or_none_usable_are_refused() {
        let many = vec![cookie(".youtube.com", "SID"); MAX_COOKIES + 1];
        assert!(to_session(&many, NOW).is_err());
        assert!(to_session(&many[..MAX_COOKIES], NOW).is_ok());
        let error = to_session(&[cookie(".example.com", "OTHER")], NOW)
            .err()
            .unwrap();
        assert!(error.contains("Sign in to YouTube"));
        assert!(to_session(&[], NOW).is_err());
    }

    #[test]
    fn a_signed_out_profile_is_not_a_session() {
        // What a browser holds for youtube.com before anyone signs in.
        let signed_out = [
            cookie(".youtube.com", "VISITOR_INFO1_LIVE"),
            cookie(".youtube.com", "YSC"),
            cookie(".youtube.com", "PREF"),
        ];
        let error = to_session(&signed_out, NOW).err().unwrap();
        assert!(error.contains("Sign in to YouTube"));
        let mut signed_in = signed_out.to_vec();
        signed_in.push(cookie(".youtube.com", "__Secure-3PSID"));
        assert!(to_session(&signed_in, NOW).is_ok());
    }
}
