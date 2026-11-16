//! The destination guard for other sites: a small forward proxy on 127.0.0.1 that yt-dlp, and
//! FFmpeg when yt-dlp hands it a stream, send every request through. For each request the guard
//! looks the host up once, applies the pasted link's rules (`other::normalize` for the scheme,
//! port and name, `other::checked_addresses` for the addresses) and connects to an address it has
//! just checked. A nested request (an embed, a manifest, a segment, a key, a redirect) to a local
//! or private address is refused, and so is a name whose DNS answer changes after a check
//! (rebinding), as no second lookup is made. HTTPS is tunnelled with CONNECT, never decrypted.
//!
//! One guard serves one inspection or download of an Other link and stops when it is dropped,
//! after the helper ends or is cancelled. When the system or the environment sets a proxy the
//! guard is not used (`system_proxy`). See `docs/reference/other-sites.md`.
use super::{EngineError, sources::other};
use std::{
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use url::Url;

const START_FAILED: &str = "stream-dlp could not start its address check for other sites. Restart stream-dlp and try again.";

type Lookup = dyn Fn(&str, u16) -> io::Result<Vec<IpAddr>> + Send + Sync;
type Connect = dyn Fn(SocketAddr, Duration) -> io::Result<TcpStream> + Send + Sync;

/// How the guard finds and reaches a destination: the system's DNS and sockets, or a fake
/// network in tests.
struct Policy {
    lookup: Box<Lookup>,
    connect: Box<Connect>,
}

impl Policy {
    fn system() -> Self {
        #[cfg(test)]
        if let Some(policy) = test_network::policy() {
            return policy;
        }
        Self {
            lookup: Box::new(other::system_lookup),
            connect: Box::new(|address, timeout| TcpStream::connect_timeout(&address, timeout)),
        }
    }
}

/// Bounds on what one guard accepts.
#[derive(Clone, Copy)]
struct Limits {
    /// Client connections served at once; more are answered 503 and closed.
    connections: usize,
    /// The largest request head (request line and headers).
    head: usize,
    /// The largest response head passed back for a plain HTTP request.
    response_head: usize,
    /// Time for a client to send its request head.
    head_time: Duration,
    /// Time to connect to a destination.
    connect_time: Duration,
    /// A connection with no data in either direction for this long is closed.
    idle_time: Duration,
}

const LIMITS: Limits = Limits {
    connections: 32,
    head: 16 * 1024,
    response_head: 64 * 1024,
    head_time: Duration::from_secs(15),
    connect_time: Duration::from_secs(15),
    idle_time: Duration::from_secs(60),
};

struct Shared {
    policy: Policy,
    limits: Limits,
    stopping: AtomicBool,
    /// Requests refused because their destination is not public or fails the link policy.
    refused: AtomicUsize,
    active: AtomicUsize,
    /// Every open socket, so stopping can close them and end their threads.
    sockets: Mutex<Vec<(u64, TcpStream)>>,
    next_socket: AtomicU64,
}

impl Shared {
    fn register(&self, stream: &TcpStream) -> Option<u64> {
        let id = self.next_socket.fetch_add(1, Ordering::Relaxed);
        let clone = stream.try_clone().ok()?;
        self.sockets.lock().ok()?.push((id, clone));
        // Registered after `stop` closed the others: close this one too.
        if self.stopping.load(Ordering::SeqCst) {
            let _ = stream.shutdown(Shutdown::Both);
        }
        Some(id)
    }

    fn unregister(&self, id: Option<u64>) {
        if let (Some(id), Ok(mut sockets)) = (id, self.sockets.lock()) {
            sockets.retain(|(other, _)| *other != id);
        }
    }

    fn close_all(&self) {
        if let Ok(sockets) = self.sockets.lock() {
            for (_, socket) in sockets.iter() {
                let _ = socket.shutdown(Shutdown::Both);
            }
        }
    }
}

/// A running guard. Dropping it stops the listener and closes every connection.
pub(in crate::engine) struct Guard {
    address: SocketAddr,
    shared: Arc<Shared>,
    acceptor: Option<thread::JoinHandle<()>>,
}

impl Guard {
    /// The guard for one inspection or download of an Other link, or `None` when the system or
    /// the environment sets a proxy: the helpers then keep using it, unguarded.
    pub(in crate::engine) fn for_other_link() -> Result<Option<Self>, EngineError> {
        #[cfg(test)]
        let forced = test_network::active();
        #[cfg(not(test))]
        let forced = false;
        if !forced && system_proxy() {
            return Ok(None);
        }
        Self::start(Policy::system(), LIMITS)
            .map(Some)
            .map_err(|_| EngineError::Helper(START_FAILED.into()))
    }

    fn start(policy: Policy, limits: Limits) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let address = listener.local_addr()?;
        let shared = Arc::new(Shared {
            policy,
            limits,
            stopping: AtomicBool::new(false),
            refused: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            sockets: Mutex::new(Vec::new()),
            next_socket: AtomicU64::new(0),
        });
        let acceptor = {
            let shared = shared.clone();
            thread::Builder::new()
                .name("other-site guard".into())
                .spawn(move || accept(&listener, &shared))?
        };
        Ok(Self {
            address,
            shared,
            acceptor: Some(acceptor),
        })
    }

    /// A guard on the system's network, for tests of the commands that use one.
    #[cfg(test)]
    pub(in crate::engine) fn for_tests() -> Self {
        Self::start(Policy::system(), LIMITS).unwrap()
    }

    /// The `--proxy` value for yt-dlp.
    pub(in crate::engine) fn proxy_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Whether a request was refused for its destination. A helper that failed after one was
    /// refused is explained by that refusal, not as a network failure.
    pub(in crate::engine) fn refused(&self) -> bool {
        self.shared.refused.load(Ordering::SeqCst) > 0
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared.close_all();
        // Wake the blocking accept so the acceptor sees `stopping`.
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_secs(1));
        if let Some(acceptor) = self.acceptor.take() {
            let _ = acceptor.join();
        }
    }
}

