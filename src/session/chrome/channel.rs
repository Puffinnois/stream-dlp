//! The channel between the host (started by the browser) and the running app.
//!
//! The app listens on a random loopback port and writes the port and a fresh 32-byte secret to
//! `channel.json` in the per-user folder (`super::folder`, inside `%LOCALAPPDATA%`, which only
//! the user, SYSTEM and administrators can read). Each side then proves it knows the secret
//! with HMAC-SHA256 over two random nonces before anything else is sent, so neither another
//! local user's process nor a stale port reused by someone else can send or receive a session.
//! The secret never crosses the socket. Frames are the native messaging ones (`framing`).
use super::framing::{self, FrameError};
use super::messages::{FromExtension, ToExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

pub const CHANNEL_FILE: &str = "channel.json";
const HELLO: &str = "stream-dlp-native-host";
const VERSION: u32 = 1;
/// How long each side waits for the other during the handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

type Secret = [u8; 32];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelFile {
    port: u16,
    secret: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    hello: String,
    version: u32,
    nonce: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Challenge {
    nonce: String,
    proof: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    proof: String,
}

/// Cryptographically secure random bytes from the operating system.
pub fn random_bytes<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                algorithm: *mut std::ffi::c_void,
                buffer: *mut u8,
                length: u32,
                flags: u32,
            ) -> i32;
        }
        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;
        // SAFETY: the buffer is valid for `N` bytes; no algorithm handle is needed with this flag.
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                bytes.as_mut_ptr(),
                N as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status != 0 {
            return Err(io::Error::other("no system randomness"));
        }
    }
    #[cfg(not(windows))]
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// HMAC-SHA256 (RFC 2104) of the concatenated `parts`, for keys of at most one block.
fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    assert!(key.len() <= 64);
    let mut block = [0u8; 64];
    block[..key.len()].copy_from_slice(key);
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// Compares without stopping at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn proof(secret: &Secret, role: &[u8], host_nonce: &[u8], app_nonce: &[u8]) -> [u8; 32] {
    hmac(secret, &[role, b"|", host_nonce, b"|", app_nonce])
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "channel not authenticated")
}

fn read<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> io::Result<T> {
    framing::read_message(stream).map_err(|error| match error {
        FrameError::Io(kind) => io::Error::from(kind),
        _ => invalid(),
    })
}

/// The app's side of the handshake.
fn accept_handshake(stream: &mut TcpStream, secret: &Secret) -> io::Result<()> {
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let hello: Hello = read(stream)?;
    let host_nonce = unhex::<32>(&hello.nonce).ok_or_else(invalid)?;
    if hello.hello != HELLO || hello.version != VERSION {
        return Err(invalid());
    }
    let app_nonce = random_bytes::<32>()?;
    framing::write_message(
        stream,
        &Challenge {
            nonce: hex(&app_nonce),
            proof: hex(&proof(secret, b"app", &host_nonce, &app_nonce)),
        },
    )?;
    let answer: Proof = read(stream)?;
    let expected = proof(secret, b"host", &host_nonce, &app_nonce);
    if !unhex::<32>(&answer.proof).is_some_and(|given| same(&given, &expected)) {
        return Err(invalid());
    }
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)
}

/// The host's side of the handshake.
fn connect_handshake(stream: &mut TcpStream, secret: &Secret) -> io::Result<()> {
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let host_nonce = random_bytes::<32>()?;
    framing::write_message(
        stream,
        &Hello {
            hello: HELLO.into(),
            version: VERSION,
            nonce: hex(&host_nonce),
        },
    )?;
    let challenge: Challenge = read(stream)?;
    let app_nonce = unhex::<32>(&challenge.nonce).ok_or_else(invalid)?;
    let expected = proof(secret, b"app", &host_nonce, &app_nonce);
    // The app proves itself first: the host sends nothing more to a listener that cannot.
    if !unhex::<32>(&challenge.proof).is_some_and(|given| same(&given, &expected)) {
        return Err(invalid());
    }
    framing::write_message(
        stream,
        &Proof {
            proof: hex(&proof(secret, b"host", &host_nonce, &app_nonce)),
        },
    )?;
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)
}

