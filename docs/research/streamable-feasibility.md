# R-02: Streamable sharing feasibility

Decision recorded October 21, 2026. Select **browser-assisted sharing** for S-02.
Direct account and anonymous uploads (S-03/S-04) remain blocked on a supported
upload integration. The decision is complete; the signed-in website acceptance
case remains open. No application sharing feature is implemented by this spike.

## Integration decision

The [official API documentation](https://streamable-support.zendesk.com/hc/en-us/articles/35415672400916-API-Documentation)
describes playback metadata access and explicitly excludes upload, clipping, and
editing. Native-player approval concerns playback, not upload authorization.
No supported upload authorization flow was identified in these public documents.
Account credentials do not unblock direct uploads, and guest website uploads do
not establish an anonymous API. Do not implement historical upload endpoints or
automate private website requests. No vendor contact was made.

S-02 should open the website in the user's browser and reveal the selected local
file in Explorer. The user completes selection, upload, and any login there.
Report that the browser and file were opened; never infer upload completion.
An optional user-pasted resulting URL can be opened/copied independently.
Preserve the selected file and successful download result on cancellation/failure.
Keep cookies, account credentials, and website automation out of this handoff.

## Website evidence

Test environment: Windows, Codex in-app browser, guest session. Nonessential
cookies were rejected. No Streamable account was created or signed in. The user
explicitly approved the synthetic upload and acceptance of upload terms, then
separately approved publishing the selected existing download to Streamable.

The live landing page offered file selection and drag/drop without an account.
After the synthetic upload the dashboard displayed a ready notification, a
thumbnail, and a share link. Opening that link exposed a video player; after Play,
the video element reported 2 seconds, 320x180, readyState 4, advancing currentTime,
and no media error. This establishes actual playback, not just link allocation.
Audio listening and a separate fresh browser context were not tested.

The generated sample is `test-output/streamable-r02/synthetic-smoke.mp4`:
75,355 bytes, 2 seconds, H.264/AAC MP4, generated test pattern and tone only.
SHA-256: `7183bde26435fa67046d95f88ea11114495a5d535e84fbb311efaac79eb70e31`.
Its temporary public link is <https://streamable.com/mt3cza>. Screenshot:
`test-output/streamable-r02/upload-ready.png`. Test artifacts are git-ignored.

The existing-file test uses the user's selected Downloads/StreamSave source:
`The NBA is back… [ur2lJ511JfQ] [698+140-7].mp4`, 7,307,318 bytes,
28.560544 seconds, 720x1280 AV1 video with AAC audio in MP4. The original stays
in place. Local metadata was checked with the bundled FFprobe.

The existing file uploaded and processed successfully without app-side conversion.
Its public link is <https://streamable.com/8yeen3>. After Play, the website video
reported 28.56 seconds, 720x1280, readyState 4, advancing currentTime, and no media
error. Screenshot: `test-output/streamable-r02/existing-video-playback.png`.
This verifies one AV1/AAC source with a filename containing spaces, emoji, `#`,
and brackets; it does not establish support for every AV1 file. No source file
was moved, modified, or deleted. Both uploads remain subject to guest expiration.

## Limits and compatibility

The [free-plan article](https://streamable-support.zendesk.com/hc/en-us/articles/35415717293204-Free-Plan-Limits)
documents 250 MB and 10 minutes per free upload, and 90-day storage for free
accounts. It describes anonymous archiving after 24 hours, but the **live guest
dashboard and uploaded video card both said expiration after 2 days**. Retention
has not been observed over time. Show that guest links are temporary and refer
users to current website limits; do not promise either duration as a guarantee.
Paid limits were not tested and should not become app-side acceptance rules.

[Recommended video settings](https://streamable-support.zendesk.com/hc/en-us/articles/35415480104340-Recommended-Video-Settings)
lists several accepted containers and recommends H.264 MP4, High Profile level
5.2 or lower. This is a recommendation, not proof that other codecs fail. Probe
files in S-01 and offer the original first; do not automatically transcode all
downloads or treat these recommendations as hard validation failures. S-05
remains conditional on an observed compatibility need.

[Uploading Basics](https://streamable-support.zendesk.com/hc/en-us/articles/35415719714324-Uploading-Basics)
describes the signed-in dashboard, file selection/drag-drop, and upload followed
by processing. [Sharing Your Content](https://streamable-support.zendesk.com/hc/en-us/articles/35415662302484-Sharing-Your-Content)
documents copying the video-page link from the dashboard. Browser assistance
can use either the observed guest flow or an existing account; do not require
login in app copy when the guest option remains available.

## Remaining acceptance

- Test a signed-in website upload with an explicitly selected test account; no
  account was available in this run. Record its actual limits and resulting link.
- Recheck retention and file requirements before shipping, especially the
  conflicting guest-retention statements. Boundary limits were not exercised.
- S-01/S-02 still need application implementation and tests for new/existing files,
  Unicode paths, missing files, Explorer/browser launch errors, cancellation,
  and optional pasted-link handling. Website success alone does not satisfy
  application acceptance or remove F-02/S-01 dependencies.
- Reopen direct-upload work only when official documentation or an approved
  integration supplies an upload endpoint and authorization model. Contacting
  Streamable requires separate user authorization.
