//! HTTPS for update checks and downloads. Requests use the system's certificates and proxy, as
//! the PowerShell calls they replace did, and fail when a connection stalls rather than after
//! a fixed total, so a large download on a slow connection still finishes.
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use ureq::tls::{RootCerts, TlsConfig};

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .user_agent(concat!("stream-dlp/", env!("CARGO_PKG_VERSION")))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .build()
            .new_agent()
    })
}

fn describe(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(403 | 429) => {
            "GitHub is limiting requests. Try again later.".into()
        }
        ureq::Error::StatusCode(status) => format!("The server answered HTTP {status}."),
        error => error.to_string(),
    }
}

/// A GitHub API document (`path` follows `https://api.github.com/`), or `None` when the API
/// answers 404, as `releases/latest` does for a repository without releases.
pub fn github(path: &str) -> Result<Option<String>, String> {
    let url = format!("https://api.github.com/{path}");
    match agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(mut response) => response
            .body_mut()
            .read_to_string()
            .map(Some)
            .map_err(describe),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(error) => Err(describe(error)),
    }
}

/// A small file (up to 10 MB), such as a license notice.
pub fn get(url: &str) -> Result<Vec<u8>, String> {
    agent()
        .get(url)
        .call()
        .and_then(|mut response| response.body_mut().read_to_vec())
        .map_err(describe)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads `url` to `path`, reporting bytes received and expected about ten times a second.
/// The file is removed again unless the download completes with the SHA-256 `sha256` (hex).
pub fn download(
    url: &str,
    sha256: &str,
    path: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let response = agent().get(url).call().map_err(describe)?;
    let total = response.body().content_length();
    let mut body = response.into_body().into_reader();
    let mut save = || -> Result<(), String> {
        let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        let (mut received, mut shown) = (0, Instant::now());
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled.".into());
            }
            let read = body.read(&mut buffer).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
            received += read as u64;
            if shown.elapsed() >= Duration::from_millis(100) {
                progress(received, total);
                shown = Instant::now();
            }
        }
        progress(received, total);
        file.flush().map_err(|e| e.to_string())?;
        if !hex(&hasher.finalize()).eq_ignore_ascii_case(sha256) {
            return Err("The download does not match its SHA-256 checksum.".into());
        }
        Ok(())
    };
    let result = save();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Serves `body` once on localhost and returns its URL.
    fn serve(body: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let _ = stream.read(&mut [0; 4096]).unwrap();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        (url, server)
    }

    #[test]
    fn download_verifies_bytes_and_removes_a_mismatch() {
        let hello = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        for (sha256, valid) in [(hello.to_uppercase(), true), ("0".repeat(64), false)] {
            let (url, server) = serve(b"hello world");
            let mut last = None;
            let result = download(&url, &sha256, &path, &AtomicBool::new(false), |r, t| {
                last = Some((r, t))
            });
            server.join().unwrap();
            if valid {
                result.unwrap();
                assert_eq!(std::fs::read(&path).unwrap(), b"hello world");
                assert_eq!(last, Some((11, Some(11))));
            } else {
                assert!(result.unwrap_err().contains("SHA-256"));
                assert!(!path.exists());
            }
        }
    }

    #[test]
    fn cancelled_download_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        let (url, server) = serve(b"hello world");
        let result = download(&url, "", &path, &AtomicBool::new(true), |_, _| {});
        server.join().unwrap();
        assert_eq!(result.unwrap_err(), "Cancelled.");
        assert!(!path.exists());
    }
}
