# Chrome/Edge companion extension (experimental) — reference

Roadmap item A-03, option 1 of [Chrome auto-mode options](../research/chrome-auto-mode-options.md).
Status (2026-11-16): **prototype**. Built and tested offline; nobody has run it in a real
Chrome or Edge yet, and no signed-in YouTube inspection or download through it has been
proven. It is shown only in Dev mode (Ctrl+Shift+D), on Windows, and labelled experimental.
Chrome and Edge keep the guided export for everyone else.

## How it works

```
Chrome/Edge profile                 stream-dlp.exe (host mode)           stream-dlp (running app)
extension/ ── connectNative ──▶  stdin/stdout, framed JSON  ── loopback TCP, HMAC ──▶  bridge
 cookies.getAll(youtube.com)      checks origin, relays            checks every cookie again,
                                  well-formed messages only        session in memory only
```

1. In the app (Dev mode), the person chooses Chrome or Edge in the YouTube connection panel and
   presses **Connect Chrome / Edge**. The app registers its own executable as a native messaging
   host for the current Windows user and starts listening (below). It shows the folder to load
   as an unpacked extension.
2. In the browser profile, the person loads `extension/` (Developer mode → Load unpacked),
   opens the toolbar button and presses **Connect**. The extension's service worker calls
   `runtime.connectNative("com.stream_dlp.youtube_session")`, sends `hello`, and after the app
   answers `connected` reads the profile's YouTube cookies and sends them.
3. The app imports the session and retries the current link or failed download once, exactly
   like a Firefox import (`App::finish_import`).
4. While the port is open, a YouTube sign-in error during inspection or download makes the app
   send `refresh` once (the **Auto mode** setting, the same refresh budget as Firefox: one per
   inspection). The extension answers with a fresh read, at most once every 10 seconds. If no
   session arrives within 20 seconds or the connection closes, the original error is shown.
5. **Disconnect** in the extension or in the app closes the connection and removes the session
   the extension supplied. The app's Disconnect also removes the registration.

An open native messaging port keeps the MV3 service worker alive (Chrome 105+). The extension
has no `storage` permission, so after a browser restart nothing reconnects by itself; press
Connect again. If the browser closes without Disconnect, the imported session stays in the
app's memory (like a Firefox import) but can no longer be refreshed.

## Pieces

- `extension/`: Manifest V3. Permissions: `cookies`, `nativeMessaging`; host permission
  `*://*.youtube.com/*` (youtube.com and its subdomains, the same domains the app's YouTube
  session policy accepts). No content scripts, no remote code, no network requests, no analytics.
  `bridge.js` holds the logic and is tested by `bridge.test.js` with Deno and a fake `chrome`
  (part of `cargo xtask check`). It sends only unpartitioned cookies whose domain is in scope,
  and only the fields `domain, hostOnly, path, secure, httpOnly, expirationDate, name, value`.
  The fixed ID `edngekgkkjhaoohkfddejaoohfaggiac` comes from the committed public `key`; the
  private key was never kept (an unpacked extension needs only the public key; a store listing
  would get its own ID and need a new `allowed_origins` entry). A Rust test recomputes the ID
  from the key.