/// Only programs on this machine may use the guard. It listens on 127.0.0.1 only; this is the
/// second check.
fn accepted_client(peer: SocketAddr) -> bool {
    peer.ip().is_loopback()
}

/// Serves connections until the guard stops. Connection threads are not waited for: stopping
/// closes every socket they hold (and any they open later), so each ends within its connect or
/// lookup time, and a cancelled download is not held up by a slow destination.
fn accept(listener: &TcpListener, shared: &Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
        let Ok(mut stream) = stream else {
            thread::sleep(Duration::from_millis(10));
            continue;
        };
        if !stream.peer_addr().is_ok_and(accepted_client) {
            continue;
        }
        if shared.active.load(Ordering::SeqCst) >= shared.limits.connections {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
            respond(&mut stream, Answer::Busy);
            continue;
        }
        shared.active.fetch_add(1, Ordering::SeqCst);
        let worker = shared.clone();
        let spawned = thread::Builder::new()
            .name("other-site guard connection".into())
            .spawn(move || {
                handle(&worker, stream);
                worker.active.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            shared.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
    shared.close_all();
}

/// Why a request got no tunnel or response from its destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// Malformed, too large, too slow, or not a proxy request.
    BadRequest,
    /// The destination is not public or fails the link policy.
    Forbidden,
    /// The destination could not be looked up or reached.
    BadGateway,
    /// Too many connections.
    Busy,
}

fn respond(stream: &mut TcpStream, answer: Answer) {
    let status = match answer {
        Answer::BadRequest => "400 Bad Request",
        Answer::Forbidden => "403 Forbidden",
        Answer::BadGateway => "502 Bad Gateway",
        Answer::Busy => "503 Service Unavailable",
    };
    let _ = stream.write_all(
        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
    );
    let _ = stream.flush();
}

fn handle(shared: &Shared, mut client: TcpStream) {
    let id = shared.register(&client);
    if let Err(answer) = serve(shared, &mut client) {
        if answer == Answer::Forbidden {
            shared.refused.fetch_add(1, Ordering::SeqCst);
        }
        respond(&mut client, answer);
        // Read a little of what the client is still sending before closing, so the answer is
        // not lost to a reset. Bounded in bytes and time.
        let _ = client.shutdown(Shutdown::Write);
        let _ = client.set_read_timeout(Some(Duration::from_millis(250)));
        let _ = io::copy(&mut (&client).take(64 * 1024), &mut io::sink());
    }
    let _ = client.shutdown(Shutdown::Both);
    shared.unregister(id);
}

/// Reads up to the end of an HTTP head: the head and whatever followed it.
fn read_head(
    stream: &mut TcpStream,
    limit: usize,
    deadline: Instant,
) -> Result<(String, Vec<u8>), Answer> {
    let mut buffer = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            if end > limit {
                return Err(Answer::BadRequest);
            }
            let rest = buffer.split_off(end + 4);
            buffer.truncate(end);
            let head = String::from_utf8(buffer).map_err(|_| Answer::BadRequest)?;
            return Ok((head, rest));
        }
        if buffer.len() > limit {
            return Err(Answer::BadRequest);
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or(Answer::BadRequest)?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| Answer::BadRequest)?;
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(Answer::BadRequest),
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
    }
}

/// A request line and its headers.
struct Head<'a> {
    method: &'a str,
    target: &'a str,
    version: &'a str,
    headers: Vec<(&'a str, &'a str)>,
}

fn parse_head(head: &str) -> Result<Head<'_>, Answer> {
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Answer::BadRequest);
    };
    let token = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
    };
    if !token(method) || target.is_empty() || !version.starts_with("HTTP/1.") {
        return Err(Answer::BadRequest);
    }
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(Answer::BadRequest)?;
        if !token(name) || value.bytes().any(|b| b == b'\r' || b == b'\n') {
            return Err(Answer::BadRequest);
        }
        headers.push((name, value.trim()));
    }
    Ok(Head {
        method,
        target,
        version,
        headers,
    })
}

