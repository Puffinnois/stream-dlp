// The public feedback service: the app posts a note here and Dev mode's Inbox reads it. Anyone
// can call this without the app, so every rule is enforced here and every field is checked.
// See docs/reference/public-feedback.md.

const MAX_BODY = 8 * 1024;
export const TEXT_MIN = 15;
export const TEXT_MAX = 4000;
const LETTERS_MIN = 10;
const DISTINCT_MIN = 5;
const LINKS_MAX = 2;
const MINUTE = 60;
const DAY = 24 * 60 * MINUTE;
const SENDER_LIMITS = [
  { name: "10min", seconds: 10 * MINUTE, limit: 5 },
  { name: "day", seconds: DAY, limit: 20 },
];
// When either is reached the service stops taking new notes, so a flood costs a day of feedback
// at most and never fills the database.
const NEW_NOTES_PER_DAY = 200;
const PENDING_MAX = 1000;
const KEEP_SECONDS = 90 * DAY;
const BLOCK_SECONDS = 365 * DAY;
const ADMIN_TOKEN_MIN = 32;
const ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const VERSION = /^\d{1,3}\.\d{1,3}\.\d{1,4}$/;
const TAGS = new Set(["bug", "idea", "polish"]);
const CONTEXT = {
  page: new Set(["download", "videos", "gif", "settings"]),
  os: new Set(["windows", "macos", "linux"]),
  phase: new Set(["idle", "inspecting", "choosing", "picking", "downloading", "done"]),
  source: new Set(["youtube", "tiktok", "x", "facebook", "instagram"]),
  error: new Set([
    "unsupported", "sign_in", "rate_limited", "unavailable", "restricted", "network",
    "timed_out", "expired_session", "missing_video", "drm", "helper", "other",
  ]),
};
// Usage reports, which the app sends only for someone who agreed to them. Every value is a fixed
// word, so a report cannot carry a link, a title, a path or a message. The caps keep a flood below
// the database's daily write allowance.
const USAGE_SENDER_LIMITS = [
  { name: "10min", seconds: 10 * MINUTE, limit: 20 },
  { name: "day", seconds: DAY, limit: 300 },
];
const USAGE_BATCHES_PER_DAY = 1000;
const USAGE_EVENTS_MAX = 32;
const USAGE_COUNT_MAX = 1000;
const USAGE_KEEP_SECONDS = 365 * DAY;
const USAGE_DAYS_MAX = 90;
const OS_VERSION = /^\d{1,3}$/;
const CHIP = /^(m\d{1,2}|other)$/;
const CRASH_PLACE = /^(src\/[a-z0-9_/]{1,80}\.rs:\d{1,6}|dependency)$/;
const OUTCOME = new Set(["ok", "cancelled", ...CONTEXT.error]);
const noSubject = (subject) => subject === undefined;
// For each kind of event, which subject and which result it may have.
const USAGE_EVENTS = {
  started: { subject: noSubject, result: new Set(["first", "clean", "unclean"]) },
  helper: {
    subject: (subject) => ["yt-dlp", "ffmpeg", "ffprobe", "deno"].includes(subject),
    result: new Set(["ok", "failed"]),
  },
  inspect: { subject: (subject) => CONTEXT.source.has(subject), result: OUTCOME },
  download: { subject: (subject) => CONTEXT.source.has(subject), result: OUTCOME },
  page: { subject: (subject) => CONTEXT.page.has(subject), result: new Set(["opened"]) },
  folder_picker: { subject: noSubject, result: new Set(["picked", "dismissed"]) },
  cookie_import: { subject: noSubject, result: new Set(["ok", "failed"]) },
  crash: {
    subject: (subject) => typeof subject === "string" && CRASH_PLACE.test(subject),
    result: new Set(["panic"]),
  },
};
// Control characters except tab and newline, and invisible or text-reversing characters, which
// could hide what a note really says from the person reviewing it.
const HIDDEN = new RegExp(
  `[${[
    [0x00, 0x08], [0x0b, 0x0c], [0x0e, 0x1f], [0x7f, 0x9f], [0xad, 0xad],
    [0x200b, 0x200f], [0x2028, 0x202e], [0x2060, 0x206f], [0xfeff, 0xfeff],
  ].map(([from, to]) => `${String.fromCodePoint(from)}-${String.fromCodePoint(to)}`).join("")}]`,
  "gu",
);

