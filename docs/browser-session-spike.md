# R-01: browser-session feasibility

Decision recorded October 21, 2026. Status: native Firefox import implemented as
experimental; full live-browser acceptance remains open. Chrome/Edge retain the
guided export path.

## Native implementation follow-up

At the user's request, `src/browser_session.rs` implements a different data path
from the CLI experiment below: Windows SQLite opens the selected Firefox database
read-only and selects only YouTube-domain cookies from the normal unpartitioned
context. No broader database copy, all-site export, Python runtime, or yt-dlp
browser extraction is used. Expired cookies are skipped; Firefox schemas 16/17
have millisecond expiry conversion. Unknown schemas fail with a manual fallback.
The query has a progress callback for cancellation and a 15-second time budget,
with a 100-ms busy timeout. SQLite/OS file-open latency is not a hard process-level
deadline. No browser cookies or source preferences are persisted by this feature.

The dialog selects the default Firefox profile when available and offers direct
import, refresh, removal, and manual fallback. A successful import retries the
current inspection or failed download. Auto mode can refresh only the connected
profile once after an authentication error; repeated failures require user action.

Verification on this machine: the user explicitly authorized their Firefox
profile. The native importer read usable YouTube cookies while Firefox was open;
the opt-in test printed no values/account details and verified disposal of its
scoped working file. Sandboxed access failed; the same test passed under normal
user permissions. This proves local extraction, **not successful authenticated
YouTube downloading**. The Windows browser UI automation tool refused Firefox
control, so the full browser UI path was not tested. Native app UI verification
did succeed: the new dialog displayed the default profile and Auto mode, and
clicking **Import from Firefox & retry** loaded the session without a file picker.
The updated portable app was left open with that session loaded for user testing.
Windows 10, closed-browser
behavior, and clean-machine acceptance still need live validation.

Deterministic tests cover schemas 15/16/17, active WAL reads, read-only enforcement,
lookalike/unrelated domains, container exclusion, expiry, missing/empty/new-schema
databases, locked databases, cancellation, deadline handling, record injection,
and no secret values in errors. The existing local inspect/download/merge/cancel
pipeline also passed. Real profile access remains an ignored, explicit-path test:

```powershell
$env:STREAM_DLP_FIREFOX_PROFILE = 'PATH_TO_EXPLICITLY_AUTHORIZED_PROFILE'
cargo test --locked authorized_firefox_profile_import -- --ignored
Remove-Item Env:STREAM_DLP_FIREFOX_PROFILE
```

The sections below preserve the earlier CLI feasibility findings. Statements about
the then-unimplemented helper and manual-only support describe that earlier spike;
the native implementation above supersedes those product decisions.

## Evidence from this machine

Windows NT build 26300, installed yt-dlp **2026.08.19**, SHA-256
`66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a`
(matches `tools/versions.json`). Installed browser executable versions: Firefox
158.0, Chrome 155.0.8059.85, Edge 155.0.4283.62. These are inventory only: none of
these browsers was launched or its real profiles read. No Windows 10 run occurred.

`scripts/browser_session_spike.py` creates disposable SQLite fixtures and invokes
the bundled executable with argument arrays:

```text
yt-dlp.exe --ignore-config --no-progress --cookies-from-browser firefox:ABSOLUTE_SYNTHETIC_PROFILE --cookies TEMP_EXPORT
```

There is deliberately no URL. The script accepts executable paths, never profile
paths. All profile paths come from its own temporary directory. It suppresses raw
helper logs and reports only version/hash, status, and synthetic-cookie counts.
It checks the source databases remain unchanged and temporary cleanup succeeds.

Nine cases passed on the installed helper:

- Firefox synthetic default and secondary profile paths (including spaces and
  Unicode): exported all three synthetic domains, including `example.com` and
  `youtube.com.evil.test`.
- Firefox database schemas 15 and 17: future expirations survived correctly;
  schema 17 uses milliseconds in SQLite and seconds in the Netscape export.
- Empty Firefox database: successful empty export; this does not establish a
  signed-in session.
- Expired Firefox cookies: all three expired cookies remained in the export;
  parsing/exporting is not authentication validation.
- Missing explicit profile for Firefox, Chrome, and Edge: exit 1, no export.
- Firefox database opened with Windows sharing denied: exit 1, no export.

Successful exports returned **exit 2**, because the CLI subsequently complained
about the absent URL. A nonzero status alone therefore cannot distinguish export
failure. Failure cases returned exit 1. A future importer needs a deliberate
protocol for success, empty sessions, warnings, and partial decryption.

