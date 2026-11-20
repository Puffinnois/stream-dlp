//! Sound for the in-app player: FFmpeg decodes a file's first audio stream to PCM from a chosen
//! moment and a worker thread feeds it to the system's audio output, whose count of played
//! samples is the player's clock. Windows (the wave output) and macOS (an Audio Queue); elsewhere
//! [`Audio::start`] gives `None` and the player runs silent on its own clock.
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
/// How long an output may hold queued sound without playing any before it counts as broken.
const STALL: Duration = Duration::from_secs(2);

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

/// Frames of queued sound played, from a device timeline that keeps its own count, as the Audio
/// Queue's does: one that may run on through a pause or a gap between buffers, or start over.
/// Its advance counts only while playing, and never past what was queued.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Default)]
struct Frames {
    queued: u64,
    played: f64,
    /// The device time last seen.
    last: Option<f64>,
    playing: bool,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Frames {
    fn queue(&mut self, frames: u64) {
        self.queued += frames;
    }

    fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    /// Takes the device time now, `None` when it has none, and gives the frames played.
    fn observe(&mut self, now: Option<f64>) -> u64 {
        if let Some(now) = now {
            // The timeline counts from the start, so its first reading while playing is all
            // played; one that went back started over and adds nothing.
            let since = now - self.last.unwrap_or(0.);
            if self.playing && since > 0. {
                self.played = (self.played + since).min(self.queued as f64);
            }
            self.last = Some(now);
        }
        self.played as u64
    }
}

/// Notices an output that holds queued sound but never plays it, so the picture is not held
/// on a clock that does not move.
struct Stall {
    played: u64,
    since: Instant,
}

impl Stall {
    fn new(now: Instant) -> Self {
        Self {
            played: 0,
            since: now,
        }
    }