fn serve(shared: &Shared, client: &mut TcpStream) -> Result<(), Answer> {
    let limits = shared.limits;
    let (head, rest) = read_head(client, limits.head, Instant::now() + limits.head_time)?;
    let head = parse_head(&head)?;
    if head.method.eq_ignore_ascii_case("CONNECT") {
        tunnel(shared, client, &head, &rest)
    } else {
        forward(shared, client, &head, rest)
    }
}

/// Checks `url` with the link policy and looks its host up once: the checked addresses to
/// connect to, or why not.
fn destination(shared: &Shared, url: &Url) -> Result<Vec<SocketAddr>, Answer> {
    other::normalize(url).map_err(|_| Answer::Forbidden)?;
    let port = url.port_or_known_default().ok_or(Answer::Forbidden)?;
    match other::checked_addresses(url, &mut |host, port| (shared.policy.lookup)(host, port)) {
        Ok(addresses) => Ok(addresses
            .into_iter()
            .map(|ip| SocketAddr::new(ip, port))
            .collect()),
        Err(EngineError::Unsupported(_)) => Err(Answer::Forbidden),
        Err(_) => Err(Answer::BadGateway),
    }
}

/// Connects to the first of the checked addresses that answers.
fn connect(shared: &Shared, addresses: &[SocketAddr]) -> Result<TcpStream, Answer> {
    let deadline = Instant::now() + shared.limits.connect_time;
    for address in addresses {
        let Some(left) = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
        else {
            break;
        };
        if let Ok(stream) = (shared.policy.connect)(*address, left) {
            return Ok(stream);
        }
    }
    Err(Answer::BadGateway)
}

/// `CONNECT host:443`: the link policy allows only the default HTTPS port, so the target is
/// checked as `https://host/`.
fn tunnel(shared: &Shared, client: &mut TcpStream, head: &Head, rest: &[u8]) -> Result<(), Answer> {
    let (host, port) = head.target.rsplit_once(':').ok_or(Answer::BadRequest)?;
    if host.is_empty()
        || host
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b"/\\@?#%".contains(&b))
    {
        return Err(Answer::BadRequest);
    }
    if port != "443" {
        return Err(Answer::Forbidden);
    }
    let url = Url::parse(&format!("https://{host}/")).map_err(|_| Answer::BadRequest)?;
    let addresses = destination(shared, &url)?;
    let mut upstream = connect(shared, &addresses)?;
    let upstream_id = shared.register(&upstream);
    let result = (|| {
        client
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .ok()?;
        upstream.write_all(rest).ok()?;
        let idle = shared.limits.idle_time;
        // Sockets wake up regularly to see whether the other direction is still moving: a
        // client that only receives (a long HTTPS download) sends nothing for minutes.
        let poll = Some(idle.min(Duration::from_secs(5)));
        for stream in [&*client, &upstream] {
            stream.set_read_timeout(poll).ok()?;
            stream.set_write_timeout(Some(idle)).ok()?;
        }
        let activity = Arc::new(Activity::new());
        let (mut from_client, mut to_upstream) =
            (client.try_clone().ok()?, upstream.try_clone().ok()?);
        let outgoing_activity = activity.clone();
        let outgoing = thread::Builder::new()
            .name("other-site guard tunnel".into())
            .spawn(move || {
                relay(&mut from_client, &mut to_upstream, &outgoing_activity, idle);
                let _ = to_upstream.shutdown(Shutdown::Both);
                let _ = from_client.shutdown(Shutdown::Both);
            })
            .ok()?;
        relay(&mut upstream, client, &activity, idle);
        let _ = upstream.shutdown(Shutdown::Both);
        let _ = client.shutdown(Shutdown::Both);
        let _ = outgoing.join();
        Some(())
    })();
    let _ = upstream.shutdown(Shutdown::Both);
    shared.unregister(upstream_id);
    // Once the destination is connected, nothing more is sent as an answer.
    let _ = result;
    Ok(())
}

/// When data last moved in either direction of a tunnel.
struct Activity {
    start: Instant,
    last: AtomicU64,
}

impl Activity {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            last: AtomicU64::new(0),
        }
    }

    fn touch(&self) {
        let now = u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.last.store(now, Ordering::Relaxed);
    }

    fn idle(&self) -> Duration {
        self.start
            .elapsed()
            .saturating_sub(Duration::from_millis(self.last.load(Ordering::Relaxed)))
    }
}

