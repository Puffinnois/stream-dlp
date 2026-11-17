//! Sound for the in-app player: FFmpeg decodes a file's first audio stream to PCM from a chosen
//! moment and a worker thread feeds it to the system's audio output, whose count of played
//! samples is the player's clock. Windows only so far; elsewhere [`Audio::start`] gives `None`
//! and the player runs silent on its own clock.
use crate::process;
use std::{
    io::Read,
    path::Path,
    process::Command,
    sync::{
        Arc, Mutex, Once, PoisonError,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
/// Samples per buffer: 40 ms. Volume applies as a buffer is queued, so the queue stays short.
const BUFFER: usize = RATE as usize / 25 * CHANNELS;
const BUFFERS: usize = 3;
const TICK: Duration = Duration::from_millis(5);

/// Whether this platform has a sound output here.
pub const AVAILABLE: bool = output::AVAILABLE;

/// Opens and closes the output once, in the background. The first opening in a process takes
/// about half a second, which would otherwise hold the first video on its first frame.
pub fn warm_up() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(|| {
            let _output = output::Output::open(RATE, CHANNELS, BUFFER, 1);
        });
    });
}

struct Position {
    /// Seconds of sound played since the start.
    played: f64,
    at: Instant,
    running: bool,
}

struct Shared {
    running: process::Running,
    playing: AtomicBool,
    /// The gain, as `f32` bits.
    gain: AtomicU32,
    position: Mutex<Position>,
    ended: AtomicBool,
}

/// One run of sound from a moment of the video to its end, paused until told to play.
/// Dropping it stops the sound at once, so seeking and switching videos start a new one.
pub struct Audio {
    shared: Arc<Shared>,
    start: f64,
}

impl Audio {
    /// Starts decoding at `seconds`. Nothing here waits. `None` where there is no output.
    pub fn start(path: &Path, seconds: f64, gain: f32) -> Option<Self> {
        if !AVAILABLE {
            return None;
        }
        let shared = Arc::new(Shared {
            running: process::Running::default(),
            playing: AtomicBool::new(false),
            gain: AtomicU32::new(gain.to_bits()),
            position: Mutex::new(Position {
                played: 0.,
                at: Instant::now(),
                running: false,
            }),
            ended: AtomicBool::new(false),
        });
        let (path, worker) = (path.to_owned(), shared.clone());
        std::thread::spawn(move || {
            play(&path, seconds, &worker);
            worker.ended.store(true, Ordering::Relaxed);
            worker.running.stop();
        });
        Some(Self {
            shared,
            start: seconds,
        })
    }

    pub fn set_playing(&self, playing: bool) {
        self.shared.playing.store(playing, Ordering::Relaxed);
    }

    /// 0 is silent and 1 is the file's own level.
    pub fn set_gain(&self, gain: f32) {
        self.shared.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    /// The moment of the video now heard, while there is sound to follow: `None` once the
    /// sound ended, the file has none, or the output could not be opened.
    pub fn position(&self) -> Option<f64> {
        if self.shared.ended.load(Ordering::Relaxed) {
            return None;
        }
        let position = self
            .shared
            .position
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // The worker reports every few milliseconds; between reports the sound plays on.
        let since = if position.running {
            position.at.elapsed().as_secs_f64().min(0.05)
        } else {
            0.
        };
        Some(self.start + position.played + since)
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.shared.running.stop();
    }
}

fn command(path: &Path, seconds: f64) -> Command {
    let mut command = Command::new(crate::tools::path("ffmpeg"));
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-protocol_whitelist",
        "file",
    ]);
    if seconds > 0. {
        command.args(["-ss", &format!("{seconds:.3}")]);
    }
    command.arg("-i").arg(path).args([
        "-map",
        "0:a:0",
        "-vn",
        "-sn",
        "-dn",
        "-ac",
        &CHANNELS.to_string(),
        "-ar",
        &RATE.to_string(),
        "-f",
        "s16le",
        "pipe:1",
    ]);
    command
}

