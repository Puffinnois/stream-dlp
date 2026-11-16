//! The messages the extension and the app exchange through the host. Both directions are
//! checked against these shapes; anything else is refused.
use serde::{Deserialize, Serialize};

/// The protocol version in `hello`; a different one is refused.
pub const PROTOCOL: u32 = 1;

/// From the extension (through the host) to the app.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FromExtension {
    Hello { protocol: u32 },
    Session { cookies: Vec<ChromeCookie> },
    Disconnect,
}

impl std::fmt::Debug for FromExtension {
    // Cookie values must never reach a log or a panic message.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hello { protocol } => write!(f, "Hello {{ protocol: {protocol} }}"),
            Self::Session { cookies } => write!(f, "Session {{ {} cookies }}", cookies.len()),
            Self::Disconnect => write!(f, "Disconnect"),
        }
    }
}

/// One cookie as the extension sends it: the fields of the browser's `cookies.Cookie` the
/// app needs, nothing else. `expirationDate` is absent for a session cookie.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChromeCookie {
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<f64>,
    pub name: String,
    pub value: String,
}

/// From the app (or the host itself) to the extension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToExtension {
    Status {
        state: State,
    },
    /// Send the session again: YouTube asked to sign in. At most once per inspection.
    Refresh,
    /// The app disconnected; close the port.
    Disconnect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The app accepted the connection; send the session.
    Connected,
    /// The app imported the session.
    Imported,
    /// The app refused the session or the message.
    Rejected,
    /// Another browser or profile is connected; disconnect it first.
    Busy,
    /// stream-dlp is not running or not listening (press Connect in the app).
    AppUnavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_have_exactly_the_documented_shapes() {
        let hello: FromExtension =
            serde_json::from_str(r#"{"type":"hello","protocol":1}"#).unwrap();
        assert_eq!(hello, FromExtension::Hello { protocol: 1 });
        let session: FromExtension = serde_json::from_str(
            r#"{"type":"session","cookies":[{"domain":".youtube.com","hostOnly":false,"path":"/","secure":true,"httpOnly":true,"expirationDate":4102444800.5,"name":"TEST","value":"fake"}]}"#,
        )
        .unwrap();
        let FromExtension::Session { cookies } = &session else {
            panic!("not a session")
        };
        assert_eq!(cookies[0].expiration_date, Some(4102444800.5));
        assert_eq!(format!("{session:?}"), "Session { 1 cookies }");
        assert_eq!(
            serde_json::to_string(&ToExtension::Status {
                state: State::AppUnavailable
            })
            .unwrap(),
            r#"{"type":"status","state":"app_unavailable"}"#
        );
        assert_eq!(
            serde_json::to_string(&ToExtension::Refresh).unwrap(),
            r#"{"type":"refresh"}"#
        );
    }

    #[test]
    fn unknown_types_fields_and_wrong_types_are_refused() {
        for text in [
            r#"{"type":"eval","code":"x"}"#,
            r#"{"type":"hello","protocol":1,"extra":true}"#,
            r#"{"type":"hello","protocol":"1"}"#,
            r#"{"protocol":1}"#,
            r#"{"type":"session","cookies":[{"domain":".youtube.com","hostOnly":false,"path":"/","secure":true,"httpOnly":true,"name":"A","value":"b","storeId":"0"}]}"#,
            r#"{"type":"session","cookies":[{"domain":".youtube.com"}]}"#,
            r#"{"type":"session","cookies":{}}"#,
            r#"[]"#,
        ] {
            assert!(
                serde_json::from_str::<FromExtension>(text).is_err(),
                "{text}"
            );
        }
        assert!(
            serde_json::from_str::<ToExtension>(r#"{"type":"status","state":"owned"}"#).is_err()
        );
    }
}
