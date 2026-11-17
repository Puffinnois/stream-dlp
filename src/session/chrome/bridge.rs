//! The app's side of the extension connection: listening, the one connected browser, the
//! session it supplied, and the bounded refresh. The UI is `app::extension_panel`.
use super::{
    channel::{ConnectionId, Event, Listener},
    cookies,
    messages::{FromExtension, PROTOCOL, State, ToExtension},
};
use crate::session::SessionCookies;
use std::{
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

/// How long the app waits for the extension to answer a refresh.
pub const REFRESH_TIMEOUT: Duration = Duration::from_secs(20);

/// What the app must do after `poll`.
pub enum Update {
    /// The extension sent a session: import it, or show why it was refused.
    Imported(Result<SessionCookies, String>),
    /// The extension disconnected: remove the session it supplied.
    ClearSession,
    /// A refresh got no session: show the original sign-in error.
    RefreshFailed,
}

pub struct Bridge {
    listener: Option<Listener>,
    events_tx: mpsc::Sender<Event>,
    events_rx: mpsc::Receiver<Event>,
    connection: Option<ConnectionId>,
    greeted: bool,
    /// The connection whose session the app holds, while it holds it.
    session_connection: Option<ConnectionId>,
    refresh_since: Option<Instant>,
    /// A short line for the panel. Never contains cookie data.
    pub note: Option<String>,
}

impl Default for Bridge {
    fn default() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            listener: None,
            events_tx,
            events_rx,
            connection: None,
            greeted: false,
            session_connection: None,
            refresh_since: None,
            note: None,
        }
    }
}

impl Bridge {
    pub fn listening(&self) -> bool {
        self.listener.is_some()
    }

    /// Whether a browser is connected and has said hello.
    pub fn connected(&self) -> bool {
        self.connection.is_some() && self.greeted
    }

    /// Whether the app's YouTube session came from the extension.
    pub fn has_session(&self) -> bool {
        self.session_connection.is_some()
    }

    #[cfg(test)]
    pub fn refreshing(&self) -> bool {
        self.refresh_since.is_some()
    }