/// Copies one direction of a tunnel until it ends, fails, or neither direction has moved data
/// for `idle`.
fn relay(from: &mut TcpStream, to: &mut TcpStream, activity: &Activity, idle: Duration) {
    let mut buffer = [0; 16 * 1024];
    loop {
        match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if to.write_all(&buffer[..n]).is_err() {
                    break;
                }
                activity.touch();
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                if activity.idle() >= idle {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// Headers that belong to one connection and are not passed on. Every connection carries one
/// request, so both sides are told `Connection: close`.
fn hop_by_hop(name: &str, listed: &[&str]) -> bool {
    [
        "connection",
        "proxy-connection",
        "keep-alive",
        "proxy-authorization",
        "proxy-authenticate",
        "te",
        "trailer",
        "upgrade",
        "expect",
    ]
    .iter()
    .chain(listed)
    .any(|hop| name.eq_ignore_ascii_case(hop))
}

/// A plain HTTP request with an absolute `http://` URI: exactly one request per connection, so
/// a later request on a kept-alive connection can never go out unchecked.
fn forward(
    shared: &Shared,
    client: &mut TcpStream,
    head: &Head,
    rest: Vec<u8>,
) -> Result<(), Answer> {
    if head.target.starts_with('/') {
        // Origin-form: a direct request, not one for a proxy.
        return Err(Answer::BadRequest);
    }
    let url = Url::parse(head.target).map_err(|_| Answer::BadRequest)?;
    if url.scheme() != "http" {
        return Err(Answer::Forbidden);
    }
    if head
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
    {
        return Err(Answer::BadRequest);
    }
    let mut lengths = head
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.parse::<u64>().map_err(|_| Answer::BadRequest));
    let length = lengths.next().transpose()?.unwrap_or(0);
    if lengths.next().is_some() {
        return Err(Answer::BadRequest);
    }
    let addresses = destination(shared, &url)?;
    let listed: Vec<&str> = head
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("connection"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .collect();
    let mut request = format!(
        "{} {}{} {}\r\nHost: {}\r\n",
        head.method,
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default(),
        head.version,
        url.host_str().unwrap_or_default(),
    );
    for (name, value) in &head.headers {
        if !name.eq_ignore_ascii_case("host") && !hop_by_hop(name, &listed) {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    request.push_str("Connection: close\r\n\r\n");

    let mut upstream = connect(shared, &addresses)?;
    let upstream_id = shared.register(&upstream);
    let idle = Some(shared.limits.idle_time);
    let result = (|| -> Result<(), Answer> {
        for stream in [&*client, &upstream] {
            stream
                .set_read_timeout(idle)
                .map_err(|_| Answer::BadGateway)?;
            stream
                .set_write_timeout(idle)
                .map_err(|_| Answer::BadGateway)?;
        }
        upstream
            .write_all(request.as_bytes())
            .map_err(|_| Answer::BadGateway)?;
        // The body: exactly Content-Length bytes, whatever the client sends after it.
        let early = rest
            .len()
            .min(usize::try_from(length).unwrap_or(usize::MAX));
        upstream
            .write_all(&rest[..early])
            .map_err(|_| Answer::BadGateway)?;
        let left = length - early as u64;
        if left > 0 {
            let copied = io::copy(&mut (&*client).take(left), &mut upstream)
                .map_err(|_| Answer::BadRequest)?;
            if copied < left {
                return Err(Answer::BadRequest);
            }
        }
        let (response, body) = read_head(
            &mut upstream,
            shared.limits.response_head,
            Instant::now() + shared.limits.idle_time,
        )
        .map_err(|_| Answer::BadGateway)?;
        let mut lines = response.split("\r\n");
        let mut answer = format!("{}\r\n", lines.next().unwrap_or_default());
        for line in lines {
            let name = line.split(':').next().unwrap_or_default();
            if !["connection", "proxy-connection", "keep-alive"]
                .iter()
                .any(|hop| name.trim().eq_ignore_ascii_case(hop))
            {
                answer.push_str(line);
                answer.push_str("\r\n");
            }
        }
        answer.push_str("Connection: close\r\n\r\n");
        // From here on the client has an answer: failures only end the connection.
        if client.write_all(answer.as_bytes()).is_ok() && client.write_all(&body).is_ok() {
            let _ = io::copy(&mut upstream, client);
        }
        Ok(())
    })();
    let _ = upstream.shutdown(Shutdown::Both);
    shared.unregister(upstream_id);
    result
}

/// Whether a proxy is set for the helpers, found the way yt-dlp finds one (Python's
/// `urllib.request.getproxies`): the environment first, then the Windows Internet settings or the
/// macOS network settings. The guard is then not used, so the helpers keep reaching the network
/// the way the user set up; the address check applies only to the pasted link and its redirects.
pub(in crate::engine) fn system_proxy() -> bool {
    proxy_in_environment(|name| std::env::var(name).ok()) || platform_proxy()
}

fn proxy_in_environment(get: impl Fn(&str) -> Option<String>) -> bool {
    [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ]
    .iter()
    .any(|name| get(name).is_some_and(|value| !value.trim().is_empty()))
}

#[cfg(windows)]
fn platform_proxy() -> bool {
    let output = crate::process::isolate(std::process::Command::new("reg").args([
        "query",
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
    ]))
    .output();
    output.is_ok_and(|output| registry_proxy(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(target_os = "macos")]
fn platform_proxy() -> bool {
    let output =
        crate::process::isolate(std::process::Command::new("scutil").arg("--proxy")).output();
    output.is_ok_and(|output| scutil_proxy(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_proxy() -> bool {
    false
}

/// `reg query` of the Internet settings: a proxy when `ProxyEnable` is on and `ProxyServer` is
/// set, as Python reads it.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn registry_proxy(output: &str) -> bool {
    let value = |name: &str| {
        output.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            (parts.next() == Some(name)).then(|| parts.skip(1).collect::<Vec<_>>().join(" "))
        })
    };
    let enabled = value("ProxyEnable")
        .is_some_and(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).is_ok_and(|n| n != 0));
    enabled && value("ProxyServer").is_some_and(|v| !v.trim().is_empty())
}

/// `scutil --proxy`: a proxy when HTTP or HTTPS proxying is enabled.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn scutil_proxy(output: &str) -> bool {
    output.lines().any(|line| {
        let mut parts = line.split(':').map(str::trim);
        matches!(
            (parts.next(), parts.next()),
            (Some("HTTPEnable" | "HTTPSEnable"), Some("1"))
        )
    })
}

/// A fake network for the end-to-end test through the real helpers: names that resolve to chosen
/// addresses, and addresses that are really served by local fixture servers. While it is set,
/// every guard uses it, even if the machine has a proxy.
#[cfg(test)]
pub(in crate::engine) mod test_network {
    use super::Policy;
    use std::{
        io,
        net::{IpAddr, SocketAddr, TcpStream},
        sync::{Arc, Mutex},
    };

    pub struct TestNetwork {
        pub hosts: Vec<(String, IpAddr)>,
        pub routes: Vec<(SocketAddr, SocketAddr)>,
    }

    static NETWORK: Mutex<Option<Arc<TestNetwork>>> = Mutex::new(None);

    pub fn set(network: Option<TestNetwork>) {
        *NETWORK.lock().unwrap() = network.map(Arc::new);
    }

    pub(super) fn active() -> bool {
        NETWORK.lock().unwrap().is_some()
    }

    pub(super) fn policy() -> Option<Policy> {
        let network = NETWORK.lock().unwrap().clone()?;
        let routes = network.clone();
        Some(Policy {
            lookup: Box::new(move |host, _| {
                network
                    .hosts
                    .iter()
                    .find(|(name, _)| name == host)
                    .map(|(_, ip)| vec![*ip])
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown test host"))
            }),
            connect: Box::new(move |address, timeout| {
                let (_, local) = routes
                    .routes
                    .iter()
                    .find(|(public, _)| *public == address)
                    .ok_or_else(|| io::Error::from(io::ErrorKind::ConnectionRefused))?;
                TcpStream::connect_timeout(local, timeout)
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const PUBLIC: &str = "93.184.215.14";

    /// An upstream server on localhost: answers every connection with `reply` after reading one
    /// head, and reports what it read.
    fn upstream(reply: &'static [u8]) -> (SocketAddr, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let tx = tx.clone();
                thread::spawn(move || {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut seen = Vec::new();
                    let mut chunk = [0; 4096];
                    while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => seen.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let _ = tx.send(String::from_utf8_lossy(&seen).into_owned());
                    let _ = stream.write_all(reply);
                    // Keep the connection open briefly, as a keep-alive server would.
                    thread::sleep(Duration::from_millis(100));
                });
            }
        });
        (address, rx)
    }

    /// An echo server, for tunnels.
    fn echo() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                thread::spawn(move || {
                    let mut reader = stream.try_clone().unwrap();
                    let _ = io::copy(&mut reader, &mut stream);
                });
            }
        });
        address
    }

    type Log = Arc<Mutex<Vec<String>>>;

    /// A policy whose lookup answers from `answer` and whose connections go to `route` for any
    /// public address; lookups and connections are logged.
    fn fake_policy(
        answer: impl Fn(&str, usize) -> Vec<IpAddr> + Send + Sync + 'static,
        route: SocketAddr,
    ) -> (Policy, Log, Log) {
        let lookups: Log = Arc::default();
        let connects: Log = Arc::default();
        let (l, c) = (lookups.clone(), connects.clone());
        let policy = Policy {
            lookup: Box::new(move |host, _| {
                let mut log = l.lock().unwrap();
                log.push(host.to_owned());
                Ok(answer(host, log.len()))
            }),
            connect: Box::new(move |address, timeout| {
                c.lock().unwrap().push(address.to_string());
                assert!(
                    other::public_address(address.ip()),
                    "connected to {address}"
                );
                TcpStream::connect_timeout(&route, timeout)
            }),
        };
        (policy, lookups, connects)
    }

    fn public(_: &str, _: usize) -> Vec<IpAddr> {
        vec![PUBLIC.parse().unwrap()]
    }

    /// Sends `request` to the guard and reads until it closes the connection.
    fn exchange(guard: &Guard, request: &[u8]) -> String {
        let mut stream = TcpStream::connect(guard.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(request).unwrap();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
        String::from_utf8_lossy(&answer).into_owned()
    }

    #[test]
    fn plain_http_carries_one_request_per_connection_to_the_checked_address() {
        let (route, seen) = upstream(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\nKeep-Alive: timeout=5\r\n\r\nok",
        );
        let (policy, lookups, connects) = fake_policy(public, route);
        let guard = Guard::start(policy, LIMITS).unwrap();
        assert!(guard.proxy_url().starts_with("http://127.0.0.1:"));
        // A second request on the same connection, to another host, must not go out.
        let answer = exchange(
            &guard,
            b"GET http://media.example.test:80/a.ts?x=1 HTTP/1.1\r\nHost: media.example.test\r\n\
              Proxy-Connection: keep-alive\r\nConnection: keep-alive, X-Hop\r\nX-Hop: 1\r\n\
              User-Agent: test\r\n\r\n\
              GET http://other.example.test/b.ts HTTP/1.1\r\nHost: other.example.test\r\n\r\n",
        );
        assert!(answer.starts_with("HTTP/1.1 200 OK\r\n"), "{answer}");
        assert!(answer.contains("Connection: close\r\n"), "{answer}");
        assert!(!answer.contains("keep-alive") && !answer.contains("Keep-Alive"));
        assert!(answer.ends_with("\r\n\r\nok"), "{answer}");
        let request = seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(request.starts_with("GET /a.ts?x=1 HTTP/1.1\r\nHost: media.example.test\r\n"));
        assert!(request.contains("User-Agent: test\r\n"));
        assert!(request.contains("Connection: close\r\n"));
        for hop in ["Proxy-Connection", "keep-alive", "X-Hop", "other.example"] {
            assert!(!request.contains(hop), "{hop} in {request}");
        }
        assert!(seen.recv_timeout(Duration::from_millis(300)).is_err());
        assert_eq!(*lookups.lock().unwrap(), ["media.example.test"]);
        assert_eq!(*connects.lock().unwrap(), [format!("{PUBLIC}:80")]);
        assert!(!guard.refused());
    }

    #[test]
    fn connect_tunnels_bytes_to_the_checked_address_without_reading_them() {
        let (policy, lookups, connects) = fake_policy(public, echo());
        let guard = Guard::start(policy, LIMITS).unwrap();
        let mut stream = TcpStream::connect(guard.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // Bytes sent with the head travel through too.
        stream
            .write_all(b"CONNECT media.example.test:443 HTTP/1.1\r\nHost: media.example.test:443\r\n\r\nfirst")
            .unwrap();
        let established = b"HTTP/1.1 200 Connection established\r\n\r\n";
        let mut answer = vec![0; established.len() + 5];
        stream.read_exact(&mut answer).unwrap();
        assert_eq!(&answer[..established.len()], established);
        assert_eq!(&answer[established.len()..], b"first");
        stream.write_all(b"\x16\x03\x01 tls bytes").unwrap();
        let mut echoed = [0; 13];
        stream.read_exact(&mut echoed).unwrap();
        assert_eq!(&echoed, b"\x16\x03\x01 tls bytes");
        assert_eq!(*lookups.lock().unwrap(), ["media.example.test"]);
        assert_eq!(*connects.lock().unwrap(), [format!("{PUBLIC}:443")]);
    }

    #[test]
    fn every_non_public_address_class_is_refused_through_the_proxy() {
        for blocked in [
            "127.0.0.1",
            "0.0.0.0",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "240.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.0.1",
            "64:ff9b::a00:1",
            "2002:c0a8:101::1",
        ] {
            let ip: IpAddr = blocked.parse().unwrap();
            // One private answer among public ones refuses the name.
            let (policy, _, connects) =
                fake_policy(move |_, _| vec![PUBLIC.parse().unwrap(), ip], echo());
            let guard = Guard::start(policy, LIMITS).unwrap();
            for request in [
                &b"CONNECT private.example.test:443 HTTP/1.1\r\n\r\n"[..],
                b"GET http://private.example.test/seg.ts HTTP/1.1\r\nHost: private.example.test\r\n\r\n",
            ] {
                let answer = exchange(&guard, request);
                assert!(answer.starts_with("HTTP/1.1 403 "), "{blocked}: {answer}");
            }
            assert!(connects.lock().unwrap().is_empty(), "{blocked}");
            assert!(guard.refused(), "{blocked}");
        }
    }

    #[test]
    fn targets_outside_the_link_policy_are_refused_and_malformed_requests_rejected() {
        let (policy, lookups, connects) = fake_policy(public, echo());
        let guard = Guard::start(policy, LIMITS).unwrap();
        for forbidden in [
            "CONNECT 127.0.0.1:443 HTTP/1.1",
            "CONNECT [::1]:443 HTTP/1.1",
            "CONNECT 2130706433:443 HTTP/1.1",
            "CONNECT localhost:443 HTTP/1.1",
            "CONNECT nas.local:443 HTTP/1.1",
            "CONNECT media.example.test:8443 HTTP/1.1",
            "CONNECT media.example.test:80 HTTP/1.1",
            "GET http://127.0.0.1/seg.ts HTTP/1.1",
            "GET http://0x7f.1/seg.ts HTTP/1.1",
            "GET http://router.home.arpa/ HTTP/1.1",
            "GET http://media.example.test:8080/seg.ts HTTP/1.1",
            "GET http://user:pw@media.example.test/seg.ts HTTP/1.1",
            "GET https://media.example.test/seg.ts HTTP/1.1",
            "GET ftp://media.example.test/seg.ts HTTP/1.1",
            "GET file:///C:/secret HTTP/1.1",
        ] {
            let answer = exchange(&guard, format!("{forbidden}\r\n\r\n").as_bytes());
            assert!(answer.starts_with("HTTP/1.1 403 "), "{forbidden}: {answer}");
        }
        for malformed in [
            &b"GARBAGE\r\n\r\n"[..],
            b"GET /seg.ts HTTP/1.1\r\nHost: media.example.test\r\n\r\n",
            b"CONNECT media.example.test HTTP/1.1\r\n\r\n",
            b"CONNECT media.example.test/x:443 HTTP/1.1\r\n\r\n",
            b"GET http://media.example.test/ HTTP/2\r\n\r\n",
            b"GET http://media.example.test/ HTTP/1.1\r\nno colon\r\n\r\n",
            b"POST http://media.example.test/ HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"POST http://media.example.test/ HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
            b"GET http://media.example.test/ HTTP/1.1\r\n\xff\xfe\r\n\r\n",
        ] {
            let answer = exchange(&guard, malformed);
            assert!(
                answer.starts_with("HTTP/1.1 400 "),
                "{}: {answer}",
                String::from_utf8_lossy(malformed)
            );
        }
        // Names that fail the policy are never looked up; nothing was connected.
        assert!(lookups.lock().unwrap().is_empty());
        assert!(connects.lock().unwrap().is_empty());
        // A name that cannot be resolved is a gateway error, not a refusal.
        let (policy, _, _) = fake_policy(|_, _| Vec::new(), echo());
        let guard = Guard::start(policy, LIMITS).unwrap();
        let answer = exchange(&guard, b"CONNECT gone.example.test:443 HTTP/1.1\r\n\r\n");
        assert!(answer.starts_with("HTTP/1.1 502 "), "{answer}");
        assert!(!guard.refused());
    }

    #[test]
    fn a_dns_answer_that_changes_after_the_check_is_never_used() {
        // Rebinding: the name answers a public address first and a private one afterwards. The
        // guard looks up once per request and connects to the address it checked.
        let (route, seen) = upstream(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        let (policy, lookups, connects) = fake_policy(
            |_, call| {
                vec![
                    if call == 1 { PUBLIC } else { "192.168.1.1" }
                        .parse()
                        .unwrap(),
                ]
            },
            route,
        );
        let guard = Guard::start(policy, LIMITS).unwrap();
        let request =
            b"GET http://rebind.example.test/a HTTP/1.1\r\nHost: rebind.example.test\r\n\r\n";
        assert!(exchange(&guard, request).starts_with("HTTP/1.1 200 "));
        assert!(seen.recv_timeout(Duration::from_secs(5)).is_ok());
        assert_eq!(lookups.lock().unwrap().len(), 1);
        assert_eq!(*connects.lock().unwrap(), [format!("{PUBLIC}:80")]);
        // The next request sees the private answer and is refused.
        assert!(exchange(&guard, request).starts_with("HTTP/1.1 403 "));
        assert_eq!(lookups.lock().unwrap().len(), 2);
        assert_eq!(connects.lock().unwrap().len(), 1);
        assert!(guard.refused());
    }

    #[test]
    fn heads_connections_and_slow_clients_are_bounded() {
        let limits = Limits {
            connections: 2,
            head_time: Duration::from_millis(300),
            ..LIMITS
        };
        let (policy, _, _) = fake_policy(public, echo());
        let guard = Guard::start(policy, limits).unwrap();
        // A head larger than the limit.
        let huge = format!(
            "GET http://media.example.test/ HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(LIMITS.head + 10)
        );
        let answer = exchange(&guard, huge.as_bytes());
        assert!(answer.starts_with("HTTP/1.1 400 "), "{:.200}", answer);
        // A client that sends nothing is closed after the head time.
        let started = Instant::now();
        let mut silent = TcpStream::connect(guard.address).unwrap();
        silent
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut answer = String::new();
        let _ = silent.read_to_string(&mut answer);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(answer.starts_with("HTTP/1.1 400 "), "{answer}");
        // Two slow connections fill the guard; a third is turned away at once.
        let limits = Limits {
            connections: 2,
            head_time: Duration::from_secs(5),
            ..LIMITS
        };
        let (policy, _, _) = fake_policy(public, echo());
        let guard = Guard::start(policy, limits).unwrap();
        let _held: Vec<_> = (0..2)
            .map(|_| TcpStream::connect(guard.address).unwrap())
            .collect();
        thread::sleep(Duration::from_millis(200));
        let started = Instant::now();
        let answer = exchange(&guard, b"");
        assert!(answer.starts_with("HTTP/1.1 503 "), "{answer}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_tunnel_stays_open_while_only_the_destination_sends() {
        // A site that sends a chunk every 100 ms for over a second to a client that sends
        // nothing, with an idle limit of 300 ms: the tunnel must carry all of it.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let route = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                for _ in 0..12 {
                    if stream.write_all(b"chunk").is_err() {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        });
        let limits = Limits {
            idle_time: Duration::from_millis(300),
            ..LIMITS
        };
        let (policy, _, _) = fake_policy(public, route);
        let guard = Guard::start(policy, limits).unwrap();
        let answer = exchange(&guard, b"CONNECT media.example.test:443 HTTP/1.1\r\n\r\n");
        let body = answer
            .strip_prefix("HTTP/1.1 200 Connection established\r\n\r\n")
            .unwrap_or_else(|| panic!("{answer}"));
        assert_eq!(body, "chunk".repeat(12));
        // With nothing moving at all, the tunnel closes after the idle limit.
        let (policy, _, _) = fake_policy(public, echo());
        let guard = Guard::start(policy, limits).unwrap();
        let started = Instant::now();
        let answer = exchange(&guard, b"CONNECT media.example.test:443 HTTP/1.1\r\n\r\n");
        assert!(answer.starts_with("HTTP/1.1 200 "), "{answer}");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn dropping_the_guard_closes_open_tunnels_and_stops_listening() {
        let (policy, _, _) = fake_policy(public, echo());
        let guard = Guard::start(policy, LIMITS).unwrap();
        let address = guard.address;
        let mut tunnel = TcpStream::connect(address).unwrap();
        tunnel
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        tunnel
            .write_all(b"CONNECT media.example.test:443 HTTP/1.1\r\n\r\n")
            .unwrap();
        let mut established = [0; 39];
        tunnel.read_exact(&mut established).unwrap();
        let idle = TcpStream::connect(address).unwrap();
        let started = Instant::now();
        drop(guard);
        assert!(started.elapsed() < Duration::from_secs(3));
        let mut rest = Vec::new();
        let _ = tunnel.read_to_end(&mut rest);
        assert!(rest.is_empty());
        drop(idle);
        assert!(TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_err());
    }

    #[test]
    fn only_loopback_clients_are_accepted() {
        let (policy, _, _) = fake_policy(public, echo());
        let guard = Guard::start(policy, LIMITS).unwrap();
        assert!(guard.address.ip().is_loopback());
        assert!(accepted_client("127.0.0.1:5000".parse().unwrap()));
        assert!(accepted_client("[::1]:5000".parse().unwrap()));
        for remote in [
            "192.168.1.20:5000",
            "10.0.0.2:1",
            "[fe80::1]:2",
            "8.8.8.8:3",
        ] {
            assert!(!accepted_client(remote.parse().unwrap()), "{remote}");
        }
    }

    #[test]
    fn a_proxy_is_detected_the_way_yt_dlp_finds_one() {
        assert!(!proxy_in_environment(|_| None));
        assert!(!proxy_in_environment(|_| Some("  ".into())));
        for name in ["HTTPS_PROXY", "http_proxy", "ALL_PROXY"] {
            assert!(proxy_in_environment(
                |n| (n == name).then(|| "http://proxy.example.test:3128".into())
            ));
        }
        // NO_PROXY alone sets no proxy.
        assert!(!proxy_in_environment(
            |n| (n == "NO_PROXY").then(|| "*".into())
        ));
        let settings = |enable: &str, server: Option<&str>| {
            let mut out = format!(
                "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings\r\n    ProxyEnable    REG_DWORD    {enable}\r\n"
            );
            if let Some(server) = server {
                out.push_str(&format!("    ProxyServer    REG_SZ    {server}\r\n"));
            }
            out
        };
        assert!(registry_proxy(&settings(
            "0x1",
            Some("proxy.example.test:3128")
        )));
        assert!(registry_proxy(&settings(
            "0x1",
            Some("http=proxy.example.test:3128;https=proxy.example.test:3129")
        )));
        assert!(!registry_proxy(&settings(
            "0x0",
            Some("proxy.example.test:3128")
        )));
        assert!(!registry_proxy(&settings("0x1", None)));
        assert!(!registry_proxy(""));
        assert!(scutil_proxy(
            "<dictionary> {\n  HTTPEnable : 0\n  HTTPSEnable : 1\n}"
        ));
        assert!(!scutil_proxy(
            "<dictionary> {\n  HTTPEnable : 0\n  SOCKSEnable : 1\n}"
        ));
    }
}