function json(status, body, headers = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
      ...headers,
    },
  });
}
const refuse = (status, error, headers) => json(status, { error }, headers);
// The same answer a stored note gets: a blocked sender or a repeat learns nothing to work around.
const accepted = () => json(202, { status: "received" });

function hex(buffer) {
  return [...new Uint8Array(buffer)].map((b) => b.toString(16).padStart(2, "0")).join("");
}
const encoder = new TextEncoder();
const sha256 = (text) => crypto.subtle.digest("SHA-256", encoder.encode(text));

async function senderOf(ip, secret) {
  const key = await crypto.subtle.importKey(
    "raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"],
  );
  return hex(await crypto.subtle.sign("HMAC", key, encoder.encode(ip)));
}

/** The request body, up to `limit` bytes however much is sent or declared; `null` beyond that. */
async function readBody(request, limit) {
  const declared = Number(request.headers.get("content-length") ?? 0);
  if (declared > limit || !request.body) return declared > limit ? null : "";
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.length;
    if (size > limit) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, at);
    at += chunk.length;
  }
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

export function cleanText(text) {
  return text.replace(/\r\n?/g, "\n").replace(HIDDEN, "").trim();
}

/** Whether a text is worth a person's time; the error's code is what the app explains. */
export function textProblem(text) {
  const length = [...text].length;
  if (length < TEXT_MIN) return "too_short";
  if (length > TEXT_MAX) return "too_long";
  const letters = text.match(/[\p{L}\p{N}]/gu) ?? [];
  if (letters.length < LETTERS_MIN) return "too_short";
  if (new Set(letters.map((c) => c.toLowerCase())).size < DISTINCT_MIN) return "low_quality";
  if ((text.match(/https?:\/\/|www\./giu) ?? []).length > LINKS_MAX) return "too_many_links";
  return null;
}

/** The stored form of a submission, or an error code. Unknown fields are refused, not dropped. */
export function validate(body) {
  if (typeof body !== "object" || body === null || Array.isArray(body)) return { error: "invalid" };
  const known = new Set(["text", "tag", "context"]);
  if (Object.keys(body).some((key) => !known.has(key))) return { error: "invalid" };
  if (typeof body.text !== "string" || !TAGS.has(body.tag)) return { error: "invalid" };
  const context = body.context;
  if (typeof context !== "object" || context === null || Array.isArray(context)) {
    return { error: "invalid" };
  }
  for (const [key, value] of Object.entries(context)) {
    if (typeof value !== "string") return { error: "invalid" };
    // `hasOwn`, because a sender can name a key such as "constructor".
    const allowed = key === "version"
      ? VERSION.test(value)
      : Object.hasOwn(CONTEXT, key) && CONTEXT[key].has(value);
    if (!allowed) return { error: "invalid" };
  }
  if (!context.version || !context.os || !context.page) return { error: "invalid" };
  const text = cleanText(body.text);
  const problem = textProblem(text);
  if (problem) return { error: problem };
  return { note: { text, tag: body.tag, context: JSON.stringify(context) } };
}

/** Repeats match whatever their case, spacing or punctuation. */
async function hashOf(text) {
  const normal = text.normalize("NFKC").toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();
  return hex(await sha256(normal));
}

async function bump(db, key, seconds, now) {
  const window = now - (now % seconds);
  const row = await db.prepare(
    `INSERT INTO counters (key, window, count) VALUES (?1, ?2, 1)
     ON CONFLICT (key, window) DO UPDATE SET count = count + 1 RETURNING count`,
  ).bind(key, window).first();
  return row.count;
}

