//! Host mode: the browser starts this executable with the extension's origin as the first
//! argument (`chrome-extension://<id>/`, plus `--parent-window=<n>` on Windows) and talks to it
//! over standard input and output. The host opens no window and writes no file or log: it
//! checks the caller, connects to the running app (`channel`), and relays well-formed messages
//! both ways until either side closes. Chrome passes no other arguments, so the origin itself
//! selects host mode (`invoked`).
use super::{
    channel,
    framing::{self, FrameError},
    messages::{FromExtension, State, ToExtension},
    origin,
};
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    net::{Shutdown, TcpStream},
    sync::{Arc, Mutex, mpsc},
};

/// Exit codes, for the browser's log.
const REFUSED: i32 = 2;
const PROTOCOL_ERROR: i32 = 1;

/// Whether a browser started this executable as a native messaging host.
pub fn invoked(args: &[OsString]) -> bool {
    args.get(1)
        .and_then(|arg| arg.to_str())
        .is_some_and(|arg| arg.starts_with("chrome-extension://"))
}

/// Whether the caller is exactly our extension, with only the arguments Chrome documents.
pub fn caller_allowed(args: &[OsString]) -> bool {
    let mut rest = args.iter().skip(1).map(|arg| arg.to_str());
    if rest.next() != Some(Some(origin().as_str())) {
        return false;
    }
    rest.all(|arg| {
        arg.and_then(|arg| arg.strip_prefix("--parent-window="))
            .is_some_and(|handle| !handle.is_empty() && handle.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Runs host mode and returns the process's exit code.
pub fn run(args: &[OsString]) -> i32 {
    if !caller_allowed(args) {
        return REFUSED;
    }
    let app = match super::folder() {
        Some(folder) => channel::connect(&folder),
        None => Err(io::ErrorKind::NotFound.into()),
    };
    relay(io::stdin(), io::stdout(), app)
}

type Shared<W> = Arc<Mutex<W>>;

fn send<W: Write>(output: &Shared<W>, message: &ToExtension) -> io::Result<()> {
    framing::write_message(
        &mut *output.lock().unwrap_or_else(|e| e.into_inner()),
        message,
    )
}

/// Relays between the browser (`input`, `output`) and the app until either side closes.
pub fn relay<R, W>(mut input: R, output: W, app: io::Result<TcpStream>) -> i32
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let output = Arc::new(Mutex::new(output));
    let Ok(app) = app else {
        let _ = send(
            &output,
            &ToExtension::Status {
                state: State::AppUnavailable,
            },
        );
        // Wait for the extension to close the port, so it reads the status first.
        loop {
            match framing::read_message::<FromExtension>(&mut input) {
                Ok(_) => {}
                Err(FrameError::Closed) => return 0,
                Err(_) => return PROTOCOL_ERROR,
            }
        }
    };
    let (mut from_app, mut to_app) = match app.try_clone() {
        Ok(reader) => (reader, app),
        Err(_) => return PROTOCOL_ERROR,
    };
    let (done, finished) = mpsc::channel();

    let browser = output.clone();
    let app_done = done.clone();
    std::thread::spawn(move || {
        let mut disconnected = false;
        while let Ok(message) = framing::read_message::<ToExtension>(&mut from_app) {
            disconnected = message == ToExtension::Disconnect;
            if send(&browser, &message).is_err() {
                break;
            }
        }
        if !disconnected {
            // The app quit or refused us: tell the extension instead of vanishing.
            let _ = send(
                &browser,
                &ToExtension::Status {
                    state: State::AppUnavailable,
                },
            );
        }
        let _ = app_done.send(0);
    });

    std::thread::spawn(move || {
        let code = loop {
            match framing::read_message::<FromExtension>(&mut input) {
                Ok(message) => {
                    if framing::write_message(&mut to_app, &message).is_err() {
                        break 0;
                    }
                }
                Err(FrameError::Closed) => break 0,
                Err(_) => break PROTOCOL_ERROR,
            }
        };
        let _ = to_app.shutdown(Shutdown::Both);
        let _ = done.send(code);
    });

    finished.recv().unwrap_or(PROTOCOL_ERROR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::chrome::{
        channel::{
            Event,
            tests::{listener, next},
        },
        framing::frame,
    };
    use std::time::Duration;

    fn args(list: &[&str]) -> Vec<OsString> {
        std::iter::once("stream-dlp.exe")
            .chain(list.iter().copied())
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn only_our_exact_origin_starts_the_host() {
        let ours = origin();
        assert!(invoked(&args(&[&ours])));
        assert!(caller_allowed(&args(&[&ours])));
        assert!(caller_allowed(&args(&[&ours, "--parent-window=0"])));
        assert!(caller_allowed(&args(&[&ours, "--parent-window=1234"])));
        for refused in [
            vec!["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"],
            vec!["chrome-extension://edngekgkkjhaoohkfddejaoohfaggiac"],
            vec!["chrome-extension://edngekgkkjhaoohkfddejaoohfaggiac/x"],
            vec!["CHROME-EXTENSION://edngekgkkjhaoohkfddejaoohfaggiac/"],
            vec![&ours, "--parent-window="],
            vec![&ours, "--parent-window=-1"],
            vec![&ours, "--dev-mode"],
            vec!["--parent-window=0", &ours],
        ] {
            assert!(!caller_allowed(&args(&refused)), "{refused:?}");
        }
        assert!(!invoked(&args(&[])));
        assert!(!invoked(&args(&["--dev-mode"])));
        assert!(!invoked(&args(&["--check-tools"])));
    }

    /// A reader fed chunk by chunk from another thread; ends when the sender is dropped.
    struct Pipe {
        rx: mpsc::Receiver<Vec<u8>>,
        buffer: Vec<u8>,
        position: usize,
    }

    impl Read for Pipe {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.position == self.buffer.len() {
                match self.rx.recv_timeout(Duration::from_secs(10)) {
                    Ok(chunk) => (self.buffer, self.position) = (chunk, 0),
                    Err(_) => return Ok(0),
                }
            }
            let n = out.len().min(self.buffer.len() - self.position);
            out[..n].copy_from_slice(&self.buffer[self.position..self.position + n]);
            self.position += n;
            Ok(n)
        }
    }

    struct PipeWriter(mpsc::Sender<Vec<u8>>);

    impl Write for PipeWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let _ = self.0.send(bytes.to_vec());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn pipe() -> (mpsc::Sender<Vec<u8>>, Pipe) {
        let (tx, rx) = mpsc::channel();
        (
            tx,
            Pipe {
                rx,
                buffer: Vec::new(),
                position: 0,
            },
        )
    }

    #[test]
    fn without_the_app_the_extension_is_told_and_the_host_exits_on_close() {
        let (stdin_tx, stdin) = pipe();
        let (stdout_tx, mut stdout) = pipe();
        let thread = std::thread::spawn(move || {
            relay(
                stdin,
                PipeWriter(stdout_tx),
                Err(io::ErrorKind::NotFound.into()),
            )
        });
        let status: ToExtension = framing::read_message(&mut stdout).unwrap();
        assert_eq!(
            status,
            ToExtension::Status {
                state: State::AppUnavailable
            }
        );
        stdin_tx
            .send(frame(br#"{"type":"hello","protocol":1}"#))
            .unwrap();
        drop(stdin_tx);
        assert_eq!(thread.join().unwrap(), 0);
    }

    #[test]
    fn messages_are_relayed_both_ways_and_invalid_ones_end_the_host() {
        let folder = tempfile::tempdir().unwrap();
        let (app, events) = listener(folder.path());
        let (stdin_tx, stdin) = pipe();
        let (stdout_tx, mut stdout) = pipe();
        let connection = channel::connect(folder.path());
        let thread = std::thread::spawn(move || relay(stdin, PipeWriter(stdout_tx), connection));
        let Event::Connected(id) = next(&events) else {
            panic!("not connected")
        };
        stdin_tx
            .send(frame(br#"{"type":"hello","protocol":1}"#))
            .unwrap();
        assert!(matches!(
            next(&events),
            Event::Message(_, FromExtension::Hello { protocol: 1 })
        ));
        assert!(app.send(id, &ToExtension::Refresh));
        let message: ToExtension = framing::read_message(&mut stdout).unwrap();
        assert_eq!(message, ToExtension::Refresh);
        // A message the app would refuse never reaches it: the host stops first.
        stdin_tx
            .send(frame(br#"{"type":"hello","protocol":1,"extra":1}"#))
            .unwrap();
        assert_eq!(thread.join().unwrap(), PROTOCOL_ERROR);
        assert!(matches!(next(&events), Event::Closed(got) if got == id));
    }

    #[test]
    fn the_host_tells_the_extension_when_the_app_goes_away() {
        let folder = tempfile::tempdir().unwrap();
        let (app, events) = listener(folder.path());
        let (_stdin_tx, stdin) = pipe();
        let (stdout_tx, mut stdout) = pipe();
        let connection = channel::connect(folder.path());
        let thread = std::thread::spawn(move || relay(stdin, PipeWriter(stdout_tx), connection));
        let Event::Connected(id) = next(&events) else {
            panic!("not connected")
        };
        app.close(id);
        let status: ToExtension = framing::read_message(&mut stdout).unwrap();
        assert_eq!(
            status,
            ToExtension::Status {
                state: State::AppUnavailable
            }
        );
        assert_eq!(thread.join().unwrap(), 0);
    }

    /// The executable `cargo build --bin stream-dlp` makes next to this test binary, or
    /// `STREAM_DLP_HOST_EXE` (for example a release build, to check the window subsystem).
    fn built_app() -> std::path::PathBuf {
        if let Some(exe) = std::env::var_os("STREAM_DLP_HOST_EXE") {
            return exe.into();
        }
        let test = std::env::current_exe().unwrap();
        let exe = test
            .parent()
            .and_then(|deps| deps.parent())
            .unwrap()
            .join(format!("stream-dlp{}", std::env::consts::EXE_SUFFIX));
        assert!(
            exe.is_file(),
            "Build the app first: cargo build --locked --bin stream-dlp"
        );
        exe
    }

    /// The built app in host mode, started directly or through `cmd /d /c` as Chrome and
    /// Edge do by default, with the channel folder set to `folder`.
    struct Hosted {
        child: std::process::Child,
        stdin: Option<std::process::ChildStdin>,
        stdout: mpsc::Receiver<Result<ToExtension, FrameError>>,
    }

    impl Hosted {
        fn start(origin: &str, folder: &std::path::Path, through_cmd: bool) -> Self {
            use std::process::{Command, Stdio};
            let exe = built_app();
            let mut command = if through_cmd {
                let mut command = Command::new("cmd");
                command.arg("/d").arg("/c").arg(&exe);
                command
            } else {
                Command::new(&exe)
            };
            let mut child = command
                .arg(origin)
                .arg("--parent-window=0")
                .env(crate::session::chrome::FOLDER_VARIABLE, folder)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut out = child.stdout.take().unwrap();
            let (tx, stdout) = mpsc::channel();
            std::thread::spawn(move || {
                loop {
                    let message = framing::read_message::<ToExtension>(&mut out);
                    let end = message.is_err();
                    let _ = tx.send(message);
                    if end {
                        break;
                    }
                }
            });
            Self {
                stdin: child.stdin.take(),
                child,
                stdout,
            }
        }

        fn send(&mut self, json: &str) {
            self.stdin
                .as_mut()
                .unwrap()
                .write_all(&frame(json.as_bytes()))
                .unwrap();
        }

        fn receive(&self) -> Result<ToExtension, FrameError> {
            self.stdout
                .recv_timeout(Duration::from_secs(10))
                .expect("host output")
        }

        /// Closes stdin like a browser closing the port and returns the exit code.
        fn finish(mut self) -> i32 {
            drop(self.stdin.take());
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return status.code().unwrap_or(-1);
                }
                if std::time::Instant::now() > deadline {
                    let _ = self.child.kill();
                    panic!("The host did not exit after its input closed");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    #[test]
    #[ignore = "runs the built app in host mode; cargo xtask pipeline builds it first"]
    fn built_app_host_mode_relays_framed_messages() {
        let launches: &[bool] = if cfg!(windows) {
            &[false, true]
        } else {
            &[false]
        };
        for &through_cmd in launches {
            // Another extension's origin: refused without a byte of output.
            let empty = tempfile::tempdir().unwrap();
            let wrong = Hosted::start(
                "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
                empty.path(),
                through_cmd,
            );
            assert_eq!(wrong.receive(), Err(FrameError::Closed));
            assert_eq!(wrong.finish(), REFUSED);

            // The app is not listening: the extension is told, and the host exits on close.
            let mut alone = Hosted::start(&origin(), empty.path(), through_cmd);
            alone.send(r#"{"type":"hello","protocol":1}"#);
            assert_eq!(
                alone.receive(),
                Ok(ToExtension::Status {
                    state: State::AppUnavailable
                })
            );
            assert_eq!(alone.finish(), 0);

            // The app is listening: both directions, then a clean exit.
            let folder = tempfile::tempdir().unwrap();
            let (app, events) = listener(folder.path());
            let mut host = Hosted::start(&origin(), folder.path(), through_cmd);
            let Event::Connected(id) = next(&events) else {
                panic!("not connected")
            };
            host.send(r#"{"type":"hello","protocol":1}"#);
            assert!(matches!(
                next(&events),
                Event::Message(_, FromExtension::Hello { protocol: 1 })
            ));
            assert!(app.send(
                id,
                &ToExtension::Status {
                    state: State::Connected
                }
            ));
            assert_eq!(
                host.receive(),
                Ok(ToExtension::Status {
                    state: State::Connected
                })
            );
            // Padded so its length prefix is 0A 0A 00 00: text-mode I/O would turn each 0A
            // into 0D 0A and break the frame.
            let session = format!(
                "{:<2570}",
                r#"{"type":"session","cookies":[{"domain":".youtube.com","hostOnly":false,"path":"/","secure":true,"httpOnly":true,"expirationDate":4102444800,"name":"TEST","value":"fake"}]}"#
            );
            assert_eq!(session.len(), 0x0a0a);
            host.send(&session);
            let Event::Message(_, FromExtension::Session { cookies }) = next(&events) else {
                panic!("no session")
            };
            assert_eq!(cookies.len(), 1);
            assert!(app.send(id, &ToExtension::Refresh));
            assert_eq!(host.receive(), Ok(ToExtension::Refresh));
            assert_eq!(host.finish(), 0);
            assert!(matches!(next(&events), Event::Closed(got) if got == id));
        }
    }
}
