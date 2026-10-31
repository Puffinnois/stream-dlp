# Experimental automatic Streamable uploads

This implementation automates the website. It does not use an upload API or
replay undocumented network endpoints. Streamable's public API still excludes
uploads. Website compatibility may change without an app release.

## User flow

After downloading, choose **Upload on Streamable**, or select an existing video
on **My videos**. The selected-video card offers **Upload automatically
(experimental)**, describes transmission and public-link behavior, and links
the upload terms. Uploading is always an explicit choice, never an automatic
side effect of downloading or choosing a folder.

The app opens a dedicated private Edge/Chrome window, automatically selects the
file, watches the upload/processing stages, and verifies the resulting public
player by observing loaded video and advancing playback. A share URL alone is
not success. Playback is checked in a separate private tab so the upload page
can finish without being navigated away. On completion the app shows the link
and a copy button.

The user handles sign-in or challenges in that visible browser. No passwords,
verification solutions, normal browser profiles, or YouTube sessions are read.
Progress percentages appear only when exposed by website controls; otherwise
the app shows a spinner and current phase. Manual website assistance remains
available when automation fails or the necessary browser/tools are unavailable.

## Runtime and lifecycle

- Reuses bundled Deno; the small, dependency-free JavaScript driver is embedded
  in the Rust executable, so no separate script installation is needed.
- Discovers installed Edge/Chrome on Windows; standard Chrome/Edge application
  paths are also recognized on macOS and Linux. Only Windows/Edge is locally
  exercised. Other browser/OS combinations remain unverified.
- Launches a fresh temporary profile with private browsing and a dynamically
  allocated debugging endpoint bound to loopback. The driver has network
  permission only for that specific loopback port and no filesystem, subprocess,
  environment, remote-module, or arbitrary internet permission. The owned
  browser performs website traffic and reads the explicitly selected file.
- Watches rendered DOM controls. It submits the selected file once and excludes
  any pre-existing dashboard links. It never automatically retries an ambiguous
  upload, creates a shared account, or stores a user's sign-in preference.
- Cancel and normal app closure stop the owned helper/browser process trees.
  The temporary profile/script directory is removed afterward. A crash or forced
  process termination can bypass normal cleanup; crash recovery is not proven.
- Retains an allocated but unverified link on failure so the user can inspect
  it before choosing to retry. Cancellation cannot retract bytes already sent
  or delete a remote video. Originals and download results stay intact.
- Startup has a 30-second budget; the overall operation has a 20-minute budget.
  The driver separately bounds website upload/playback polling and CDP commands.

## Verification

`cargo test --locked`, Clippy, and driver unit tests cover scoped share URLs,
link allocation versus verified completion, progress validation, local-video
selection, and single-submission behavior.

The local Chromium fixture test uses a temporary local web server. It asserts
that the browser selects the exact file and uploads its unchanged bytes once,
ignores an old share link, plays a real MP4 at the new link, and removes the
temporary profile. It makes no request to Streamable:

```powershell
.\tools\deno.exe test --no-config --no-remote scripts/streamable-upload.test.js
.\tools\deno.exe test --no-config --no-remote --allow-read=. --allow-write=test-output --allow-net=127.0.0.1 '--allow-run=C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe' scripts/streamable-upload.test.js -- 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe' test-output/streamable-r02/synthetic-smoke.mp4
```

The ignored `authorized_streamable_upload` Rust test exercises the actual
production runtime and **publishes a video**. Run it only after explicit
authorization of that file, destination, and upload terms:

```powershell
$env:STREAM_DLP_UPLOAD_TEST_FILE = 'ABSOLUTE_PATH_TO_AUTHORIZED_VIDEO'
cargo test --locked authorized_streamable_upload -- --ignored --nocapture
Remove-Item Env:STREAM_DLP_UPLOAD_TEST_FILE
```

Live acceptance, signed-in cases, website rejection/verification flows, and
cross-platform browser compatibility must be recorded separately from fixture
success. Do not advertise those cases as proven by the local test.

### Live result — October 21, 2026

The user explicitly authorized upload of the generated 2-second test pattern
and tone and acceptance of the upload terms. The production Rust runtime and
embedded Deno driver passed `authorized_streamable_upload` on Windows/Edge in
15.80 seconds. It automatically selected the file through the website's actual
file chooser, uploaded it, and verified playback in a separate tab. Final link:
<https://streamable.com/qcnrfj>. This is a temporary guest upload.

The source was the same 75,355-byte H.264/AAC sample described in the R-02
record. No personal video or browser profile was used. The local fixture also
passed with an intentionally inactive file input ahead of the real control,
ensuring automation uses the website's chooser rather than the first input.
Signed-in uploads, large files, challenges, and other OS/browser combinations
remain unverified. Desktop screenshot inspection stopped because Windows
automation could not establish the current browser URL; the successful live
acceptance above uses the app's production runtime, not desktop automation.