async function submit(request, env, now) {
  if (!(request.headers.get("content-type") ?? "").startsWith("application/json")) {
    return refuse(415, "invalid");
  }
  if (!env.DB || !env.SENDER_KEY) return refuse(503, "unavailable");
  const ip = request.headers.get("cf-connecting-ip");
  if (!ip) return refuse(400, "invalid");
  const sender = await senderOf(ip, env.SENDER_KEY);
  // Stops a flood before it reaches the database, where each request would cost a write.
  if (env.LIMITER) {
    const { success } = await env.LIMITER.limit({ key: sender });
    if (!success) return refuse(429, "rate_limited", { "retry-after": "60" });
  }
  const raw = await readBody(request, MAX_BODY);
  if (raw === null) return refuse(413, "too_long");
  const db = env.DB;
  const blocked = await db.prepare("SELECT 1 FROM blocked WHERE sender = ?1").bind(sender).first();
  if (blocked) return accepted();
  // Invalid requests count too, so probing for what passes uses up the same allowance.
  for (const { name, seconds, limit } of SENDER_LIMITS) {
    if (await bump(db, `sender:${name}:${sender}`, seconds, now) > limit) {
      return refuse(429, "rate_limited", { "retry-after": String(seconds - (now % seconds)) });
    }
  }
  let body;
  try {
    body = JSON.parse(raw);
  } catch {
    return refuse(400, "invalid");
  }
  const { note, error } = validate(body);
  if (error) return refuse(400, error);
  const hash = await hashOf(note.text);
  const existing = await db.prepare("SELECT id, status FROM notes WHERE hash = ?1").bind(hash).first();
  if (existing) {
    if (existing.status === "pending") {
      await db.prepare("INSERT OR IGNORE INTO reports (note, sender) VALUES (?1, ?2)")
        .bind(existing.id, sender).run();
    }
    return accepted();
  }
  const pending = await db.prepare("SELECT COUNT(*) AS count FROM notes WHERE status = 'pending'").first();
  if (pending.count >= PENDING_MAX) return refuse(503, "busy", { "retry-after": String(DAY) });
  if (await bump(db, "new-notes", DAY, now) > NEW_NOTES_PER_DAY) {
    return refuse(503, "busy", { "retry-after": String(DAY - (now % DAY)) });
  }
  const id = crypto.randomUUID();
  await db.batch([
    db.prepare(
      `INSERT INTO notes (id, hash, sender, tag, text, context, created)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)`,
    ).bind(id, hash, sender, note.tag, note.text, note.context, now),
    db.prepare("INSERT INTO reports (note, sender) VALUES (?1, ?2)").bind(id, sender),
  ]);
  return accepted();
}

/** The stored form of a usage report, or `null`. Unknown fields are refused, not dropped. */
export function validateUsage(body) {
  if (typeof body !== "object" || body === null || Array.isArray(body)) return null;
  const known = new Set(["version", "os", "os_version", "chip", "events"]);
  if (Object.keys(body).some((key) => !known.has(key))) return null;
  const { version, os, os_version: osVersion, chip, events } = body;
  const fixed = [version, os, osVersion, chip].every((value) => typeof value === "string");
  if (!fixed || !VERSION.test(version) || !CONTEXT.os.has(os)) return null;
  if (!OS_VERSION.test(osVersion) || !CHIP.test(chip)) return null;
  if (!Array.isArray(events) || events.length === 0 || events.length > USAGE_EVENTS_MAX) return null;
  for (const event of events) {
    if (typeof event !== "object" || event === null || Array.isArray(event)) return null;
    const fields = new Set(["kind", "subject", "result", "count"]);
    if (Object.keys(event).some((key) => !fields.has(key))) return null;
    // `hasOwn`, because a sender can name a kind such as "constructor".
    if (typeof event.kind !== "string" || !Object.hasOwn(USAGE_EVENTS, event.kind)) return null;
    const rule = USAGE_EVENTS[event.kind];
    if (!rule.subject(event.subject) || !rule.result.has(event.result)) return null;
    if (!Number.isInteger(event.count) || event.count < 1 || event.count > USAGE_COUNT_MAX) return null;
  }
  return { version, os, osVersion, chip, events };
}

