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
  domains and lookalikes are dropped; expired and nameless cookies are dropped; a list without one of YouTube's account cookies
  (a signed-out profile, which still has visitor cookies) is refused as no session; more than 2000
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
keys, except the opt-in `live_browser_extension_session` ([below](#the-apps-side-without-the-window)). The registry writes are direct `advapi32` calls, the random bytes `BCryptGenRandom`: no new
dependency.

### What Connect creates

Everything is per user; nothing goes under `HKEY_LOCAL_MACHINE` or into the app folder:

| What | Where | Removed by |
| --- | --- | --- |
| Chrome key, default value = manifest path | `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.stream_dlp.youtube_session` | Disconnect, Remove registration, uninstall |
| Edge key, the same value | `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.stream_dlp.youtube_session` | the same |
| Host manifest | `%LOCALAPPDATA%\stream-dlp\native-host\com.stream_dlp.youtube_session.json` | the same |
| Channel file (port and secret), only while listening | `%LOCALAPPDATA%\stream-dlp\native-host\channel.json` | the app when it stops listening; uninstall if one was left |
| The `native-host` folder | `%LOCALAPPDATA%\stream-dlp\native-host\` | uninstall, only when empty |

No setting is stored: the extension has no `storage` permission and the app keeps the session in
memory. The rest of `%LOCALAPPDATA%\stream-dlp` (settings, logs) is not the extension's and the
uninstaller leaves it, as before.

### Removal

- **Windows installation.** The uninstaller's first step (`[UninstallRun]` in
  `installer/stream-dlp.iss`, before any file is deleted) runs
  `stream-dlp.exe --unregister-extension`, hidden and waited for. `main` handles it right after the
  host-mode check, before any window, settings, error log or startup sweep, and calls
  `registry::unregister_copy`: the same `unregister` as Disconnect (both keys by our host name,
  then the manifest), then a leftover `channel.json` and the folder if it is empty. One function
  decides what is removed, for the app and the uninstaller alike. Nothing registered is success
  (exit 0). A failure exits 1, which Inno Setup ignores, so the uninstall never fails because of
  it. Only keys named `com.stream_dlp.youtube_session` are deleted; other programs' hosts are not
  touched.
- **A registration of another copy is kept.** If the manifest's `path` names a different
  executable (compared without case on Windows), for example a portable copy that pressed Connect
  later, the cleanup changes nothing: that copy still exists and its registration still works.
  A manifest that cannot be read counts as ours and is removed.
- **Updates keep it.** `[UninstallRun]` runs only on uninstall, never for the setup run over an
  existing installation or the update installer, and an update keeps the executable's path, so
  the registration stays valid. The update installer is built from the same script, so its
  uninstall log carries the same entry; `RunOnceId` keeps it from running twice. Installations
  set up by 0.10.0 or earlier get the entry with their next update to a version that has it.
- **Portable ZIP.** There is no uninstaller. Press **Disconnect extension** (or **Remove
  registration**) in Dev mode before deleting the folder. If the folder is already gone, the
  keys point at a manifest naming a missing executable (Chrome then fails to start the host);
  delete them by hand:

  ```powershell
  reg delete "HKCU\Software\Google\Chrome\NativeMessagingHosts\com.stream_dlp.youtube_session" /f
  reg delete "HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.stream_dlp.youtube_session" /f
  Remove-Item -Recurse "$env:LOCALAPPDATA\stream-dlp\native-host"
  ```

- **macOS.** The flow does not exist there: the panel is hidden and `registry::UserRegistry`
  refuses every call off Windows, so Connect is never offered and nothing is registered. Chrome
  on a Mac would read a manifest file instead of a registry key, per user in
  `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/`
  ([Chrome docs](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging));
  if the Mac build ever gains the flow, it must write and remove that file, and dragging the app to
  the Trash would not remove it.

`registry.rs` tests the cleanup with the fake registry: this copy's registration, channel file
and empty folder go while another host's key stays; a manifest naming another executable is left
alone; an unreadable manifest is removed but unknown files in the folder are kept; repeating with
nothing registered succeeds. The uninstaller itself can only be exercised by CI's Package check
(AGENTS.md forbids running installers on the development machine).

## Limits and leftovers

- Uninstalling removes the registration ([Removal](#removal)); deleting a portable folder does
  not, so press Disconnect first.
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

## Distribution and maintenance estimate (November 20, 2026)

A written estimate for A-03's second checkbox. Store requirements were read from the stores' own
pages on this date (sources at the end); no account, listing or key was created. Effort figures
are rough ranges for one developer who knows this code, and exclude waiting for reviews.

### Option A: ship `extension/` in the release packages (load unpacked)

What it takes:

- Copy `extension/` into the portable folder and ZIP (`build.ps1`) and the installer
  (`installer/stream-dlp.iss` `[Files]`, both of which need the user's approval under AGENTS.md),
  and point the panel's "select this folder" path at the copy next to the executable instead of
  the source checkout (`extension_panel::extension_folder`). Add a package-check assertion that
  the folder is present. About half a day.
- The ID stays `edngekgkkjhaoohkfddejaoohfaggiac` (the committed public `key`), so the host
  manifest and the host's origin check do not change.

What it costs the person: Developer mode must be on in the browser profile, and Chrome and Edge
warn about developer-mode extensions. An unpacked extension never updates itself: after an app
update that changed `extension/`, the person must press Reload on `chrome://extensions`; if the
message protocol changed, the app refuses the old extension's `hello` until then. A managed browser can forbid Developer mode by policy.
Chrome 157 ignores `--load-extension` ([partial run](#partial-run-in-a-real-chrome-november-17-2026)),
so the app cannot load it for the person.

Ongoing: a release note line when `extension/` changes, and keeping `PROTOCOL` in
`messages.rs` and `bridge.js` in step (a test already checks they match).

### Option B: Chrome Web Store and Edge Add-ons listings

What it takes, per store:

- **Accounts.** Chrome Web Store: a one-time registration fee whose amount the registration page
  does not state (it was US$5 when introduced; Chromium blog, 2020), and 2-Step Verification on
  the account before publishing. Edge Add-ons: a Partner Center account, no registration fee;
  an individual account is verified quickly, a company account in days to weeks.
- **Listing and privacy tab.** Both stores require a narrow single-purpose description, a
  justification for each permission (`cookies`, `nativeMessaging`, `*://*.youtube.com/*`), a
  remote-code declaration (none: MV3 forbids it and the extension has none), data-use
  disclosures with a Limited Use certification, and a privacy policy. The Chrome Web Store
  requires a policy whenever an item handles user data, and it treats authentication data
  (YouTube's account cookies are exactly that) as data to keep secure. Edge requires a policy in
  general and asks that a dependency on other software (here the stream-dlp app) be disclosed in
  the description. The project has no hosted privacy policy today; it would need one at a stable
  URL that says the cookies go only to the local app, stay in memory and are never sent
  elsewhere, plus the Limited Use statement the Chrome Web Store asks for on a website. Store
  graphics, descriptions (Edge: 250 to 10,000 characters) and a listing per store.
- **Review.** Chrome Web Store: "within a few days, but it can take up to a few weeks"; a new
  developer, a new item, and `cookies` combined with host permissions are named as reasons for
  closer review. Edge: certification up to seven business days. Every update is reviewed again.
- **Policy risk, the largest unknown.** Both stores forbid extensions that enable unauthorized
  downloading of copyrighted media and access that gets around login restrictions, and the
  Chrome Web Store lists video downloaders among items it may keep out of featuring. The
  extension does not download anything itself, but its single purpose is to hand a signed-in
  YouTube session to a downloader. A rejection is plausible and could not be fixed by changing
  code; repeated violations can end the developer account.
- **The ID changes.** A listing has its own item ID, and each store assigns a different one. The
  Chrome docs keep one ID across development and the store by uploading the package unpublished,
  copying the store's public key into the manifest's `key`; the private key behind the current
  `key` was not kept, so the store ID can never be `edngekgkkjhaoohkfddejaoohfaggiac`. Either
  switch the committed `key` to the Chrome Web Store's (the unpacked ID then matches the Chrome
  listing) or keep both. Edge's listing gets a third ID in any case. `allowed_origins` takes no
  wildcards, so the manifest must list every ID, and `host::caller_allowed`, `origin()` and the
  ID test must accept a fixed list instead of one origin. About a day of code and tests, plus a
  migration note for anyone with the old unpacked ID.

First listing effort, both stores: two to four days of work (privacy policy page, listings and
graphics, the ID change, store packaging in `build.ps1` or a release step, a CI check that the
packaged manifest's ID is in `allowed_origins`), then one to three weeks of review, more if a
rejection must be answered.

### Ongoing maintenance (either option, more for B)

- **Chromium and MV3 changes.** The design rests on the service worker staying alive while a
  native port is open (Chrome 105+), on `cookies.getAll` returning unpartitioned cookies, and on
  native messaging starting the host through `cmd.exe`. Cookie partitioning, Device Bound Session
  Credentials and YouTube's rotation may change what the extension can read or what yt-dlp can
  use. Check each change against the manual checklist; budget a few hours per Chrome major
  version that touches extensions or cookies, plus the same for Edge.
- **Store policy reviews (B).** Each update goes through review; policy updates can require new
  disclosures or remove the listing. Answering a takedown or warning is unplanned work with a
  7- to 30-day deadline in the Chrome Web Store. Keep 2-Step Verification and the account owner's
  access current.
- **IDs and allowed origins.** Any new listing or key change must be added to
  `allowed_origins`, the host's origin list and its tests in the same release; an app older than
  the extension rejects it as an unknown origin. A test should compare the list with every
  shipped manifest.
- **Privacy policy (B).** Update it whenever what the extension reads or sends changes.

### Recommendation

Keep A-03 a Dev mode prototype and do not make it a release commitment until a person has run the
[manual acceptance checklist](#manual-acceptance-checklist) in Chrome and Edge on a clean
machine. Nothing yet shows that yt-dlp keeps working with cookies read from a live profile, and
that is the whole value of the feature. If the checklist passes, take option A first (half a day,
no store accounts, no policy review, the ID unchanged) for Dev mode users only. Consider option B
only after option A has shown the flow keeps working across several YouTube cookie rotations and
browser versions, and only with a decision on the policy risk above. It adds two to four days
up front, recurring store reviews, a public privacy policy and the ID migration, and may be
rejected for reasons code cannot fix.

Sources (read November 20, 2026):
[Chrome Web Store registration](https://developer.chrome.com/docs/webstore/register),
[Chrome Web Store program policies](https://developer.chrome.com/docs/webstore/program-policies/policies),
[Chrome Web Store review process](https://developer.chrome.com/docs/webstore/review-process),
[Chrome Web Store privacy fields](https://developer.chrome.com/docs/webstore/cws-dashboard-privacy),
[manifest `key`](https://developer.chrome.com/docs/extensions/reference/manifest/key),
[native messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging),
[Chromium blog, March 2020](https://blog.chromium.org/2020/03/new-developer-dashboard-and.html),
[Edge developer account](https://learn.microsoft.com/en-us/microsoft-edge/extensions/publish/create-dev-account),
[Edge publishing](https://learn.microsoft.com/en-us/microsoft-edge/extensions/publish/publish-extension),
[Edge Add-ons developer policies](https://learn.microsoft.com/en-us/legal/microsoft-edge/extensions/developer-policies).

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

Only after these pass on a clean Windows machine may A-03's first checkbox (the evaluation) be
ticked. The second, the estimate, is done ([above](#distribution-and-maintenance-estimate-november-20-2026)).

### The app's side without the window

`live_browser_extension_session` (ignored; run it by name) does what **Connect Chrome / Edge**
and **Disconnect extension** do, without the window: it registers the built app with the real
keys, listens in the real `native-host` folder for the given number of seconds, prints each
state change (connected, session imported or refused with its message, cleared, refresh
requested and failed; never cookie data), asks for one refresh 12 seconds after an import, then
disconnects and removes the registration. Use it for steps 2, 4 and 7–9 while you work in the
browser:

```powershell
cargo build --locked --bin stream-dlp
$env:STREAM_DLP_EXTENSION_LIVE_SECONDS = '120'
cargo test --locked live_browser_extension_session -- --ignored --nocapture
Remove-Item Env:STREAM_DLP_EXTENSION_LIVE_SECONDS
```

Run it from an ordinary terminal, and not while a stream-dlp on the machine is connected to the
extension. Do not run it from a terminal inside a packaged (MSIX) app, such as the Claude
desktop app: in the run below, the keys written there were visible to that terminal but not to
Chrome, which answered "Specified native messaging host not found." (packaged apps get a
private view of the `Software` key under `HKEY_CURRENT_USER`; not confirmed from outside).

### Partial run in a real Chrome (November 17, 2026)

Chrome 157.0.8103.79 on Windows 11, a throwaway profile (its own `--user-data-dir`, never
signed in, deleted afterwards), driven through the DevTools protocol by a coding agent:

- Chrome 157 ignores `--load-extension`. The folder loaded through the DevTools command
  `Extensions.loadUnpacked` (Chrome started with `--remote-debugging-pipe` and
  `--enable-unsafe-extension-debugging`) and got the ID `edngekgkkjhaoohkfddejaoohfaggiac`;
  **Load unpacked** on `chrome://extensions` was not used.
- The popup opened and showed "Not connected."; its service worker started.
- Without a registration, **Connect** showed "stream-dlp is not registered with this
  browser…" (step 10's second half). Chrome's own error was "Specified native messaging host
  not found.", which `bridge.js` maps to that message.
- Nothing further could be run: the agent's session was inside a packaged app (above), so
  the registration it wrote never reached Chrome. Steps 2, 4, 5 and 7–11 with a real browser,
  every signed-in step, and Edge are still open.
