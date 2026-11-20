# In-app playback — reference

Playing a local video inside the app (S-06): on the Download page after a download, in the download-all results, and on My videos. The code is `src/app/player.rs` (the widget), `src/media/playback.rs` (frames) and `src/media/audio.rs` (sound), on top of `process::Running`. The app links no media library: the bundled FFmpeg decodes, as it does for previews and GIFs.

## What the person sees

`Player::show(ui, previews, path)` is the player for one file. The picture is at most 720 wide and 520 high, and never taller than the visible part of the page, so the strip under it stays in view. Until it is clicked it is that file's poster (the `preview` widget's frame) with a play mark; a click opens the file and plays from the start. Lists use `player::thumbnail`, the same poster and mark in a small size.

- **Download page**: under **Download complete**, for a saved file whose name ends like a video's (`media::local::is_video`; an audio-only download gets no player).
- **Download all**: each saved video has a thumbnail; clicking it puts the player under that entry.
- **My videos**: clicking a thumbnail plays the video at once in the **Selected video** card, which the page scrolls to, while the file's check (FFprobe, as before) runs beside it; clicking the name only selects. **Open in player** and the button at the right of the strip hand the file to the system player and pause the app's.

The strip under the picture has play/pause, the time, the position bar, the speaker and level, the system-player button and the fullscreen button. With the pointer over the player and no text field active: Space or K play and pause, ← and → move 5 seconds, M mutes, F goes fullscreen. A double click on the picture also goes fullscreen; both of its clicks toggle playing, so it plays on as it was.

One `Player` belongs to the Download page (`App::player`) and one to My videos (`Library::player`). Each holds at most one open video: asking it to show another file closes the open one. A page that is not shown has its player suspended, so only one video plays.

## Fullscreen

`Player::set_fullscreen` sends `ViewportCommand::Fullscreen(true)`: winit makes the window borderless fullscreen on its monitor and, on Windows, saves its placement (`GetWindowPlacement`), which `Fullscreen(false)` restores, so the window comes back maximized or at its size and place. The session is not touched, so playback goes on without a restart; entering also ends any text input on the page so the player gets the keys.

The page still lays out underneath: `video` allocates the player's box as usual (painted black) and draws the picture and strip in an `egui::Area` of `Order::Foreground` that covers `ctx.content_rect()`. The rest of the page, the sidebar and the Dev mode button are under it, and the page keeps its layout and scroll position, so leaving shows the same page where it was. The picture keeps its shape on black (`fit`) and the strip lies over its bottom.