/// Connects the host to the app listening in `folder`, authenticated both ways.
pub fn connect(folder: &Path) -> io::Result<TcpStream> {
    let mut bytes = Vec::new();
    std::fs::File::open(folder.join(CHANNEL_FILE))?
        .take(4096)
        .read_to_end(&mut bytes)?;
    let file: ChannelFile = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let secret = unhex::<32>(&file.secret).ok_or_else(invalid)?;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, file.port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    stream.set_nodelay(true)?;
    connect_handshake(&mut stream, &secret)?;
    Ok(stream)
}

/// Identifies one authenticated host connection for the app's lifetime.
pub type ConnectionId = u64;

/// What the listener reports to the app.
#[derive(Debug)]
pub enum Event {
    Connected(ConnectionId),
    Message(ConnectionId, FromExtension),
    /// The connection sent something that is not a valid message; it was closed.
    Invalid(ConnectionId),
    Closed(ConnectionId),
}

struct Current {
    id: ConnectionId,
    writer: TcpStream,
}

/// The app's listener; stops and removes its channel file when dropped.
pub struct Listener {
    file: PathBuf,
    secret_hex: String,
    stop: Arc<AtomicBool>,
    current: Arc<Mutex<Option<Current>>>,
    events: mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Listener {
    /// Listens on a random loopback port and publishes it with a fresh secret in `folder`.
    /// `wake` runs after every event so the window can repaint.
    pub fn start(
        folder: &Path,
        events: mpsc::Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let secret = random_bytes::<32>()?;
        let secret_hex = hex(&secret);
        std::fs::create_dir_all(folder)?;
        let file = folder.join(CHANNEL_FILE);
        write_private(
            &file,
            &serde_json::to_vec(&ChannelFile {
                port: listener.local_addr()?.port(),
                secret: secret_hex.clone(),
            })
            .map_err(io::Error::other)?,
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        let current = Arc::new(Mutex::new(None));
        let (thread_stop, thread_current) = (stop.clone(), current.clone());
        let (thread_events, thread_wake) = (events.clone(), wake.clone());
        std::thread::spawn(move || {
            accept_loop(
                listener,
                secret,
                thread_stop,
                thread_current,
                thread_events,
                thread_wake,
            )
        });
        Ok(Self {
            file,
            secret_hex,
            stop,
            current,
            events,
            wake,
        })
    }

    /// Sends `message` to connection `id` if it is still the current one.
    pub fn send(&self, id: ConnectionId, message: &ToExtension) -> bool {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        match current.as_mut() {
            Some(connection) if connection.id == id => {
                framing::write_message(&mut connection.writer, message).is_ok()
            }
            _ => false,
        }
    }

    /// Closes connection `id` if it is the current one and reports it closed. The host sees
    /// the end of the stream and exits, which also ends the connection's reading thread.
    pub fn close(&self, id: ConnectionId) {
        let closed = {
            let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
            match current.take() {
                Some(connection) if connection.id == id => {
                    let _ = connection.writer.shutdown(std::net::Shutdown::Both);
                    true
                }
                other => {
                    *current = other;
                    false
                }
            }
        };
        if closed {
            let _ = self.events.send(Event::Closed(id));
            (self.wake)();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(current) = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = current.writer.shutdown(std::net::Shutdown::Both);
        }
        // Another instance may have replaced the file since; only remove our own.
        let ours = std::fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ChannelFile>(&bytes).ok())
            .is_some_and(|file| file.secret == self.secret_hex);
        if ours {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

/// Replaces `path` with `bytes`, readable only by the owner where the platform has modes.
/// On Windows the file inherits the per-user folder's permissions.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let _ = std::fs::remove_file(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.flush()
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn accept_loop(
    listener: TcpListener,
    secret: Secret,
    stop: Arc<AtomicBool>,
    current: Arc<Mutex<Option<Current>>>,
    events: mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) if peer.ip().is_loopback() => {
                let (current, events, wake, stop) =
                    (current.clone(), events.clone(), wake.clone(), stop.clone());
                std::thread::spawn(move || serve(stream, secret, stop, current, events, wake));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

fn serve(
    mut stream: TcpStream,
    secret: Secret,
    stop: Arc<AtomicBool>,
    current: Arc<Mutex<Option<Current>>>,
    events: mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_nodelay(true);
    if accept_handshake(&mut stream, &secret).is_err() || stop.load(Ordering::Relaxed) {
        return;
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    {
        let mut current = current.lock().unwrap_or_else(|e| e.into_inner());
        if current.is_some() {
            // One browser profile at a time: never mix or silently switch sessions.
            let _ = framing::write_message(
                &mut stream,
                &ToExtension::Status {
                    state: super::messages::State::Busy,
                },
            );
            return;
        }
        let Ok(writer) = stream.try_clone() else {
            return;
        };
        *current = Some(Current { id, writer });
    }
    let notify = |event| {
        let _ = events.send(event);
        wake();
    };
    notify(Event::Connected(id));
    loop {
        match framing::read_message::<FromExtension>(&mut stream) {
            Ok(message) => notify(Event::Message(id, message)),
            Err(FrameError::Invalid | FrameError::TooLarge) => {
                notify(Event::Invalid(id));
                break;
            }
            Err(_) => break,
        }
    }
    // `Listener::close` already reported a connection it closed.
    let was_current = {
        let mut current = current.lock().unwrap_or_else(|e| e.into_inner());
        current.take_if(|connection| connection.id == id).is_some()
    };
    let _ = stream.shutdown(std::net::Shutdown::Both);
    if was_current {
        notify(Event::Closed(id));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::session::chrome::messages::State;

    /// The RFC 4231 test case 1 vector.
    #[test]
    fn hmac_matches_the_standard_vector() {
        let mac = hmac(&[0x0b; 20], &[b"Hi ", b"There"]);
        assert_eq!(
            hex(&mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert!(same(&mac, &mac));
        let mut other = mac;
        other[31] ^= 1;
        assert!(!same(&mac, &other));
        assert!(!same(&mac, &mac[..31]));
    }

    #[test]
    fn random_bytes_differ_and_hex_round_trips() {
        let a = random_bytes::<32>().unwrap();
        let b = random_bytes::<32>().unwrap();
        assert_ne!(a, b);
        assert_eq!(unhex::<32>(&hex(&a)), Some(a));
        assert_eq!(unhex::<2>("zz00"), None);
        assert_eq!(unhex::<2>("abc"), None);
        assert_eq!(unhex::<1>("+1"), None);
    }

    pub(crate) fn listener(folder: &Path) -> (Listener, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        (Listener::start(folder, tx, Arc::new(|| {})).unwrap(), rx)
    }

    pub(crate) fn next(rx: &mpsc::Receiver<Event>) -> Event {
        rx.recv_timeout(Duration::from_secs(10)).expect("an event")
    }

    #[test]
    fn an_authenticated_host_exchanges_messages_with_the_app() {
        let folder = tempfile::tempdir().unwrap();
        let (listener, rx) = listener(folder.path());
        let mut host = connect(folder.path()).unwrap();
        let Event::Connected(id) = next(&rx) else {
            panic!("not connected")
        };
        framing::write_message(&mut host, &FromExtension::Hello { protocol: 1 }).unwrap();
        assert!(matches!(
            next(&rx),
            Event::Message(got, FromExtension::Hello { protocol: 1 }) if got == id
        ));
        assert!(listener.send(id, &ToExtension::Refresh));
        let message: ToExtension = framing::read_message(&mut host).unwrap();
        assert_eq!(message, ToExtension::Refresh);
        assert!(!listener.send(id + 1000, &ToExtension::Refresh));
        drop(host);
        assert!(matches!(next(&rx), Event::Closed(got) if got == id));
        assert!(!listener.send(id, &ToExtension::Refresh));
    }

    #[test]
    fn a_wrong_secret_fails_both_ways_and_nothing_is_delivered() {
        let folder = tempfile::tempdir().unwrap();
        let (_listener, rx) = listener(folder.path());
        let path = folder.path().join(CHANNEL_FILE);
        let real: ChannelFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

        // A host that does not know the secret: the app's proof does not verify for it, so
        // it stops; and its own guessed proof never makes the app accept it.
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, real.port)).unwrap();
        let wrong = [7u8; 32];
        assert!(connect_handshake(&mut stream, &wrong).is_err());
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, real.port)).unwrap();
        framing::write_message(
            &mut stream,
            &Hello {
                hello: HELLO.into(),
                version: VERSION,
                nonce: hex(&[1; 32]),
            },
        )
        .unwrap();
        let _: Challenge = framing::read_message(&mut stream).unwrap();
        framing::write_message(
            &mut stream,
            &Proof {
                proof: hex(&[0; 32]),
            },
        )
        .unwrap();
        framing::write_message(&mut stream, &FromExtension::Disconnect).unwrap();
        // Garbage before the handshake is refused too.
        let mut garbage = TcpStream::connect((Ipv4Addr::LOCALHOST, real.port)).unwrap();
        std::io::Write::write_all(&mut garbage, &framing::frame(b"{\"type\":\"hello\"}")).unwrap();
        assert!(
            rx.recv_timeout(Duration::from_millis(500)).is_err(),
            "An unauthenticated peer reached the app"
        );
    }

    #[test]
    fn the_host_refuses_a_listener_that_cannot_prove_the_secret() {
        let folder = tempfile::tempdir().unwrap();
        // An impostor holds the published port but not the secret.
        let impostor = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = impostor.local_addr().unwrap().port();
        std::fs::write(
            folder.path().join(CHANNEL_FILE),
            serde_json::to_vec(&ChannelFile {
                port,
                secret: hex(&[9; 32]),
            })
            .unwrap(),
        )
        .unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = impostor.accept().unwrap();
            let _: Hello = framing::read_message(&mut stream).unwrap();
            framing::write_message(
                &mut stream,
                &Challenge {
                    nonce: hex(&[2; 32]),
                    proof: hex(&[3; 32]),
                },
            )
            .unwrap();
            // The host must send nothing after a failed proof.
            matches!(
                framing::read_frame(&mut stream),
                Err(FrameError::Closed | FrameError::Io(_))
            )
        });
        assert!(connect(folder.path()).is_err());
        assert!(thread.join().unwrap());
    }

    #[test]
    fn one_connection_at_a_time_and_the_file_goes_with_the_listener() {
        let folder = tempfile::tempdir().unwrap();
        let (listener, rx) = listener(folder.path());
        let _first = connect(folder.path()).unwrap();
        let Event::Connected(id) = next(&rx) else {
            panic!("not connected")
        };
        let mut second = connect(folder.path()).unwrap();
        let busy: ToExtension = framing::read_message(&mut second).unwrap();
        assert_eq!(busy, ToExtension::Status { state: State::Busy });
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        listener.close(id);
        assert!(matches!(next(&rx), Event::Closed(got) if got == id));
        let file = folder.path().join(CHANNEL_FILE);
        assert!(file.is_file());
        drop(listener);
        assert!(!file.exists());
        assert!(connect(folder.path()).is_err());
    }

    #[test]
    fn invalid_messages_close_the_connection() {
        let folder = tempfile::tempdir().unwrap();
        let (_listener, rx) = listener(folder.path());
        let mut host = connect(folder.path()).unwrap();
        let Event::Connected(id) = next(&rx) else {
            panic!("not connected")
        };
        std::io::Write::write_all(&mut host, &framing::frame(b"{\"type\":\"eval\"}")).unwrap();
        assert!(matches!(next(&rx), Event::Invalid(got) if got == id));
        assert!(matches!(next(&rx), Event::Closed(got) if got == id));
    }

    #[test]
    fn a_replaced_channel_file_is_left_alone() {
        let folder = tempfile::tempdir().unwrap();
        let (first, _rx) = listener(folder.path());
        let (second, _rx2) = listener(folder.path());
        drop(first);
        assert!(folder.path().join(CHANNEL_FILE).is_file());
        assert!(connect(folder.path()).is_ok());
        drop(second);
        assert!(!folder.path().join(CHANNEL_FILE).exists());
    }
}