    /// Starts listening for the host. `wake` repaints the window when something arrives.
    pub fn start(
        &mut self,
        folder: &Path,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<(), String> {
        if self.listener.is_none() {
            self.listener = Some(
                Listener::start(folder, self.events_tx.clone(), wake)
                    .map_err(|_| "Could not start listening for the browser extension.")?,
            );
        }
        self.note = Some("Waiting for the extension. Press Connect in its popup.".into());
        Ok(())
    }

    /// Tells the connected extension, stops listening and forgets the session's origin.
    /// Returns whether the app holds a session from the extension, which it must remove.
    pub fn stop(&mut self) -> bool {
        if let (Some(listener), Some(id)) = (&self.listener, self.connection) {
            listener.send(id, &ToExtension::Disconnect);
            listener.close(id);
        }
        self.listener = None;
        self.connection = None;
        self.greeted = false;
        self.refresh_since = None;
        self.note = None;
        self.session_connection.take().is_some()
    }

    /// The app's YouTube session was removed or replaced by another import.
    pub fn forget_session(&mut self) {
        self.session_connection = None;
        self.refresh_since = None;
    }

    /// Whether the connection that supplied the current session is still open.
    pub fn can_refresh(&self) -> bool {
        self.listener.is_some()
            && self.greeted
            && self.connection.is_some()
            && self.connection == self.session_connection
            && self.refresh_since.is_none()
    }

    /// Asks that same connection for its session once more.
    pub fn request_refresh(&mut self, now: Instant) -> bool {
        if !self.can_refresh() {
            return false;
        }
        let (Some(listener), Some(id)) = (&self.listener, self.connection) else {
            return false;
        };
        if !listener.send(id, &ToExtension::Refresh) {
            return false;
        }
        self.refresh_since = Some(now);
        self.note = Some("Asking the extension for a fresh session…".into());
        true
    }

    /// Handles everything that arrived. `unix_now` filters expired cookies.
    pub fn poll(&mut self, unix_now: u64, now: Instant) -> Vec<Update> {
        let mut updates = Vec::new();
        while let Ok(event) = self.events_rx.try_recv() {
            updates.extend(self.handle(event, unix_now));
        }
        if self
            .refresh_since
            .is_some_and(|since| now.duration_since(since) > REFRESH_TIMEOUT)
        {
            self.refresh_since = None;
            self.note = Some("The extension did not send a fresh session.".into());
            updates.push(Update::RefreshFailed);
        }
        updates
    }

    fn reply(&self, id: ConnectionId, state: State) {
        if let Some(listener) = &self.listener {
            listener.send(id, &ToExtension::Status { state });
        }
    }

    fn refuse(&mut self, id: ConnectionId, note: &str) {
        self.reply(id, State::Rejected);
        if let Some(listener) = &self.listener {
            listener.close(id);
        }
        self.note = Some(note.into());
    }

    fn handle(&mut self, event: Event, unix_now: u64) -> Option<Update> {
        match event {
            Event::Connected(id) => {
                self.connection = Some(id);
                self.greeted = false;
                self.note = Some("The extension connected.".into());
                None
            }
            Event::Closed(id) => {
                if self.connection != Some(id) {
                    return None;
                }
                self.connection = None;
                self.greeted = false;
                if self.listener.is_some() {
                    self.note = Some(if self.session_connection == Some(id) {
                        "The browser closed the connection. The imported session stays until you remove it, but it cannot be refreshed.".into()
                    } else {
                        "The browser closed the connection.".into()
                    });
                }
                self.refresh_since.take().map(|_| Update::RefreshFailed)
            }
            Event::Invalid(id) if self.connection == Some(id) => {
                self.note = Some(
                    "The extension sent an invalid message; the connection was closed.".into(),
                );
                None
            }
            Event::Message(id, message) if self.connection == Some(id) => match message {
                FromExtension::Hello { protocol } if protocol == PROTOCOL && !self.greeted => {
                    self.greeted = true;
                    self.reply(id, State::Connected);
                    None
                }
                FromExtension::Hello { .. } => {
                    self.refuse(id, "The extension's version does not match this app. Reload the extension from this app's folder.");
                    None
                }
                FromExtension::Session { .. } if !self.greeted => {
                    self.refuse(id, "The extension sent a session before saying hello; the connection was closed.");
                    None
                }
                FromExtension::Session { cookies } => {
                    let result = cookies::to_session(&cookies, unix_now);
                    self.refresh_since = None;
                    match &result {
                        Ok(_) => {
                            self.session_connection = Some(id);
                            self.reply(id, State::Imported);
                            self.note = Some("Session imported from the browser extension.".into());
                        }
                        Err(_) => {
                            self.reply(id, State::Rejected);
                            self.note = None;
                        }
                    }
                    Some(Update::Imported(result))
                }
                FromExtension::Disconnect => {
                    if let Some(listener) = &self.listener {
                        listener.close(id);
                    }
                    self.connection = None;
                    self.greeted = false;
                    self.refresh_since = None;
                    self.note = Some("The extension disconnected.".into());
                    self.session_connection.take().map(|_| Update::ClearSession)
                }
            },
            // From a connection that is no longer the current one.
            Event::Invalid(_) | Event::Message(..) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::chrome::{channel, framing};
    use std::net::TcpStream;

    const NOW: u64 = 1_800_000_000;
    const SESSION: &str = r#"{"type":"session","cookies":[{"domain":".youtube.com","hostOnly":false,"path":"/","secure":true,"httpOnly":true,"expirationDate":4102444800,"name":"LOGIN_INFO","value":"fake"}]}"#;

    struct Rig {
        bridge: Bridge,
        _folder: tempfile::TempDir,
        host: TcpStream,
    }

    impl Rig {
        fn new() -> Self {
            let folder = tempfile::tempdir().unwrap();
            let mut bridge = Bridge::default();
            bridge.start(folder.path(), Arc::new(|| {})).unwrap();
            let host = channel::connect(folder.path()).unwrap();
            let mut rig = Self {
                bridge,
                _folder: folder,
                host,
            };
            assert!(rig.next().is_none());
            rig
        }

        /// Handles the next event, waiting for it.
        fn next(&mut self) -> Option<Update> {
            let event = self
                .bridge
                .events_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("an event");
            self.bridge.handle(event, NOW)
        }

        fn send(&mut self, json: &str) {
            std::io::Write::write_all(&mut self.host, &framing::frame(json.as_bytes())).unwrap();
        }

        fn receive(&mut self) -> ToExtension {
            framing::read_message(&mut self.host).unwrap()
        }

        fn greet(&mut self) {
            self.send(r#"{"type":"hello","protocol":1}"#);
            assert!(self.next().is_none());
            assert_eq!(
                self.receive(),
                ToExtension::Status {
                    state: State::Connected
                }
            );
        }

        fn import(&mut self) {
            self.send(SESSION);
            assert!(matches!(self.next(), Some(Update::Imported(Ok(_)))));
            assert_eq!(
                self.receive(),
                ToExtension::Status {
                    state: State::Imported
                }
            );
        }
    }

    #[test]
    fn connect_import_and_one_refresh_from_the_same_connection() {
        let mut rig = Rig::new();
        assert!(!rig.bridge.can_refresh(), "No session yet");
        rig.greet();
        rig.import();
        assert!(rig.bridge.has_session());
        let start = Instant::now();
        assert!(rig.bridge.request_refresh(start));
        assert_eq!(rig.receive(), ToExtension::Refresh);
        assert!(!rig.bridge.request_refresh(start), "One refresh at a time");
        rig.import();
        assert!(!rig.bridge.refreshing());
        assert!(rig.bridge.request_refresh(start));
        assert_eq!(rig.receive(), ToExtension::Refresh);
        let updates = rig
            .bridge
            .poll(NOW, start + REFRESH_TIMEOUT + Duration::from_secs(1));
        assert!(matches!(updates[..], [Update::RefreshFailed]));
        assert!(!rig.bridge.refreshing());
    }

    #[test]
    fn a_session_before_hello_or_a_wrong_version_is_refused() {
        let mut rig = Rig::new();
        rig.send(SESSION);
        assert!(rig.next().is_none());
        assert_eq!(
            rig.receive(),
            ToExtension::Status {
                state: State::Rejected
            }
        );
        assert!(rig.next().is_none());
        assert!(!rig.bridge.has_session());
        assert!(rig.bridge.connection.is_none());

        let mut rig = Rig::new();
        rig.send(r#"{"type":"hello","protocol":2}"#);
        assert!(rig.next().is_none());
        assert_eq!(
            rig.receive(),
            ToExtension::Status {
                state: State::Rejected
            }
        );
    }

    #[test]
    fn a_bad_session_is_reported_and_keeps_the_connection() {
        let mut rig = Rig::new();
        rig.greet();
        rig.send(r#"{"type":"session","cookies":[{"domain":".example.com","hostOnly":false,"path":"/","secure":true,"httpOnly":false,"name":"OTHER","value":"PRIVATE"}]}"#);
        let Some(Update::Imported(Err(error))) = rig.next() else {
            panic!("not refused")
        };
        assert!(!error.contains("PRIVATE"));
        assert_eq!(
            rig.receive(),
            ToExtension::Status {
                state: State::Rejected
            }
        );
        assert!(rig.bridge.connected());
        assert!(!rig.bridge.has_session());
    }

    #[test]
    fn disconnect_from_the_extension_clears_its_session() {
        let mut rig = Rig::new();
        rig.greet();
        rig.import();
        rig.send(r#"{"type":"disconnect"}"#);
        assert!(matches!(rig.next(), Some(Update::ClearSession)));
        assert!(!rig.bridge.has_session());
        assert!(!rig.bridge.can_refresh());
        assert!(
            rig.bridge.listening(),
            "The app keeps listening for a new connection"
        );
    }

    #[test]
    fn a_closed_browser_keeps_the_session_but_cannot_refresh() {
        let mut rig = Rig::new();
        rig.greet();
        rig.import();
        let start = Instant::now();
        assert!(rig.bridge.request_refresh(start));
        let _ = rig.receive();
        rig.host.shutdown(std::net::Shutdown::Both).unwrap();
        assert!(matches!(rig.next(), Some(Update::RefreshFailed)));
        assert!(rig.bridge.has_session());
        assert!(!rig.bridge.can_refresh());
        assert!(!rig.bridge.request_refresh(start));
    }

    #[test]
    fn stop_disconnects_the_extension_and_reports_the_session_to_remove() {
        let mut rig = Rig::new();
        rig.greet();
        rig.import();
        assert!(rig.bridge.stop());
        assert_eq!(rig.receive(), ToExtension::Disconnect);
        assert!(!rig.bridge.listening());
        assert!(!rig.bridge.has_session());
        assert!(!rig.bridge.stop());
        assert!(channel::connect(rig._folder.path()).is_err());
    }

    #[test]
    fn a_forgotten_session_is_never_refreshed() {
        let mut rig = Rig::new();
        rig.greet();
        rig.import();
        rig.bridge.forget_session();
        assert!(!rig.bridge.request_refresh(Instant::now()));
    }

    /// The app's side of the manual acceptance checklist without the window: registers the
    /// built app for this Windows user (the real keys, removed again at the end), listens in
    /// the real `native-host` folder and prints what a browser's extension does for
    /// `STREAM_DLP_EXTENSION_LIVE_SECONDS` seconds. After an import it asks for one refresh.
    /// Prints states and messages only, never cookie data. Do not run it while a stream-dlp
    /// on this machine is connected to the extension: both use the same folder and keys.
    #[cfg(windows)]
    #[test]
    #[ignore = "writes this user's native messaging registration; needs STREAM_DLP_EXTENSION_LIVE_SECONDS and a browser with the extension loaded"]
    fn live_browser_extension_session() {
        use crate::session::chrome::{folder, registry};
        let seconds: u64 = std::env::var("STREAM_DLP_EXTENSION_LIVE_SECONDS")
            .expect("Set STREAM_DLP_EXTENSION_LIVE_SECONDS explicitly")
            .parse()
            .unwrap();
        let exe = std::env::var_os("STREAM_DLP_HOST_EXE").map_or_else(
            || {
                let test = std::env::current_exe().unwrap();
                test.parent()
                    .and_then(|deps| deps.parent())
                    .unwrap()
                    .join("stream-dlp.exe")
            },
            std::path::PathBuf::from,
        );
        assert!(
            exe.is_file(),
            "Build the app first: cargo build --locked --bin stream-dlp"
        );
        let folder = folder().unwrap();
        registry::register(&mut registry::UserRegistry, &folder, &exe).unwrap();
        let mut bridge = Bridge::default();
        bridge.start(&folder, Arc::new(|| {})).unwrap();
        println!("registered and listening");
        let start = Instant::now();
        let (mut note, mut connected, mut refresh_at) = (None, false, None);
        while start.elapsed() < Duration::from_secs(seconds) {
            let unix_now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            for update in bridge.poll(unix_now, Instant::now()) {
                let at = start.elapsed().as_secs();
                match update {
                    Update::Imported(Ok(_)) => {
                        println!("[{at}s] session imported");
                        // The extension answers a refresh at most once every 10 seconds.
                        refresh_at.get_or_insert(Instant::now() + Duration::from_secs(12));
                    }
                    Update::Imported(Err(error)) => println!("[{at}s] session refused: {error}"),
                    Update::ClearSession => println!("[{at}s] session cleared"),
                    Update::RefreshFailed => println!("[{at}s] refresh failed"),
                }
            }
            if refresh_at.is_some_and(|at| Instant::now() > at) {
                refresh_at = Some(Instant::now() + Duration::from_secs(3600));
                println!(
                    "refresh requested: {}",
                    bridge.request_refresh(Instant::now())
                );
            }
            if bridge.note != note || bridge.connected() != connected {
                (note, connected) = (bridge.note.clone(), bridge.connected());
                let at = start.elapsed().as_secs();
                println!("[{at}s] connected={connected} note={note:?}");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        println!("stopping; held a session: {}", bridge.stop());
        registry::unregister(&mut registry::UserRegistry, &folder).unwrap();
        assert!(registry::registered(&registry::UserRegistry, &folder).is_empty());
    }
}
