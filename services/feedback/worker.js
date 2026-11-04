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

async function admin(request, url, env, now) {
  if (!env.DB) return refuse(503, "unavailable");
  if (!await isAdmin(request, env)) return refuse(401, "unauthorized");
  if (request.method === "GET" && url.pathname === "/v1/admin/notes") return listNotes(url, env.DB);
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
  ]);
}

export async function handle(request, env, now = Math.floor(Date.now() / 1000)) {
  try {
    const url = new URL(request.url);
    if (url.pathname === "/v1/notes") {
      return request.method === "POST" ? await submit(request, env, now) : refuse(405, "invalid");
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