- Keys work anywhere while fullscreen (not only with the pointer over the player). Escape is consumed by the player, so it only leaves fullscreen and the Dev mode and feedback panels underneath never see it.
- `Fullscreen::controls_shown`: the strip shows while the video is paused, while the pointer is on it or the bar is dragged, and for 2.5 s after the last pointer movement, click, scroll or key press (`stirred`). Hidden, the strip takes no clicks and the cursor is `CursorIcon::None`.
- Leaving: the button, F, Escape or a double click; and `open` (another video), `close`, `suspend` (the page is not shown, a tools update, closing the app) and the system-player button. The `Fullscreen` state keeps a clone of the `egui::Context` so these can restore the window without a frame. At the end of the video the window stays fullscreen on the last picture, as the page does.
- A safety net for a page that stops calling `show` without closing the player: the player writes the pass it was drawn in to the context's temporary data, and an `on_end_pass` callback (registered once per context) restores the window at the end of a pass that did not draw it. The player then finds the mark gone and draws itself on the page again.
- The picture is decoded at most 960 pixels on its longer side (see Frames), so fullscreen enlarges it.
- No platform-specific code: macOS takes the same viewport command (winit's borderless fullscreen there).

## The system player

The button at the right of the strip (and **Open in player** on My videos, **Play source video** and **Preview GIF** on the GIF page) calls `platform::open_file`, not `open_target`, which stays for links. It first checks that the file exists ("This file was moved or deleted. Choose it again." otherwise). On Windows it then calls `ShellExecuteExW` with no verb, the default action a double click in Explorer runs, with `SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI`, so the call returns once the app was started and reports a failure as an error code instead of a dialog: missing file or folder, no app associated with the type (`ERROR_NO_ASSOCIATION`, which asks the person to choose one in Open with), access denied, or anything else. The structure and function are declared by hand in `src/platform.rs`, like the wave output; no new dependency. On macOS it is `/usr/bin/open` through `open_target`, as before. The player pauses its own video and leaves fullscreen first, so the two never play at once and the other player is not behind the app's window.

Before, the button called `open_target`, which runs `rundll32.exe url.dll,FileProtocolHandler <path>`. That treats its argument as a URL, takes the command line in the system's ANSI code page, and only reports whether rundll32 started. Tested on November 20, 2026 (Windows 11, default player Media Player), with generated clips each showing its own case number, through the same calls as the app:

| File name | rundll32 (before) | `ShellExecuteExW` (now) |
| --- | --- | --- |
| `plain.mp4` | opened | opened |
| `with spaces.mp4` | opened | opened |
| letters outside the code page (`café ñ 日本.mp4`) | **nothing opened**, rundll32 exited with 0 | opened |
| `a%20b.mp4` | opened the right file | opened |
| `100% done.mp4` | opened | opened |
| `part #2.mp4` | opened | opened |
| `tom & jerry.mp4` | opened | opened |
| `one, two.mp4` | opened | opened |
| `semi;colon (1) [x].mp4` | opened | opened |
| a path of 332 characters | opened | opened |
| a missing file | nothing, exit code 0 | error 2, shown as the moved-file message |

The call took 140 to 320 ms on the UI thread (it waits for the start to be handed over), once per click. A type with no associated app was not tried on this machine.

## Frames

`playback::Decoder::start(path, seconds, wake)` starts one FFmpeg run from a moment to the end of the file and returns at once; a worker thread starts the helper and reads it.

```
ffmpeg -ss <seconds> -i <file> -map 0:V:0 -an -sn -dn
  -vf scale=…(at most 960 on the longer side, never upscaled, even sides),setsar=1,format=yuv420p
  -fpsmax <60, or fewer after falling behind> -f yuv4mpegpipe pipe:1
```

- The output is YUV4MPEG2: a header line with the size and frame rate, then `FRAME` and one 4:2:0 picture at a time. The header is what tells the app the picture's shape, so nothing is probed before the first frame. The worker converts each picture to RGBA (BT.709, limited range, which the scale filter is told to produce) and gives frame `n` the moment `seconds + n / rate`; the muxer is constant-rate, so FFmpeg has already repeated or dropped frames of a variable-rate file.
- Frames go through a channel that holds three. With three untaken the worker waits, FFmpeg's pipe fills and FFmpeg waits too: a paused video decodes nothing, and decoding never runs ahead of playback by more than that.
- `process::Running` holds the helper. Dropping the `Decoder` kills FFmpeg at once from the calling thread (FFmpeg starts no programs of its own, so no process-tree walk) and the worker ends on the closed pipe or channel.
- The end is `Step::End(Ok)`. A file that gives no frame is `Step::End(Err(NOT_PLAYABLE))`; missing tools are `NO_TOOLS`. Only `file` is on FFmpeg's protocol whitelist.
- `playback::probe` (FFprobe, on its own thread) reads the duration and whether there is an audio stream. Playback starts without waiting for it; until it arrives the position bar shows only the time, and without a duration it cannot seek.

Seeking, and playing again after the end, is a new `Decoder` (and `Audio`) at the new moment: `-ss` before `-i` jumps to the keyframe before and decodes up to the moment asked for. The frame on screen stays until the new one arrives. While the position bar is dragged, a new run starts at most every 120 ms without sound; releasing it starts the final one with sound.

## Sound

`audio::Audio::start(path, seconds, gain)` runs a second FFmpeg from the same moment:

```
ffmpeg -ss <seconds> -i <file> -map 0:a:0 -vn -sn -dn -ac 2 -ar 48000 -f s16le pipe:1
```

A worker thread feeds that PCM to the system output in three 40 ms buffers, applying the gain (the level squared, or 0 when muted) as each buffer is queued, so a volume change is heard within about 120 ms. The output starts paused and is started when the first picture is on screen.

- **Windows**: the wave output (`winmm`: `waveOutOpen` on the default device, `waveOutWrite`, `waveOutPause`/`Restart`, `waveOutReset`), declared by hand like the app's other Windows calls; no audio crate. `waveOutGetPosition` gives the samples actually played.
- **macOS**: an Audio Queue (AudioToolbox: `AudioQueueNewOutput` on the default device with its callback on the queue's own thread, `AudioQueueAllocateBuffer`/`EnqueueBuffer`, `AudioQueueStart`/`Pause`, then `AudioQueueStop` and `AudioQueueDispose`, both immediate), declared by hand from `AudioQueue.h` and `CoreAudioBaseTypes.h`, with the struct layouts checked at compile time; no audio crate. The callback only marks its buffer free. `AudioQueueGetCurrentTime` gives the queue's sample time, which may run on through a pause or a gap between buffers, so `audio::Frames` counts its advance only while playing and never past what was queued. The Mac FFmpeg is built with the `pcm_s16le` encoder for this (`scripts/setup-macos.py`). Compiled and checked by CI, not yet heard on a Mac. AV1 files do not decode there, as for previews.
- A file without an audio stream, or a machine without an output device, ends the sound run at once and playback is silent. So does an output that cannot start, or that holds queued sound for two seconds without playing any (`audio::Stall`); the picture then runs on its own clock.
- The first time a process opens the wave output takes about half a second. `audio::warm_up` opens and closes it once on a background thread when the first poster or thumbnail is drawn, so the first video does not wait for it.

## The clock

The player's time is a wall clock (`player::Clock`) that the sound corrects: while `Audio::position` has an answer (start moment + samples played ÷ 48000, read by the worker every 5 ms), every frame of the UI sets the clock to it. So the picture follows what is heard, and pausing, a slow device or a dropout cannot pull them apart. When the sound ends before the picture, or there is none, the wall clock simply runs on.

Each UI frame, `Session::advance` takes the decoded frames whose moment has come, uploads the newest as the texture and asks for a repaint when the next one is due; frames that are already late are skipped. The clock never waits for the picture. When the picture is more than half a second behind its moment (a decoder that cannot keep up; judged 1.5 s after a run started at the earliest), `Session::catch_up` starts a new `Decoder` a quarter of a second ahead of the clock, asked for half as many frames a second (60, then 30, then 15), and leaves the sound alone. Nothing on the UI thread decodes, reads the file or waits for a helper; it starts threads and takes what they have ready.

## Stopping

- Opening another file, closing, leaving the page, starting a tools update and closing the app all drop the `Decoder` and `Audio`, which stops both helpers and the sound at once, and leave fullscreen.
- A video paused for 30 seconds, or on a page that is no longer shown, is parked: its helpers stop and its picture and position stay. Playing again starts new ones at that position.
- The player never writes, moves or changes a file, and nothing about what is played is recorded, sent or added to recent downloads, usage reports, feedback or dev notes.

## Thumbnails

`preview::Previews` extracts posters with up to three single-threaded FFmpeg runs at once (it was one), only for rows that are visible, and keeps the last 48 as textures. `Previews::cached` lets the player paint an already loaded poster until its first frame arrives.

## Tests

In `cargo xtask check`: the stream header and color conversion, PCM and gain, the clock, the Mac output's frame count and the stall rule, the picture's fit, the position while dragging, frames waiting for their moment, the end and failure states, and the poster and player drawing headless. Fullscreen, headless with the viewport commands each pass asks for: on and off once each and only with a video open; Escape leaving it without reaching the rest of the app, and Escape left alone when not fullscreen; F; a pass that does not draw the player restoring the window; close, suspend and another video leaving it; the controls' hide timing; what counts as movement; the system player pausing and leaving fullscreen. In `platform.rs`: a missing file (also with an awkward name) and a folder give the moved-file message without starting anything, the error codes' messages, and the size of the hand-declared `SHELLEXECUTEINFOW` (112 bytes on 64-bit).

In `cargo xtask pipeline` (ignored tests, on clips FFmpeg generates):

- `local_playback_decodes_seeks_and_stops`: a 1080×1920 clip with sound and an awkward name decodes to 540×960 at its frame rate; a seek shows what the first run showed at that moment; a small WebM is not upscaled; an untaken video stops after the three frames ahead; dropping stops FFmpeg; a broken file gives the message.
- `local_player_plays_pauses_seeks_and_ends`: the player's session, without a window, on real workers: two seconds of video take two seconds and show at least 30 of 40 frames in order; play after the end starts over; pause holds; dragging shows the moment without sound; a parked video resumes where it was; a broken file shows its error. It plays through the default sound output at no volume. Set `STREAM_DLP_PLAYBACK_TEST_FILE` to a video of your own (run the test by name, not through the pipeline, which removes that variable) to play its first five seconds and print how many frames were shown.

By name only, because it needs a sound output device: `cargo test --locked local_sound_follows_play_pause_and_the_end -- --ignored` plays a generated tone at no volume and checks that the position stays put until play, follows the device rather than the decoder, holds on pause and ends with the sound.

### Measured, November 17, 2026 (Windows 11)

Generated clips (FFmpeg's test pattern and a tone, in the ignored `test-output/`), five seconds of each through `local_player_plays_pauses_seeks_and_ends` with `STREAM_DLP_PLAYBACK_TEST_FILE`:

| Clip | Release build | Debug build |
| --- | --- | --- |
| H.264 1920×1080 at 60 fps, AAC, MP4 | 301 of 300 frames, 5.18 s | 219 frames after dropping to 30 fps, 5.17 s |
| VP9 3840×2160 at 30 fps, Opus, WebM | 151 of 150 frames, 5.14 s | 151 frames, 5.20 s |
| AV1 1080×1920 at 30 fps, Opus, MKV | 151 of 150 frames, 5.10 s | 151 frames, 5.15 s |
| H.264 1280×720, no sound, 30 minutes (1.6 GB) | not run | 151 frames, 5.12 s |

The time is from opening the file to showing the frame at 5.00 s, so what exceeds five seconds is the start: 0.1 to 0.2 s. In a traced run every frame was shown within 5 ms of the sound's position. Before the catch-up rule the debug build fell 1.1 s behind the sound on the 60 fps clip, and before the warm-up the first video of a process waited about half a second on its first frame for the sound output.

### In the window, November 17, 2026 (Windows 11, debug build)

The same generated clips on My videos, driven by hand in the app's window, with the level of the app's own Windows audio session read every quarter second:

- The four thumbnails appeared within a few seconds of choosing the folder. Clicking one scrolled to the player and played it; clicking another switched at once.
- Play and pause by the button, a click on the picture and Space; a click on the position bar; a drag along it, which shows each moment and its time; ← and →; the replay mark at the end. A seek to 23:50 of the 30-minute file showed its frame within a second.
- Sound: a steady level for exactly the clip's seven seconds, nothing while muted, the level again when unmuted, and about 0.15 of it at a volume near 0.4 (the level squared). AAC in MP4 and Opus in WebM and MKV all played. The file without sound showed the greyed speaker and no level.
- Leaving the page and coming back left the video paused on its picture.
- Found and fixed there: a portrait video made the player taller than a small window and pushed the strip out of view. Its height is now limited to the visible part of the page, and the page keeps the player in view while it takes the picture's shape.

Not verified: no person has listened to it (the level proves a signal of the right size and duration, not how it sounds), the Download page's and download-all's players were not shown with a real download, real downloads were not played, and nothing was run on a Mac (its build passes CI).

### Fullscreen and the system player in the window, November 20, 2026 (Windows 11, debug build)

On My videos, with a generated 1280×720 clip with a tone (its name has an accented letter) and a 540×960 one, the window once at a normal size and once maximized:

- The fullscreen button (hover text **Fullscreen**), F and a double click each made the window fill the screen with the picture on black, the taskbar hidden and the strip over the bottom; the portrait clip was centred between black bars. Playing went on through each change (the time kept counting, and a double click left it playing).
- Without movement the strip went away after about 2.5 s and `GetCursorInfo` reported no cursor; a mouse movement brought both back. Paused, the strip stayed.
- Fullscreen: the play/pause button, Space, →, a click and a drag on the position bar, mute and unmute all worked. At the end the last picture stayed fullscreen with the play mark; Space played it again.
- Escape, F and the exit button (inward corners, hover text **Exit fullscreen**) restored the window at its size and place, or maximized when it had been, with the page at the same scroll position.
- The system-player button, fullscreen and playing: Media Player opened that clip, and the app had left fullscreen and paused. From the page it opened again. After the file was renamed, the button showed the moved-file message under the player.
- Found and fixed there: the fullscreen layer's `Area` had the same id as the fullscreen button, so the strip's buttons took no clicks in fullscreen (a click went to the picture).

Not verified: the Download page's and download-all's players in fullscreen (no download was run; they call the same `Player::show`, and the headless tests cover the shared code), a second monitor, a person watching for stutter beyond the time counting on, and anything on a Mac (no local Mac build; CI checks it).