- `src/session/chrome/` (no egui): `framing` (frames), `messages` (the message shapes),
  `cookies` (re-filtering), `channel` (app ↔ host), `registry` (registration), `host` (host
  mode), `bridge` (the app's state and bounded refresh). The UI is `src/app/extension_panel.rs`.

## Host mode

Chrome and Edge start the host with the extension's origin as the first argument
(`chrome-extension://<id>/`) and, on Windows, `--parent-window=<n>`; they cannot pass other
arguments. So `main` enters host mode whenever the first argument starts with
`chrome-extension://`, before any window, settings or error log. The host then requires the
origin to be exactly ours and every other argument to be `--parent-window=<digits>`; otherwise it
exits with code 2 and writes nothing. By default both browsers start an `.exe` host through
`cmd.exe /d /c` with redirected pipes (direct launch only with the
`NativeHostsExecutablesLaunchDirectly` policy). The release executable is a `windows`
(GUI) subsystem program; the end-to-end test showed it speaks the protocol through inherited
pipes, both started directly and through `cmd /d /c`, and that a length prefix containing
`0A` bytes passes unchanged (no text-mode translation).

The host exits when its standard input ends (the browser closed the port; the browser also
terminates it about two seconds later) or when the app closes the channel, after telling the
extension `app_unavailable`.

## Messages

Frames are a 32-bit length in native byte order (little-endian on every supported machine)
followed by UTF-8 JSON, at most 1 MiB each way (Chrome's own limit for host messages). Both the
host and the app parse every message into these shapes and refuse unknown types and fields:

- Extension → app: `{"type":"hello","protocol":1}`, `{"type":"session","cookies":[…]}`,
  `{"type":"disconnect"}`.
- App → extension: `{"type":"status","state":"connected|imported|rejected|busy|app_unavailable"}`,
  `{"type":"refresh"}`, `{"type":"disconnect"}`.

A session before `hello`, a different protocol version, or an invalid frame closes the
connection.

## Trust boundary

The extension, the browser and the host are outside the app's trust boundary. The origin
argument is not proof that Chrome or Edge started the host: any program running as the same
user can start `stream-dlp.exe chrome-extension://edngekgkkjhaoohkfddejaoohfaggiac/` and write
frames to it. The origin check only keeps other extensions (which the browser itself labels
correctly) out. The channel secret has the same limit: it shuts out other users, not the
same user's programs. That is why the app treats whatever arrives as untrusted input,
re-validates every cookie, and only ever builds a session from YouTube's domains. At worst, a
local program could swap in a different YouTube session, which it could do anyway by writing
a cookie file for the manual import. The app:

- accepts host connections only on `127.0.0.1` and only after the channel handshake (below);
- accepts one connection at a time; a second browser or profile gets `busy` and is closed, so
  sessions are never mixed or silently switched;
- converts the cookies itself (`session::chrome::cookies`) and passes them through the same
  `SessionCookies::parse_for(…, Source::YouTube)` domain policy as every other import: other
  domains and lookalikes are dropped; expired and nameless cookies are dropped; more than 2000
  cookies, a field longer than 8 KiB, a control character in any field, or a non-finite
  expiry refuses the whole session. Error messages never contain cookie names or values;
- keeps the session in memory. Nothing from the extension is written to disk or logged; the
  helper runs get the usual disposable working file ([session files](session-files.md)).

### Channel between host and app

The app listens on a random loopback port and writes `channel.json` (port and a fresh 32-byte
secret from the system's random generator) to `%LOCALAPPDATA%\stream-dlp\native-host\`, whose
permissions only let the user, SYSTEM and administrators read it. The host reads it, connects,
and both sides prove knowledge of the secret with HMAC-SHA256 over two random nonces (the app
first, so the host sends nothing to a listener that cannot prove it, such as a stale port reused
by another user). The secret never crosses the socket. The file is removed when the app stops
listening (Disconnect or closing the app), and only if it still holds this app's secret.

## Registration

Only on Connect, for the current user:

- `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.stream_dlp.youtube_session`
- `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.stream_dlp.youtube_session`

Both default values name `%LOCALAPPDATA%\stream-dlp\native-host\com.stream_dlp.youtube_session.json`:
the running executable's absolute path, `"type": "stdio"`, and `allowed_origins` with only our
extension's origin. Connect writes them again, so a moved app is picked up. Disconnect (or
**Remove registration**) deletes both keys and the manifest. The registration survives closing
the app; while the app is not listening, the host answers `app_unavailable`.

Tests use a fake registry (`registry::Registry`) and temporary folders; they never write the real
keys. The registry writes are direct `advapi32` calls, the random bytes `BCryptGenRandom`: no new
dependency.

## Limits and leftovers

- Uninstalling stream-dlp does not remove the registration or the manifest (that needs an
  installer change, not made). Press Disconnect before uninstalling, or delete the two keys and
  the `native-host` folder by hand.
- Windows only. The code builds and its tests run on macOS; the panel is hidden there.
- Release packages do not ship `extension/`; the panel points to the folder in the source
  checkout the build came from.
- Incognito and partitioned cookies are not read. The extension uses the profile's regular cookie
  store.
- YouTube rotates account cookies on open tabs (yt-dlp's reason for its private-window export
  advice). Reading the live profile gives the browser's current cookies; whether yt-dlp keeps
  working with them, and whether the single refresh is enough, is unproven. Device Bound Session
  Credentials may also limit reuse outside the browser.
- Edge reads its own key first and falls back to Chrome's. Edge support must be accepted on its
  own; do not infer it from Chrome.

## Manual acceptance checklist

Only the person can run this; record exact Chrome, Edge, yt-dlp and stream-dlp versions, and keep
real links, titles, account names and screenshots out of the repository (AGENTS.md).

1. **Prepare.** Create a dedicated Chrome profile for testing (not your everyday profile). Sign
   in to YouTube in it with an account you accept the yt-dlp ban risk for.
2. **Connect in the app.** Run a build of this branch (`cargo run`, Dev mode is on), choose
   Chrome in the YouTube connection panel and press **Connect Chrome / Edge**. Expect the
   instructions and "Registered … with Chrome and Edge".
3. **Load the extension.** In the test profile open `chrome://extensions`, turn on Developer
   mode, **Load unpacked**, select the folder shown in the app. Check the ID is
   `edngekgkkjhaoohkfddejaoohfaggiac` and the permissions list only reading YouTube data.
4. **Connect in the extension.** Open the stream-dlp toolbar button, press **Connect**. Expect
   "Connected. stream-dlp has your YouTube session." and in the app "Extension connected" and
   "YouTube cookies imported".
5. **Signed-in inspect and download.** Inspect and download a video that needs sign-in (for
   example an age-restricted one you are allowed to watch). Record whether both succeed.
6. **Scope.** In `chrome://extensions` → Details, check that site access lists only
   youtube.com. (That only YouTube cookies are sent and kept is covered by the offline tests on
   both sides.)
7. **Refresh.** With the extension still connected, trigger a sign-in error (for example sign out
   of YouTube in another window of the test profile, or wait for rotation) and inspect again.
   Expect one "Asking the browser extension for a fresh YouTube session…", then either success
   or the original error. Record which.
8. **Disconnect from the extension.** Press Disconnect in the popup. Expect the app to drop the
   YouTube session. Connect again from the popup to restore it.
9. **Disconnect from the app.** Press **Disconnect extension** in the app. Expect the popup to
   show "stream-dlp disconnected.", the session gone, and both registry keys and the
   `native-host` folder's manifest removed (check with `reg query`).
10. **Failure messages.** With the app closed, press Connect in the popup: expect "stream-dlp is
    not reachable…". After Disconnect in the app, press Connect in the popup: expect "not
    registered".
11. **Browser restart.** Connect, close Chrome without Disconnect: the app keeps the session but
    says it cannot be refreshed. Reopen Chrome and press Connect again.
12. **Edge separately.** Repeat 1–11 in a dedicated Edge profile (`edge://extensions`). Record
    Edge results separately from Chrome.

Only after these pass on a clean Windows machine may A-03's checkboxes be ticked.