    /// `waiting` while sound is queued and should be playing.
    fn stalled(&mut self, played: u64, waiting: bool, now: Instant) -> bool {
        if !waiting || played != self.played {
            self.played = played;
            self.since = now;
            return false;
        }
        now.duration_since(self.since) > STALL
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
    let mut stall = Stall::new(Instant::now());
    while !shared.running.stopped() {
        let wanted = shared.playing.load(Ordering::Relaxed);
        if wanted != playing {
            output.set_playing(wanted);
            playing = wanted;
        }
        // A device that cannot play ends the sound; the picture runs on its own clock.
        if output.failed() {
            return;
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
        let played = output.played();
        let running = playing && !output.idle();
        if stall.stalled(played, running, Instant::now()) {
            return;
        }
        *shared
            .position
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Position {
            played: played as f64 / f64::from(RATE),
            at: Instant::now(),
            running,
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

        /// Whether the device stopped being able to play. The wave output reports that on
        /// opening.
        pub fn failed(&self) -> bool {
            false
        }

        /// Sample frames played since the device was opened.
        pub fn played(&mut self) -> u64 {
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

/// The macOS Audio Queue output (AudioToolbox), which plays through the default output device.
/// Declared from `AudioToolbox/AudioQueue.h` and `CoreAudioTypes/CoreAudioBaseTypes.h`, which
/// use natural alignment; `OSStatus` is `SInt32` and `Boolean` is `unsigned char` (`MacTypes.h`).
#[cfg(target_os = "macos")]
mod output {
    use super::Frames;
    use std::{
        ffi::c_void,
        mem::{offset_of, size_of},
        ptr,
        sync::atomic::{AtomicBool, Ordering},
    };

    pub const AVAILABLE: bool = true;

    /// `AudioQueueRef`.
    type Queue = *mut c_void;
    /// `kAudioFormatLinearPCM`, the four characters `'lpcm'`.
    const LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
    const FLAG_SIGNED_INTEGER: u32 = 1 << 2;
    const FLAG_PACKED: u32 = 1 << 3;
    /// `kAudioTimeStampSampleTimeValid`.
    const SAMPLE_TIME_VALID: u32 = 1 << 0;

    /// `AudioStreamBasicDescription`.
    #[repr(C)]
    struct Format {
        sample_rate: f64,
        format: u32,
        flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels: u32,
        bits_per_channel: u32,
        reserved: u32,
    }

    /// `AudioQueueBuffer`. The queue allocates it and its data, and frees both on disposal.
    #[repr(C)]
    struct Buffer {
        capacity: u32,
        data: *mut c_void,
        size: u32,
        user: *mut c_void,
        descriptions_capacity: u32,
        descriptions: *mut c_void,
        descriptions_count: u32,
    }

    /// `SMPTETime`.
    #[repr(C)]
    struct Smpte {
        subframes: i16,
        subframe_divisor: i16,
        counter: u32,
        kind: u32,
        flags: u32,
        hours: i16,
        minutes: i16,
        seconds: i16,
        frames: i16,
    }

    /// `AudioTimeStamp`.
    #[repr(C)]
    struct TimeStamp {
        sample_time: f64,
        host_time: u64,
        rate_scalar: f64,
        word_clock_time: u64,
        smpte: Smpte,
        flags: u32,
        reserved: u32,
    }

    // The layouts the headers give on 64-bit Apple systems.
    const _: () = {
        assert!(size_of::<Format>() == 40);
        assert!(size_of::<Buffer>() == 56);
        assert!(offset_of!(Buffer, size) == 16 && offset_of!(Buffer, user) == 24);
        assert!(size_of::<Smpte>() == 24);
        assert!(size_of::<TimeStamp>() == 64);
        assert!(offset_of!(TimeStamp, smpte) == 32 && offset_of!(TimeStamp, flags) == 56);
    };

    /// `AudioQueueOutputCallback`.
    type Callback = unsafe extern "C" fn(user: *mut c_void, queue: Queue, buffer: *mut Buffer);

    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        fn AudioQueueNewOutput(
            format: *const Format,
            callback: Callback,
            user: *mut c_void,
            run_loop: *const c_void,
            run_loop_mode: *const c_void,
            flags: u32,
            queue: *mut Queue,
        ) -> i32;
        fn AudioQueueAllocateBuffer(queue: Queue, size: u32, buffer: *mut *mut Buffer) -> i32;
        fn AudioQueueEnqueueBuffer(
            queue: Queue,
            buffer: *mut Buffer,
            description_count: u32,
            descriptions: *const c_void,
        ) -> i32;
        fn AudioQueueStart(queue: Queue, start: *const TimeStamp) -> i32;
        fn AudioQueuePause(queue: Queue) -> i32;
        fn AudioQueueStop(queue: Queue, immediate: u8) -> i32;
        fn AudioQueueDispose(queue: Queue, immediate: u8) -> i32;
        fn AudioQueueGetCurrentTime(
            queue: Queue,
            timeline: *mut c_void,
            time: *mut TimeStamp,
            discontinuity: *mut u8,
        ) -> i32;
    }

    /// What the callback, on one of the queue's own threads, shares with the worker: which
    /// buffers the queue still holds.
    struct Context {
        queued: Box<[AtomicBool]>,
    }

    /// The queue is done with `buffer`.
    unsafe extern "C" fn returned(user: *mut c_void, _queue: Queue, buffer: *mut Buffer) {
        // SAFETY: `user` is the `Context` that `Output` keeps alive until the queue is
        // disposed, after which no callback runs. `buffer` is one of this queue's buffers;
        // its `user` field holds its index, written before it was first queued and never
        // changed after.
        let (context, index) = unsafe { (&*user.cast::<Context>(), (*buffer).user as usize) };
        if let Some(queued) = context.queued.get(index) {
            queued.store(false, Ordering::Release);
        }
    }

    pub struct Output {
        queue: Queue,
        // Owned by the queue, which frees them when it is disposed.
        buffers: Vec<*mut Buffer>,
        samples: Vec<Box<[i16]>>,
        // Boxed: the queue's threads keep this address until the queue is disposed.
        context: Box<Context>,
        frames: Frames,
        channels: usize,
        failed: bool,
    }

    impl Output {
        /// Opens the default device, paused, with `count` buffers of `length` samples.
        pub fn open(rate: u32, channels: usize, length: usize, count: usize) -> Option<Self> {
            let block = (channels * 2) as u32;
            let format = Format {
                sample_rate: f64::from(rate),
                format: LINEAR_PCM,
                // Native byte order, little-endian on Apple silicon: no big-endian flag.
                flags: FLAG_SIGNED_INTEGER | FLAG_PACKED,
                bytes_per_packet: block,
                frames_per_packet: 1,
                bytes_per_frame: block,
                channels: channels as u32,
                bits_per_channel: 16,
                reserved: 0,
            };
            let context = Box::new(Context {
                queued: (0..count).map(|_| AtomicBool::new(false)).collect(),
            });
            let mut queue: Queue = ptr::null_mut();
            // SAFETY: `format` and `queue` are valid for the call, which copies the format.
            // No run loop: the callback runs on one of the queue's threads. The context it
            // gets is boxed, holds only atomics, and outlives the queue (see `Drop`).
            let status = unsafe {
                AudioQueueNewOutput(
                    &format,
                    returned,
                    ptr::from_ref::<Context>(&context).cast_mut().cast(),
                    ptr::null(),
                    ptr::null(),
                    0,
                    &mut queue,
                )
            };
            if status != 0 || queue.is_null() {
                return None;
            }
            // From here `Drop` disposes the queue, also when a buffer cannot be had.
            let mut output = Self {
                queue,
                buffers: Vec::with_capacity(count),
                samples: Vec::with_capacity(count),
                context,
                frames: Frames::default(),
                channels,
                failed: false,
            };
            let bytes = (length * 2) as u32;
            for index in 0..count {
                let mut buffer: *mut Buffer = ptr::null_mut();
                // SAFETY: `queue` is open and `buffer` is valid for the call.
                let status = unsafe { AudioQueueAllocateBuffer(queue, bytes, &mut buffer) };
                // SAFETY: a buffer the call gave is valid until the queue is disposed, and the
                // queue does not use it before it is queued.
                if status != 0 || buffer.is_null() || unsafe { (*buffer).capacity } < bytes {
                    return None;
                }
                // SAFETY: as above; the callback reads this to find the buffer's flag.
                unsafe { (*buffer).user = index as *mut c_void };
                output.buffers.push(buffer);
                output.samples.push(vec![0_i16; length].into_boxed_slice());
            }
            Some(output)
        }

        /// A buffer that is not waiting to be played.
        pub fn free(&mut self) -> Option<usize> {
            self.context
                .queued
                .iter()
                .position(|queued| !queued.load(Ordering::Acquire))
        }

        pub fn samples(&mut self, index: usize) -> &mut [i16] {
            &mut self.samples[index]
        }

        /// Queues the first `count` samples of a free buffer.
        pub fn queue(&mut self, index: usize, count: usize) {
            let samples = &self.samples[index];
            let count = count.min(samples.len());
            let buffer = self.buffers[index];
            // SAFETY: the buffer is not queued, so the queue's threads leave its data alone,
            // and its capacity was checked to hold all the samples.
            unsafe {
                ptr::copy_nonoverlapping(
                    samples.as_ptr().cast::<u8>(),
                    (*buffer).data.cast::<u8>(),
                    count * 2,
                );
                (*buffer).size = (count * 2) as u32;
            }
            // Marked first: the callback may run as soon as the buffer is queued.
            let queued = &self.context.queued[index];
            queued.store(true, Ordering::Release);
            // SAFETY: `queue` is open and the buffer is its own; linear PCM needs no packet
            // descriptions.
            if unsafe { AudioQueueEnqueueBuffer(self.queue, buffer, 0, ptr::null()) } == 0 {
                self.frames.queue((count / self.channels) as u64);
            } else {
                queued.store(false, Ordering::Release);
            }
        }

        pub fn set_playing(&mut self, playing: bool) {
            // SAFETY: `queue` is open; starting again after a pause resumes it.
            let status = unsafe {
                if playing {
                    AudioQueueStart(self.queue, ptr::null())
                } else {
                    AudioQueuePause(self.queue)
                }
            };
            if playing && status != 0 {
                self.failed = true;
            }
            self.frames.set_playing(playing && !self.failed);
        }

        /// Whether everything queued has been played.
        pub fn idle(&self) -> bool {
            self.context
                .queued
                .iter()
                .all(|queued| !queued.load(Ordering::Acquire))
        }

        /// Whether the device could not start playing.
        pub fn failed(&self) -> bool {
            self.failed
        }

        /// Sample frames played since the device was opened.
        pub fn played(&mut self) -> u64 {
            // SAFETY: all-zero is a valid `AudioTimeStamp`.
            let mut time: TimeStamp = unsafe { std::mem::zeroed() };
            // SAFETY: `queue` is open and `time` is valid for the call; without a timeline
            // there is no discontinuity flag to ask for.
            let status = unsafe {
                AudioQueueGetCurrentTime(self.queue, ptr::null_mut(), &mut time, ptr::null_mut())
            };
            // Before the queue first starts there is no time to read.
            let now =
                (status == 0 && time.flags & SAMPLE_TIME_VALID != 0).then_some(time.sample_time);
            self.frames.observe(now)
        }
    }

    impl Drop for Output {
        fn drop(&mut self) {
            // SAFETY: `queue` is open. Stopping at once returns the queued buffers through the
            // callback while the context is alive. Disposing at once frees the buffers and
            // runs no callback after it returns, so the context can then be freed with `self`.
            unsafe {
                AudioQueueStop(self.queue, 1);
                AudioQueueDispose(self.queue, 1);
            }
        }
    }
}

/// No sound output is written for this platform yet.
#[cfg(not(any(windows, target_os = "macos")))]
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
        pub fn failed(&self) -> bool {
            false
        }
        pub fn played(&mut self) -> u64 {
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

    #[test]
    fn played_frames_follow_the_timeline_only_while_playing() {
        let mut frames = Frames::default();
        assert_eq!(frames.observe(None), 0, "No time before the device starts");
        frames.queue(4800);
        assert_eq!(frames.observe(Some(0.)), 0, "Paused");
        frames.set_playing(true);
        assert_eq!(frames.observe(None), 0);
        assert_eq!(frames.observe(Some(960.)), 960, "Counted from the start");
        assert_eq!(frames.observe(Some(1920.5)), 1920);

        // A timeline that runs on through a pause adds nothing for it.
        frames.set_playing(false);
        assert_eq!(frames.observe(Some(3000.)), 1920);
        frames.set_playing(true);
        assert_eq!(frames.observe(Some(3100.)), 2020);

        // Nothing past what was queued, and the gap before more arrives is not counted.
        assert_eq!(frames.observe(Some(9000.)), 4800);
        assert_eq!(frames.observe(Some(12_000.)), 4800);
        frames.queue(4800);
        assert_eq!(frames.observe(Some(12_480.)), 5280);

        // A timeline that starts over goes on from where the sound was.
        assert_eq!(frames.observe(Some(100.)), 5280);
        assert_eq!(frames.observe(Some(580.)), 5760);
    }

    #[test]
    fn an_output_that_holds_sound_without_playing_it_stalls() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut stall = Stall::new(start);
        assert!(!stall.stalled(0, true, at(1_000)));
        assert!(
            stall.stalled(0, true, at(2_500)),
            "Queued, playing, not moving"
        );
        let mut stall = Stall::new(start);
        assert!(!stall.stalled(10, true, at(1_900)));
        assert!(!stall.stalled(10, true, at(3_500)), "Moved 1.6 s ago");
        assert!(
            !stall.stalled(10, false, at(9_000)),
            "Paused or nothing queued"
        );
        assert!(!stall.stalled(10, true, at(10_000)));
        assert!(stall.stalled(10, true, at(11_100)));
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