The latest official release metadata returned candidate **2026.10.21**, with
`yt-dlp.exe` digest
`33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555`.
Its GitHub release download timed out. The candidate was **not run**, and the
installed helper was not replaced. Rerun the probe against a verified candidate
before drawing a version comparison.

## Data-path decision

Direct `--cookies-from-browser` on a media request cannot enforce our YouTube-only
import policy: upstream reads the selected profile's broader cookie jar, and the
CLI export demonstrably contains unrelated cookies. Ordinary cookie matching is
not an import allowlist. Do not attach this raw export to a network operation.

The upstream Python API `extract_cookies_from_browser` could support a separate
offline helper: explicit selected profile → extraction → exact `youtube.com` or
subdomain allowlist → validated Netscape serialization → existing
`SessionCookies::load` → in-memory session → disposable per-operation file.
The existing Rust parser already rejects lookalike domains and the local pipeline
checks that YouTube cookies do not go to localhost.

This helper path is a **proposal, not proven support**. Upstream extraction copies
the SQLite database into a temporary directory before reading it; an in-memory
filtered return does not eliminate broader temporary data. Before adopting it,
verify Windows ACLs, subprocess output isolation, timeout/cancellation, partial
decryption detection, and cleanup on success, failure, and termination. Normal
temporary-directory cleanup alone is insufficient evidence of crash recovery.
The Python API also requires a separately packaged/pinned runtime or equivalent
helper; the current portable executable cannot be assumed to expose that API.

No supported private/incognito-session extraction mechanism was established.
Upstream explicitly distinguishes regular-profile extraction from the recommended
private-window export. Retain the existing manual private-window export guide.

## Supported behavior and fallbacks

- Supported today: user-selected Netscape YouTube export, through the existing UI.
- Firefox automatic import: experimental candidate; synthetic extraction works,
  authenticated connect-and-retry and real open/closed browser cases untested.
- Chrome/Edge automatic import: unverified; missing-profile handling alone proves
  neither extraction nor Windows decryption support.
- Missing profile: let the user select the intended dedicated profile or import
  manually; never fall back to another profile silently.
- Locked database: ask the user to close that browser and retry, with manual
  import available. Do not terminate their browser.
- Decryption failure or partial decryption: fail with manual import available;
  never infer success from an export containing some cookies.
- Empty/signed-out profile: sign in in the browser or use a manual export. An empty
  jar must not be labeled connected.
- Expired/challenged session: request a fresh export; a valid file is not proof of
  access. A future automatic flow gets at most one refresh/retry.
- Private session: use the existing private-window export instructions.
- Timeout/cancellation: report the stopped import, discard its working data, and
  preserve the manual option. This lifecycle remains to be implemented/tested.

## Reproduction and remaining acceptance

Python 3.12 standard library was used; no Python packages are needed:

```powershell
python scripts/browser_session_spike.py --yt-dlp tools/yt-dlp.exe
python scripts/browser_session_spike.py --yt-dlp tools/yt-dlp.exe --yt-dlp PATH_TO_VERIFIED_CANDIDATE
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

The probe fails if the observed export/status behavior changes or a source
database changes. It is a synthetic compatibility probe, not a supported importer.
Its scratch data is synthetic; it is not suitable for testing real secrets or
proving restricted ACLs. On this machine Python temporary-file access required an
unsandboxed run. A successful run confirmed scratch-directory deletion.

Still required, on dedicated test accounts/profiles only:

- Windows 10 and Windows 11, installed and verified candidate helper versions.
- Each browser's default and secondary profiles, open and closed, signed in and
  signed out; record browser/version/status only, never values/account identities.
- Actual Chromium decryption failure/partial success and current encryption.
- A scoped helper with verified access restrictions and lifecycle handling.
- Authenticated YouTube inspect/download/retry, including expired sessions.

Do not mark the R-01 live matrix complete based on these SQLite fixtures.

## Upstream sources checked

- [Cookie FAQ](https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp):
  browser extraction/export includes all sites.
- [YouTube export guidance](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies):
  regular-profile extraction does not capture the recommended private session.
- [Cookie extraction source](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/cookies.py):
  browser selection, temporary database copy, Firefox schema conversion.
- [Windows decryption issue](https://github.com/yt-dlp/yt-dlp/issues/10927):
  reason to validate Chromium separately; not evidence of a local failure.
- [Official latest-release metadata](https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest):
  candidate selection and digest. The API response can change after this record.
