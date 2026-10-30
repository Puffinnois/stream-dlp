# Chrome automatic session options

Saved October 21, 2026, for future work on roadmap task A-03.

Status: options researched, **no Chrome automatic integration implemented**.
The current app offers Firefox automatic import and guided Chrome/Edge export.
The user requested preserving these options; this record does not commit to an
implementation or authorize installing an extension or changing browser settings.

## Desired experience

The user chooses Chrome, connects their profile once, and the app handles session
import, bounded refresh, and retry with as little repeated work as possible.
Keep access limited to YouTube, sessions in memory, and manual import available.

## Option 1: companion extension and native messaging

Recommended first prototype. The user installs our extension once in the desired
Chrome profile and clicks **Connect to stream-dlp**. The extension uses Chrome's
cookies API with YouTube-only host permissions, then passes the scoped session to
the app through native messaging. No cookie export file is needed. Automatic
refresh from that connected profile could run while Chrome and the extension are
available, with bounded retries and an explicit disconnect option.

Chrome provides supported cookie and native messaging APIs. Their existence does
not establish reliable YouTube authentication; that needs a working prototype.

Costs and open questions:

- Maintain and distribute an extension, including store review and updates.
- Register a native messaging host on Windows, preferably per user. This adds
  setup beyond the current portable folder; account for moved app paths and
  uninstall cleanup.
- Restrict the host to our extension's exact ID and validate messages and cookie
  domains again in the app. Do not expose an unauthenticated local import endpoint.
- Define how the host communicates with an already-running app and how the app
  requests refresh. Chrome launches the native host; installing an extension does
  not by itself create a working app-initiated connection.
- Bind the connection to the profile where the user installed/connected the
  extension. Never silently switch profiles or mix their sessions.
- Test extension lifecycle, Chrome closed/reopened, disconnect, expired sessions,
  missing host, and Chrome updates.
- Treat incognito as a separate opt-in workflow requiring explicit browser
  permission and cookie-store selection; do not assume regular-profile access
  captures a private-window session.

## Option 2: dedicated Chrome profile/window

The app launches a separate Chrome data directory for stream-dlp. The user signs
into YouTube there themselves. Investigate whether a controlled browser connection
can supply scoped sessions and refresh them automatically.

This avoids a companion extension but requires a separate browser profile and
sign-in. Chrome's remote debugging switches require a non-default data directory;
they cannot be assumed to attach to the user's normal default profile. Keep any
control channel private to the app. This is a feasibility candidate, not a proven
supported YouTube workflow. Do not replace it with an embedded Google sign-in
window without separately investigating compatibility and applicable requirements.

## Option 3: assisted export and automatic file import

Use an existing cookie-export extension. After the user explicitly selects an
export location or file, the app could detect a new export, validate/filter it,
import it, and retry automatically. The user still clicks **Export** when refreshing
their session, but avoids repeatedly locating the file in the app.

This is the smallest change to the current guided workflow. It still materializes
a credential file, so watch only the explicitly selected source; handle partially
written files and duplicate events, and never scan arbitrary Downloads files.
Preserve the original export unless the user explicitly requests its removal.

## Constraints and decision

Direct Chrome database import is not the preferred default: Windows App-Bound
Encryption binds protected cookie data to Chrome's application identity. Firefox's
native SQLite importer cannot simply be reused for encrypted Chrome cookie values.
Closing Chrome may resolve a file lock without resolving decryption.

Readable/exportable cookies also do not prove that yt-dlp can reuse a session.
Device Bound Session Credentials introduce browser/device-mediated renewal, so
test the actual authenticated inspect/download/refresh flow rather than declaring
support from successful extraction alone.

Recommendation to revisit: prototype **option 1**, retaining **option 3** as the
fallback. Consider **option 2** if extension distribution is unacceptable. No final
choice has been made by the user.

## Resume checklist

1. Recheck current Chrome APIs, Windows cookie protection, yt-dlp behavior, and
   YouTube session guidance; these can change after this record.
2. Confirm which option to build and the permitted setup/distribution model.
3. Use a dedicated test profile first. Access a regular profile only when the user
   explicitly selects/authorizes it.
4. Prove YouTube-only scoping, authenticated inspection/download, bounded refresh,
   cancellation, cleanup, disconnect, and actionable fallback.
5. Record exact browser/helper versions and separate Chrome results from Edge
   results. Do not infer Edge support solely from shared Chromium APIs.

## Sources checked October 21, 2026

- [Chrome cookie API and host permissions](https://developer.chrome.com/docs/extensions/reference/api/cookies)
- [Chrome native messaging and Windows host registration](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
- [Chrome remote debugging restrictions](https://developer.chrome.com/blog/remote-debugging-port)
- [Google: App-Bound Encryption on Windows](https://security.googleblog.com/2024/07/improving-security-of-chrome-cookies-on.html)
- [Google: Device Bound Session Credentials](https://blog.google/security/protecting-cookies-with-device-bound-session-credentials/)
- [yt-dlp YouTube session export guidance](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies)