/// Reads until `bytes` is full or the source ends; returns how much was read.
fn fill(source: &mut impl Read, bytes: &mut [u8]) -> usize {
    let mut filled = 0;
    while filled < bytes.len() {
        match source.read(&mut bytes[filled..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => filled += n,
        }
    }
    filled
}

/// Little-endian PCM bytes to samples at `gain`.
fn samples(bytes: &[u8], gain: f32, out: &mut [i16]) {
    for (pair, sample) in bytes.chunks_exact(2).zip(out) {
        let value = f32::from(i16::from_le_bytes([pair[0], pair[1]])) * gain;
        *sample = value.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
    }
}

fn play(path: &Path, seconds: f64, shared: &Shared) {
    // A file without sound ends here: FFmpeg has nothing to map and prints nothing.
    let Ok(mut source) = shared.running.start(&mut command(path, seconds)) else {
        return;
    };
    let Some(mut output) = output::Output::open(RATE, CHANNELS, BUFFER, BUFFERS) else {
        return;
    };
    let mut bytes = vec![0; BUFFER * 2];
    let mut playing = false;
    let mut more = true;
    let mut heard = false;
    while !shared.running.stopped() {
        let wanted = shared.playing.load(Ordering::Relaxed);
        if wanted != playing {
            output.set_playing(wanted);
            playing = wanted;
        }
        while more && let Some(buffer) = output.free() {
            let read = fill(&mut source, &mut bytes) / 2 / CHANNELS * CHANNELS;
            more = read == BUFFER;
            if read == 0 {
                break;
            }
            let gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));
            samples(&bytes[..read * 2], gain, output.samples(buffer));
            output.queue(buffer, read);
            heard = true;
        }
        if !more && (!heard || output.idle()) {
            break;
        }
        *shared
            .position
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Position {
            played: output.played() as f64 / f64::from(RATE),
            at: Instant::now(),
            running: playing && !output.idle(),
        };
        std::thread::sleep(TICK);
    }
}

/// The Windows wave output (`winmm`), which mixes into the default playback device.
#[cfg(windows)]
mod output {
    use std::ffi::c_void;

    pub const AVAILABLE: bool = true;

    type Handle = *mut c_void;
    const WAVE_MAPPER: u32 = u32::MAX;
    const WAVE_FORMAT_PCM: u16 = 1;
    const WHDR_DONE: u32 = 1;
    const TIME_MS: u32 = 1;
    const TIME_SAMPLES: u32 = 2;
    const TIME_BYTES: u32 = 4;

    /// `WAVEFORMATEX`; `mmsystem.h` packs its structures.
    #[repr(C, packed)]
    struct Format {
        tag: u16,
        channels: u16,
        samples_per_second: u32,
        bytes_per_second: u32,
        block_align: u16,
        bits_per_sample: u16,
        extra: u16,
    }

    /// `WAVEHDR`, whose natural layout has no padding to pack away.
    #[repr(C)]
    struct Header {
        data: *mut u8,
        length: u32,
        recorded: u32,
        user: usize,
        flags: u32,
        loops: u32,
        next: *mut Header,
        reserved: usize,
    }

    /// `MMTIME`: a kind and a union whose first member is the 32-bit count asked for.
    #[repr(C)]
    struct Time {
        kind: u32,
        value: u32,
        rest: u32,
    }

    #[link(name = "winmm")]
    unsafe extern "system" {
        fn waveOutOpen(
            handle: *mut Handle,
            device: u32,
            format: *const Format,
            callback: usize,
            instance: usize,
            flags: u32,
        ) -> u32;
        fn waveOutPrepareHeader(handle: Handle, header: *mut Header, size: u32) -> u32;
        fn waveOutUnprepareHeader(handle: Handle, header: *mut Header, size: u32) -> u32;
        fn waveOutWrite(handle: Handle, header: *mut Header, size: u32) -> u32;
        fn waveOutPause(handle: Handle) -> u32;
        fn waveOutRestart(handle: Handle) -> u32;
        fn waveOutReset(handle: Handle) -> u32;
        fn waveOutClose(handle: Handle) -> u32;
        fn waveOutGetPosition(handle: Handle, time: *mut Time, size: u32) -> u32;
    }

    const HEADER_SIZE: u32 = size_of::<Header>() as u32;

    struct Buffer {
        // Boxed: the system keeps these addresses while a buffer is queued.
        header: Box<Header>,
        samples: Box<[i16]>,
        queued: bool,
    }

