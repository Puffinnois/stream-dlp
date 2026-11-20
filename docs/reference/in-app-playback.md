# In-app playback — reference

Playing a local video inside the app (S-06): on the Download page after a download, in the download-all results, and on My videos. The code is `src/app/player.rs` (the widget), `src/media/playback.rs` (frames) and `src/media/audio.rs` (sound), on top of `process::Running`. The app links no media library: the bundled FFmpeg decodes, as it does for previews and GIFs.

## What the person sees

`Player::show(ui, previews, path)` is the player for one file. The picture is at most 720 wide and 520 high, and never taller than the visible part of the page, so the strip under it stays in view. Until it is clicked it is that file's poster (the `preview` widget's frame) with a play mark; a click opens the file and plays from the start. Lists use `player::thumbnail`, the same poster and mark in a small size.

- **Download page**: under **Download complete**, for a saved file whose name ends like a video's (`media::local::is_video`; an audio-only download gets no player).
- **Download all**: each saved video has a thumbnail; clicking it puts the player under that entry.
- **My videos**: clicking a thumbnail plays the video at once in the **Selected video** card, which the page scrolls to, while the file's check (FFprobe, as before) runs beside it; clicking the name only selects. **Open in player** and the button at the right of the strip hand the file to the system player and pause the app's.

The strip under the picture has play/pause, the time, the position bar, the speaker and level, and the system-player button. With the pointer over the player and no text field active: Space or K play and pause, ← and → move 5 seconds, M mutes.

One `Player` belongs to the Download page (`App::player`) and one to My videos (`Library::player`). Each holds at most one open video: asking it to show another file closes the open one. A page that is not shown has its player suspended, so only one video plays.

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

- Opening another file, closing, leaving the page, starting a tools update and closing the app all drop the `Decoder` and `Audio`, which stops both helpers and the sound at once.
- A video paused for 30 seconds, or on a page that is no longer shown, is parked: its helpers stop and its picture and position stay. Playing again starts new ones at that position.
- The player never writes, moves or changes a file, and nothing about what is played is recorded, sent or added to recent downloads, usage reports, feedback or dev notes.

## Thumbnails

`preview::Previews` extracts posters with up to three single-threaded FFmpeg runs at once (it was one), only for rows that are visible, and keeps the last 48 as textures. `Previews::cached` lets the player paint an already loaded poster until its first frame arrives.

## Tests

In `cargo xtask check`: the stream header and color conversion, PCM and gain, the clock, the Mac output's frame count and the stall rule, the picture's fit, the position while dragging, frames waiting for their moment, the end and failure states, and the poster and player drawing headless.

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