async function submitUsage(request, env, now) {
  if (!(request.headers.get("content-type") ?? "").startsWith("application/json")) {
    return refuse(415, "invalid");
  }
  if (!env.DB || !env.SENDER_KEY) return refuse(503, "unavailable");
  const ip = request.headers.get("cf-connecting-ip");
  if (!ip) return refuse(400, "invalid");
  const sender = await senderOf(ip, env.SENDER_KEY);
  if (env.LIMITER) {
    const { success } = await env.LIMITER.limit({ key: sender });
    if (!success) return refuse(429, "rate_limited", { "retry-after": "60" });
  }
  const raw = await readBody(request, MAX_BODY);
  if (raw === null) return refuse(413, "too_long");
  const db = env.DB;
  const blocked = await db.prepare("SELECT 1 FROM blocked WHERE sender = ?1").bind(sender).first();
  if (blocked) return accepted();
  for (const { name, seconds, limit } of USAGE_SENDER_LIMITS) {
    if (await bump(db, `usage:${name}:${sender}`, seconds, now) > limit) {
      return refuse(429, "rate_limited", { "retry-after": String(seconds - (now % seconds)) });
    }
  }
  let body;
  try {
    body = JSON.parse(raw);
  } catch {
    return refuse(400, "invalid");
  }
  const report = validateUsage(body);
  if (!report) return refuse(400, "invalid");
  if (await bump(db, "usage-batches", DAY, now) > USAGE_BATCHES_PER_DAY) {
    return refuse(503, "busy", { "retry-after": String(DAY - (now % DAY)) });
  }
  const day = now - (now % DAY);
  await db.batch([
    db.prepare("INSERT OR IGNORE INTO usage_senders (day, sender) VALUES (?1, ?2)").bind(day, sender),
    ...report.events.map((event) =>
      db.prepare(
        `INSERT INTO usage_daily (day, version, os, os_version, chip, kind, subject, result, count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (day, version, os, os_version, chip, kind, subject, result)
         DO UPDATE SET count = count + excluded.count`,
      ).bind(
        day, report.version, report.os, report.osVersion, report.chip,
        event.kind, event.subject ?? "", event.result, event.count,
      )
    ),
  ]);
  return accepted();
}

async function isAdmin(request, env) {
  const token = env.ADMIN_TOKEN;
  if (typeof token !== "string" || token.length < ADMIN_TOKEN_MIN) return false;
  const given = (request.headers.get("authorization") ?? "").replace(/^Bearer /, "");
  // Comparing digests keeps the time from depending on how much of the token is right.
  const [a, b] = await Promise.all([sha256(given), sha256(token)]);
  const left = new Uint8Array(a);
  const right = new Uint8Array(b);
  return left.reduce((difference, byte, i) => difference | (byte ^ right[i]), 0) === 0;
}

async function listNotes(url, db) {
  const limit = Math.min(Math.max(Number(url.searchParams.get("limit")) || 50, 1), 100);
  const { results } = await db.prepare(
    `SELECT n.id, n.tag, n.text, n.context, n.created,
       (SELECT COUNT(*) FROM reports r WHERE r.note = n.id) AS reports
     FROM notes n WHERE n.status = 'pending' ORDER BY n.created LIMIT ?1`,
  ).bind(limit).all();
  const total = await db.prepare("SELECT COUNT(*) AS count FROM notes WHERE status = 'pending'").first();
  return json(200, {
    total: total.count,
    notes: results.map((note) => ({ ...note, context: JSON.parse(note.context) })),
  });
}

async function resolveNote(id, action, db, now) {
  const note = await db.prepare("SELECT sender FROM notes WHERE id = ?1").bind(id).first();
  if (!note) return refuse(404, "not_found");
  const status = action === "accept" ? "accepted" : "rejected";
  const closeOne = db.prepare(
    `UPDATE notes SET status = ?1, text = '', context = '{}' WHERE id = ?2 AND status = 'pending'`,
  ).bind(status, id);
  if (action !== "block") {
    await db.batch([closeOne]);
    return json(200, { status });
  }
  await db.batch([
    db.prepare("INSERT OR REPLACE INTO blocked (sender, created) VALUES (?1, ?2)").bind(note.sender, now),
    db.prepare(
      `UPDATE notes SET status = 'rejected', text = '', context = '{}'
       WHERE sender = ?1 AND status = 'pending'`,
    ).bind(note.sender),
  ]);
  return json(200, { status: "blocked" });
}