    pub struct Output {
        handle: Handle,
        buffers: Vec<Buffer>,
        channels: usize,
        rate: u32,
    }

    impl Output {
        /// Opens the default device, paused, with `count` buffers of `length` samples.
        pub fn open(rate: u32, channels: usize, length: usize, count: usize) -> Option<Self> {
            let block = (channels * 2) as u16;
            let format = Format {
                tag: WAVE_FORMAT_PCM,
                channels: channels as u16,
                samples_per_second: rate,
                bytes_per_second: rate * u32::from(block),
                block_align: block,
                bits_per_sample: 16,
                extra: 0,
            };
            let mut handle: Handle = std::ptr::null_mut();
            // SAFETY: `handle` and `format` are valid for the call, which copies the format;
            // no callback is registered.
            if unsafe { waveOutOpen(&mut handle, WAVE_MAPPER, &format, 0, 0, 0) } != 0 {
                return None;
            }
            // SAFETY: `handle` was just opened.
            unsafe { waveOutPause(handle) };
            let buffers = (0..count)
                .map(|_| {
                    let mut samples = vec![0_i16; length].into_boxed_slice();
                    Buffer {
                        header: Box::new(Header {
                            data: samples.as_mut_ptr().cast(),
                            length: 0,
                            recorded: 0,
                            user: 0,
                            flags: 0,
                            loops: 0,
                            next: std::ptr::null_mut(),
                            reserved: 0,
                        }),
                        samples,
                        queued: false,
                    }
                })
                .collect();
            Some(Self {
                handle,
                buffers,
                channels,
                rate,
            })
        }

        fn done(buffer: &Buffer) -> bool {
            // SAFETY: the header is alive; the system sets this flag from its own thread.
            let flags = unsafe { std::ptr::read_volatile(&raw const buffer.header.flags) };
            !buffer.queued || flags & WHDR_DONE != 0
        }

        /// A buffer that is not waiting to be played.
        pub fn free(&mut self) -> Option<usize> {
            let index = self.buffers.iter().position(Self::done)?;
            self.release(index);
            Some(index)
        }

        fn release(&mut self, index: usize) {
            let buffer = &mut self.buffers[index];
            if buffer.queued {
                // SAFETY: the header was prepared for this handle and has been played, or
                // the device was reset.
                unsafe { waveOutUnprepareHeader(self.handle, &mut *buffer.header, HEADER_SIZE) };
                buffer.queued = false;
            }
        }

        pub fn samples(&mut self, index: usize) -> &mut [i16] {
            &mut self.buffers[index].samples
        }

        /// Queues the first `count` samples of a free buffer.
        pub fn queue(&mut self, index: usize, count: usize) {
            let buffer = &mut self.buffers[index];
            buffer.header.length = (count.min(buffer.samples.len()) * 2) as u32;
            buffer.header.flags = 0;
            // SAFETY: the header and the samples it points to are boxed and stay alive and
            // untouched until `release`, which runs only after the system is done with them.
            let queued = unsafe {
                waveOutPrepareHeader(self.handle, &mut *buffer.header, HEADER_SIZE) == 0
                    && waveOutWrite(self.handle, &mut *buffer.header, HEADER_SIZE) == 0
            };
            if queued {
                buffer.queued = true;
            } else {
                // SAFETY: unpreparing a header that was not queued is allowed.
                unsafe { waveOutUnprepareHeader(self.handle, &mut *buffer.header, HEADER_SIZE) };
            }
        }

        pub fn set_playing(&mut self, playing: bool) {
            // SAFETY: `handle` is open.
            unsafe {
                if playing {
                    waveOutRestart(self.handle);
                } else {
                    waveOutPause(self.handle);
                }
            }
        }

        /// Whether everything queued has been played.
        pub fn idle(&self) -> bool {
            self.buffers.iter().all(Self::done)
        }

        /// Sample frames played since the device was opened.
        pub fn played(&self) -> u64 {
            let mut time = Time {
                kind: TIME_SAMPLES,
                value: 0,
                rest: 0,
            };
            // SAFETY: `time` is valid for the call and as large as the size passed.
            if unsafe { waveOutGetPosition(self.handle, &mut time, size_of::<Time>() as u32) } != 0
            {
                return 0;
            }
            // A driver may answer in another unit than the one asked for.
            let value = u64::from(time.value);
            match time.kind {
                TIME_SAMPLES => value,
                TIME_BYTES => value / (self.channels as u64 * 2),
                TIME_MS => value * u64::from(self.rate) / 1000,
                _ => 0,
            }
        }
    }

