# Usage reports — reference

The Mac version is hard to test without Macs, so a Mac user can agree to send counts of how the app is working. Windows copies never ask and never send, and write no files for this.

## The flow

1. On the first launch a modal asks **Share reports** or **No thanks**: two equal buttons and no default. It lists what is sent. Nothing leaves the Mac before a yes.
2. After a yes, the app counts events, saves the counts to `usage.json` in the data folder, and sends them in the background 10 seconds after agreeing or launching, then at most every 5 minutes. After a network error, or while the service cannot take them, it waits 15 minutes and keeps the counts.
3. **Settings → Usage reports** turns this off, and shows what is sent and what is waiting. Turning it off deletes everything waiting. A request already on its way cannot be recalled.
4. Developers read totals in **DEV chip → Usage reports**, with the Inbox's admin token.

A person who never answers is asked again at the next launch. Anything counted before an answer stays in memory, apart from the two files below.

## What a report carries

The app version, operating system, major macOS version and chip generation (`m1`, not the model), then up to 32 lines of event, subject, result and count:

| Event | Subject | Result |
| --- | --- | --- |
| `started` | none | `first`, `clean`, `unclean` (the last session did not close normally) |
| `helper` | `yt-dlp`, `ffmpeg`, `ffprobe`, `deno` | `ok`, `failed`; tried once per app version |
| `inspect`, `download` | the site | `ok`, `cancelled`, or the kind of error |
| `page` | the page | `opened` |
| `folder_picker` | none | `picked`, `dismissed` |
| `cookie_import` | none | `ok`, `failed` |
| `crash` | a place like `src/app/download.rs:120`, or `dependency` | `panic` |

These come from the types in `src/usage.rs`, and the service refuses any other field or value, so a link, title, path or message cannot be added by accident. A crash keeps only the place, and only if it is in this app's own source. A dependency's location is a path on the build machine, so it becomes the word `dependency`, and the panic's message is never kept.

There is no install ID, so reports from one person cannot be joined together.

## What stays on the Mac

| File in the data folder | What it is |
| --- | --- |
| `usage.json` | The choice, and the counts waiting to be sent. Counts from another app version are dropped. |
| `usage-running` | Exists while the app runs. Finding it at launch means `started: unclean`. A force quit counts too, and a second running copy can cause a false one. |
| `usage-crashes.txt` | The version and place of up to five panics, kept until the next launch with a yes, so a crash before the question can still be reported. |

The last two are created before the person answers, because the first launch is what most needs reporting. They hold no personal details, are never sent without a yes, and are deleted on a no.

## What it cannot tell you

- A copy macOS blocks, or that crashes before the question, sends nothing. Few reports do not show why.
- A session under 10 seconds sends at its next launch, so a person who gives up and never reopens the app is not counted.
- Reports cannot see broken text or layout, or whether a video plays. After `unclean` the app offers **Tell us what happened**, and an error on the Download page has the same button; both open Send feedback.

## On the service

`POST /v1/usage` in `services/feedback/worker.js` validates every value, then adds to `usage_daily`, a table of totals per day, version, system, event, subject and result. No row in it links to a sender, so no individual report is stored.

| Risk | What the service does |
| --- | --- |
| Flooding | The same Cloudflare limiter as notes, then 20 reports per 10 minutes and 300 a day per sender; invalid requests count |
| Filling the database | 1000 reports a day in all, 32 lines each, then it answers "busy" |
| Inflated counts | A line's count is at most 1000. Someone with many addresses can still send false numbers, so read the totals as a guide |

Blocked senders get the usual answer and nothing is stored. Reports share the database with notes, so a flood could use up the daily writes for both.

To count people roughly, `usage_senders` holds the keyed hash of each sender's address for the current day. The daily clean-up turns finished days into one number in `usage_reach` and deletes those rows. People sharing an address count once. Totals and daily numbers are deleted after a year.

## Deploying

Do this before releasing an app with this feature. An app that reaches an older service keeps its counts and tries again later, but the service has to come first to receive anything. From `services/feedback/`:

1. `npx wrangler d1 execute stream-dlp-feedback --remote --file=schema.sql`. It only adds the new tables.
2. `npx wrangler deploy`

## Trying it locally

Start the local service as in [public-feedback.md](public-feedback.md). To see the question and the Settings card on any system, start a debug build with `STREAM_DLP_FEEDBACK_URL=http://127.0.0.1:8787` and `STREAM_DLP_USAGE_TEST_DIR` set to an empty folder, which is used instead of the data folder (release builds ignore both). With the server running, `cargo test --locked a_report_reaches_the_local_service -- --ignored` sends a report and reads it back.

## Code

| Where | What |
| --- | --- |
| `src/usage.rs` | The event types, the counts and their files, sending, the crash notes, the summary request |
| `src/app/usage.rs` | The question, the Settings card, the offer to write a note, the summary window |
| `src/dev/errors.rs` | The panic hook, which calls `usage::note_panic` |
| `services/feedback/worker.js`, `schema.sql` | `/v1/usage`, `/v1/admin/usage` and the clean-up |