/** Totals over the last days, for Dev mode. Today's senders are counted live. */
async function usageSummary(url, db, now) {
  const days = Math.min(Math.max(Number(url.searchParams.get("days")) || 30, 1), USAGE_DAYS_MAX);
  const today = now - (now % DAY);
  const since = today - (days - 1) * DAY;
  const { results } = await db.prepare(
    `SELECT version, os, os_version, chip, kind, subject, result, SUM(count) AS count
     FROM usage_daily WHERE day >= ?1
     GROUP BY version, os, os_version, chip, kind, subject, result
     ORDER BY version DESC, kind, subject, result LIMIT 500`,
  ).bind(since).all();
  const { results: reach } = await db.prepare(
    `SELECT day, senders FROM usage_reach WHERE day >= ?1
     UNION ALL SELECT day, COUNT(*) FROM usage_senders WHERE day >= ?1 GROUP BY day
     ORDER BY day`,
  ).bind(since).all();
  return json(200, { days, reach, rows: results });
}

async function admin(request, url, env, now) {
  if (!env.DB) return refuse(503, "unavailable");
  if (!await isAdmin(request, env)) return refuse(401, "unauthorized");
  if (request.method === "GET" && url.pathname === "/v1/admin/notes") return listNotes(url, env.DB);
  if (request.method === "GET" && url.pathname === "/v1/admin/usage") {
    return usageSummary(url, env.DB, now);
  }
  const match = url.pathname.match(/^\/v1\/admin\/notes\/([^/]+)\/(accept|reject|block)$/);
  if (request.method === "POST" && match && ID.test(match[1])) {
    return resolveNote(match[1], match[2], env.DB, now);
  }
  return refuse(404, "not_found");
}

/** Deletes what is past its time. Runs daily; a note nobody handled does not stay forever. */
export async function cleanUp(db, now) {
  await db.batch([
    db.prepare("DELETE FROM notes WHERE created < ?1").bind(now - KEEP_SECONDS),
    db.prepare("DELETE FROM reports WHERE note NOT IN (SELECT id FROM notes)"),
    db.prepare("DELETE FROM counters WHERE window < ?1").bind(now - 2 * DAY),
    db.prepare("DELETE FROM blocked WHERE created < ?1").bind(now - BLOCK_SECONDS),
    // A finished day becomes one number, and the hashed addresses behind it are deleted.
    db.prepare(
      `INSERT OR REPLACE INTO usage_reach (day, senders)
       SELECT day, COUNT(*) FROM usage_senders WHERE day < ?1 GROUP BY day`,
    ).bind(now - (now % DAY)),
    db.prepare("DELETE FROM usage_senders WHERE day < ?1").bind(now - (now % DAY)),
    db.prepare("DELETE FROM usage_daily WHERE day < ?1").bind(now - USAGE_KEEP_SECONDS),
    db.prepare("DELETE FROM usage_reach WHERE day < ?1").bind(now - USAGE_KEEP_SECONDS),
  ]);
}

export async function handle(request, env, now = Math.floor(Date.now() / 1000)) {
  try {
    const url = new URL(request.url);
    if (url.pathname === "/v1/notes") {
      return request.method === "POST" ? await submit(request, env, now) : refuse(405, "invalid");
    }
    if (url.pathname === "/v1/usage") {
      return request.method === "POST" ? await submitUsage(request, env, now) : refuse(405, "invalid");
    }
    if (url.pathname.startsWith("/v1/admin/")) return await admin(request, url, env, now);
    return refuse(404, "not_found");
  } catch {
    // Nothing about the request goes to the logs: it may hold a person's text.
    return refuse(500, "unavailable");
  }
}

export default {
  fetch: (request, env) => handle(request, env),
  scheduled: (_event, env, context) => {
    context.waitUntil(cleanUp(env.DB, Math.floor(Date.now() / 1000)));
  },
};