    impl Drop for Output {
        fn drop(&mut self) {
            // SAFETY: `handle` is open. Reset returns every queued buffer, so each can be
            // unprepared before the device closes and the boxes are freed.
            unsafe { waveOutReset(self.handle) };
            for index in 0..self.buffers.len() {
                self.release(index);
            }
            // SAFETY: nothing is queued any more.
            unsafe { waveOutClose(self.handle) };
        }
    }
}

/// No sound output is written for this platform yet.
#[cfg(not(windows))]
mod output {
    pub const AVAILABLE: bool = false;

    pub struct Output;

    impl Output {
        pub fn open(_rate: u32, _channels: usize, _length: usize, _count: usize) -> Option<Self> {
            None
        }
        pub fn free(&mut self) -> Option<usize> {
            None
        }
        pub fn samples(&mut self, _index: usize) -> &mut [i16] {
            &mut []
        }
        pub fn queue(&mut self, _index: usize, _count: usize) {}
        pub fn set_playing(&mut self, _playing: bool) {}
        pub fn idle(&self) -> bool {
            true
        }
        pub fn played(&self) -> u64 {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_bytes_become_samples_at_the_gain() {
        let bytes = [0x00, 0x40, 0x00, 0xC0, 0xFF, 0x7F, 0x34];
        let mut out = [1_i16; 4];
        samples(&bytes, 0.5, &mut out);
        assert_eq!(
            out,
            [0x2000, -0x2000, 0x3FFF, 1],
            "A stray byte is left out"
        );
        samples(&bytes, 4., &mut out);
        assert_eq!(
            out[..3],
            [i16::MAX, i16::MIN, i16::MAX],
            "Clipped, not wrapped"
        );
        samples(&bytes, 0., &mut out);
        assert_eq!(out[..3], [0, 0, 0]);
    }

    #[test]
    fn a_short_source_fills_what_it_has() {
        let mut bytes = [0; 8];
        assert_eq!(fill(&mut &[1_u8, 2, 3][..], &mut bytes), 3);
        assert_eq!(fill(&mut &[9_u8; 20][..], &mut bytes), 8);
    }

    fn wait_until(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(Instant::now() < deadline, "Timed out: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Plays through the real default device at no volume, so nothing is heard.
    #[test]
    #[ignore = "requires installed FFmpeg and a sound output device"]
    fn local_sound_follows_play_pause_and_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let clip = super::super::playback::test_clip(dir.path(), "tone.mp4", "64x64", true);
        let silent = super::super::playback::test_clip(dir.path(), "silent.mp4", "64x64", false);
        if !AVAILABLE {
            assert!(Audio::start(&clip, 0., 0.).is_none());
            return;
        }
        let audio = Audio::start(&clip, 1., 0.).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(audio.position(), Some(1.), "Paused until told to play");
        audio.set_playing(true);
        let started = Instant::now();
        wait_until("sound to advance", || audio.position().unwrap_or(9.) > 1.3);
        audio.set_playing(false);
        std::thread::sleep(Duration::from_millis(100));
        let paused = audio.position().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(audio.position(), Some(paused), "Pausing holds the position");
        assert!(
            paused < 1.9,
            "The clock follows the device, not the decoder"
        );
        audio.set_playing(true);
        wait_until("the sound to end", || audio.position().is_none());
        let took = started.elapsed().as_secs_f64();
        assert!(
            (0.9..3.).contains(&took),
            "One second of sound took {took:.2} s"
        );

        // A file without sound has no position to follow.
        let audio = Audio::start(&silent, 0., 0.).unwrap();
        audio.set_playing(true);
        wait_until("the silent file to end", || audio.position().is_none());

        // Dropping stops the helper and the worker.
        let audio = Audio::start(&clip, 0., 0.).unwrap();
        audio.set_playing(true);
        let shared = audio.shared.clone();
        drop(audio);
        assert!(shared.running.stopped());
        wait_until("the worker to end", || shared.ended.load(Ordering::Relaxed));
    }
}
