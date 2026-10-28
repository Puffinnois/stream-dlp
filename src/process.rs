//! Runs the bundled helpers (yt-dlp, FFmpeg, FFprobe, Deno) and the upload browser: no
//! console window, cancellation, a time limit, bounded output, and stopping the whole
//! process tree.
use std::{
    io::{self, Read},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(25);

/// Starts `cmd` without a console window and, on Unix, in its own process group, so
/// [`stop`] also ends the programs it starts (yt-dlp runs FFmpeg).
pub fn isolate(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd
}

/// Stops `child` and everything it started, then reaps it.
pub fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        // SAFETY: isolate() assigns the spawned helper a process group equal to
        // its PID. This targets that group, never our application's group.
        unsafe {
            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ =
            isolate(Command::new("taskkill").args(["/PID", &child.id().to_string(), "/T", "/F"]))
                .output();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// A started helper that is stopped, with its children, when dropped.
pub struct Owned(pub Child);

impl Drop for Owned {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            stop(&mut self.0);
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// The helper could not be started.
    Start(io::Error),
    Cancelled,
    TimedOut,
    /// A stream printed more than the output limit.
    TooMuchOutput,
    /// Waiting for the helper failed.
    Wait(io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start(error) => write!(f, "The helper could not start: {error}"),
            Self::Cancelled => f.write_str("Cancelled."),
            Self::TimedOut => f.write_str("The helper took too long."),
            Self::TooMuchOutput => f.write_str("The helper printed too much output."),
            Self::Wait(error) => write!(f, "{error}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// One run of a helper. Stdin is closed; stdout and stderr are captured unless stderr is
/// ignored. Cancellation, the time limit and the output limit stop the process tree.
pub struct Helper<'a> {
    command: &'a mut Command,
    cancel: &'a AtomicBool,
    time: Option<Duration>,
    output: Option<u64>,
    stderr: bool,
}

impl<'a> Helper<'a> {
    pub fn new(command: &'a mut Command, cancel: &'a AtomicBool) -> Self {
        Self {
            command,
            cancel,
            time: None,
            output: None,
            stderr: true,
        }
    }

    pub fn time_limit(mut self, limit: Duration) -> Self {
        self.time = Some(limit);
        self
    }

    /// The most bytes each stream may print.
    pub fn output_limit(mut self, bytes: u64) -> Self {
        self.output = Some(bytes);
        self
    }

    pub fn ignore_stderr(mut self) -> Self {
        self.stderr = false;
        self
    }

    /// Runs to the end and returns everything the helper printed.
    pub fn output(self) -> Result<Output, Error> {
        let (status, [stdout, stderr]) = self.run(None)?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    /// Runs to the end, passing each line to `on_line` on this thread as it arrives.
    pub fn lines(self, mut on_line: impl FnMut(Stream, &str)) -> Result<ExitStatus, Error> {
        self.run(Some(&mut on_line)).map(|(status, _)| status)
    }

    fn run(self, mut on_line: OnLine<'_>) -> Result<(ExitStatus, [Vec<u8>; 2]), Error> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let mut child = Owned(
            isolate(self.command)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(if self.stderr {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .spawn()
                .map_err(Error::Start)?,
        );
        let (tx, rx) = mpsc::channel();
        let mut readers = vec![read(Stream::Stdout, child.0.stdout.take(), tx.clone())];
        readers.push(read(Stream::Stderr, child.0.stderr.take(), tx));
        let mut captured = [Captured::default(), Captured::default()];
        let mut take = |(stream, chunk): (Stream, Vec<u8>), on_line: &mut OnLine<'_>| {
            captured[stream as usize].push(stream, &chunk, self.output, on_line)
        };
        let start = Instant::now();
        let result = loop {
            if self.cancel.load(Ordering::Relaxed) {
                break Err(Error::Cancelled);
            }
            if self.time.is_some_and(|limit| start.elapsed() > limit) {
                break Err(Error::TimedOut);
            }
            match rx.recv_timeout(POLL) {
                Ok(chunk) => {
                    if let Err(error) = take(chunk, &mut on_line) {
                        break Err(error);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => thread::sleep(POLL),
            }
            match child.0.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {}
                Err(error) => break Err(Error::Wait(error)),
            }
        };
        // A stopped tree closes its pipes, so the readers finish.
        drop(child);
        for reader in readers {
            let _ = reader.join();
        }
        let status = result?;
        for chunk in rx.try_iter() {
            take(chunk, &mut on_line)?;
        }
        for (stream, captured) in [Stream::Stdout, Stream::Stderr]
            .into_iter()
            .zip(&mut captured)
        {
            captured.finish(stream, &mut on_line);
        }
        let [stdout, stderr] = captured;
        Ok((status, [stdout.bytes, stderr.bytes]))
    }
}

fn read(
    stream: Stream,
    source: Option<impl Read + Send + 'static>,
    tx: mpsc::Sender<(Stream, Vec<u8>)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Some(mut source) = source else {
            return;
        };
        let mut buffer = [0; 8192];
        loop {
            match source.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send((stream, buffer[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    })
}

type OnLine<'f> = Option<&'f mut dyn FnMut(Stream, &str)>;

/// One stream's output: all of it, or in line mode only the unfinished last line.
#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    total: u64,
}

impl Captured {
    fn push(
        &mut self,
        stream: Stream,
        chunk: &[u8],
        limit: Option<u64>,
        on_line: &mut OnLine<'_>,
    ) -> Result<(), Error> {
        self.total += chunk.len() as u64;
        if limit.is_some_and(|limit| self.total > limit) {
            return Err(Error::TooMuchOutput);
        }
        self.bytes.extend_from_slice(chunk);
        if let Some(on_line) = on_line {
            let mut start = 0;
            while let Some(end) = self.bytes[start..].iter().position(|b| *b == b'\n') {
                emit(stream, &self.bytes[start..start + end], on_line);
                start += end + 1;
            }
            self.bytes.drain(..start);
        }
        Ok(())
    }

    fn finish(&mut self, stream: Stream, on_line: &mut OnLine<'_>) {
        if let Some(on_line) = on_line
            && !self.bytes.is_empty()
        {
            emit(stream, &self.bytes, on_line);
            self.bytes.clear();
        }
    }
}

fn emit(stream: Stream, line: &[u8], on_line: &mut dyn FnMut(Stream, &str)) {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    on_line(stream, &String::from_utf8_lossy(line));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut command = Command::new("cmd.exe");
            command.args(["/D", "/C"]).raw_arg(script);
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            command
        }
    }

    const PRINT: &str = if cfg!(windows) {
        "echo one& echo two& echo three 1>&2"
    } else {
        "echo one; echo two; echo three 1>&2"
    };
    const WAIT: &str = if cfg!(windows) {
        "ping -n 6 127.0.0.1 >nul"
    } else {
        "sleep 5"
    };

    #[test]
    fn output_and_lines_keep_each_stream() {
        let cancel = AtomicBool::new(false);
        let output = Helper::new(&mut shell(PRINT), &cancel).output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "one\ntwo\n"
        );
        assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "three");
        let mut lines = Vec::new();
        Helper::new(&mut shell(PRINT), &cancel)
            .lines(|stream, line| lines.push((stream, line.trim_end().to_owned())))
            .unwrap();
        lines.sort_by_key(|(stream, _)| *stream as u8);
        assert_eq!(
            lines,
            [
                (Stream::Stdout, "one".into()),
                (Stream::Stdout, "two".into()),
                (Stream::Stderr, "three".into()),
            ]
        );
        let ignored = Helper::new(&mut shell(PRINT), &cancel)
            .ignore_stderr()
            .output()
            .unwrap();
        assert!(ignored.stderr.is_empty());
    }

    #[test]
    fn limits_and_cancellation_stop_the_helper() {
        let cancel = AtomicBool::new(false);
        assert!(matches!(
            Helper::new(&mut shell(PRINT), &cancel)
                .output_limit(4)
                .output(),
            Err(Error::TooMuchOutput)
        ));
        let start = Instant::now();
        assert!(matches!(
            Helper::new(&mut shell(WAIT), &cancel)
                .time_limit(Duration::from_millis(200))
                .output(),
            Err(Error::TimedOut)
        ));
        assert!(start.elapsed() < Duration::from_secs(4));
        assert!(matches!(
            Helper::new(&mut Command::new("stream-dlp-missing-helper"), &cancel).output(),
            Err(Error::Start(_))
        ));
        cancel.store(true, Ordering::Relaxed);
        assert!(matches!(
            Helper::new(&mut Command::new("stream-dlp-missing-helper"), &cancel).output(),
            Err(Error::Cancelled)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_stops_descendants_in_the_helper_process_group() {
        let directory = tempfile::tempdir().unwrap();
        let survived = directory.path().join("survived");
        let ready = directory.path().join("ready");
        let mut child = isolate(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "(sleep 1; printf survived > \"$1\") & printf ready > \"$2\"; wait",
                    "test",
                ])
                .arg(&survived)
                .arg(&ready),
        )
        .spawn()
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let started = ready.exists();
        stop(&mut child);
        assert!(started, "The synthetic helper did not start");
        thread::sleep(Duration::from_millis(1200));
        assert!(!survived.exists(), "A descendant survived cancellation");
    }
}
