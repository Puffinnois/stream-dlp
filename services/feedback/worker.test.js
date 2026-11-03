// Runs the Worker's real SQL against SQLite, the engine D1 is built on.
import { assertEquals } from "./assert.js";
import { cleanUp, handle, TEXT_MAX } from "./worker.js";
import { memoryDatabase } from "./sqlite-d1.js";

const SCHEMA = new URL("./schema.sql", import.meta.url);
const ADMIN = "a".repeat(40);
const NOW = 1_800_000_000;
const CONTEXT = { version: "0.6.1", os: "windows", page: "download" };

async function service() {
  const db = memoryDatabase(await Deno.readTextFile(SCHEMA));
  return { DB: db, SENDER_KEY: "secret", ADMIN_TOKEN: ADMIN };
}

function post(env, body, { ip = "203.0.113.1", now = NOW, headers = {} } = {}) {
  const request = new Request("https://feedback.test/v1/notes", {
    method: "POST",
    headers: { "content-type": "application/json", "cf-connecting-ip": ip, ...headers },
    body: typeof body === "string" ? body : JSON.stringify(body),
  });
  return handle(request, env, now);
}

const note = (text, more = {}) => ({ text, tag: "bug", context: CONTEXT, ...more });
const adminRequest = (env, path, method = "GET", token = ADMIN, now = NOW) =>
  handle(
    new Request(`https://feedback.test${path}`, {
      method,
      headers: { authorization: `Bearer ${token}` },
    }),
    env,
    now,
  );
async function pending(env) {
  const response = await adminRequest(env, "/v1/admin/notes");
  assertEquals(response.status, 200);
  return (await response.json()).notes;
}
const code = async (response) => (await response.json()).error;
const GOOD = "The format list cuts off long codec names";

Deno.test("a valid note is stored and listed without its sender", async () => {
  const env = await service();
  assertEquals((await post(env, note(GOOD))).status, 202);
  const notes = await pending(env);
  assertEquals(notes.length, 1);
  assertEquals(notes[0].text, GOOD);
  assertEquals(notes[0].context, CONTEXT);
  assertEquals(notes[0].reports, 1);
  assertEquals("sender" in notes[0], false);
});

Deno.test("no IP address reaches the database", async () => {
  const env = await service();
  await post(env, note(GOOD), { ip: "198.51.100.77" });
  assertEquals(JSON.stringify(env.DB.dump()).includes("198.51.100.77"), false);
});

Deno.test("fields outside the allowlist are refused", async () => {
  const cases = [
    note(GOOD, { screenshot: "x" }),
    note(GOOD, { tag: "urgent" }),
    note(GOOD, { context: { ...CONTEXT, path: "C:\\Users\\someone\\video.mp4" } }),
    note(GOOD, { context: { ...CONTEXT, page: "C:\\Users\\someone" } }),
    note(GOOD, { context: { ...CONTEXT, version: "1.0.0; drop" } }),
    note(GOOD, { context: { ...CONTEXT, source: 5 } }),
    note(GOOD, { context: { os: "windows", page: "gif" } }),
    note(GOOD, { context: JSON.parse(`{"constructor":"x","version":"1.0.0","os":"windows","page":"gif"}`) }),
    note(GOOD, { context: JSON.parse(`{"__proto__":"x","version":"1.0.0","os":"windows","page":"gif"}`) }),
    [],
    "not json",
  ];
  for (const body of cases) {
    const env = await service();
    const response = await post(env, body);
    assertEquals(response.status, 400);
    assertEquals(await code(response), "invalid");
    assertEquals((await pending(env)).length, 0);
  }
});

Deno.test("useless text is refused", async () => {
  const env = await service();
  const refused = [
    ["hello", "too_short"],
    ["!!!!!!!!!!!!!!!!!!!!!!!!", "too_short"],
    ["aaaaaaaaaaaaaaaaaaaaaaaaaaaa", "low_quality"],
    ["ababababababababababab", "low_quality"],
    ["x".repeat(TEXT_MAX + 1), "too_long"],
    ["buy here https://a.example https://b.example www.c.example", "too_many_links"],
  ];
  for (const [i, [text, expected]] of refused.entries()) {
    const response = await post(env, note(text), { ip: `203.0.113.${i + 10}` });
    assertEquals(await code(response), expected);
  }
  assertEquals((await post(env, note("格式列表把很长的编码名称截断了，看不到完整内容"), { ip: "203.0.113.50" })).status, 202);
});

Deno.test("hidden and text-reversing characters are removed", async () => {
  const env = await service();
  const hidden = (code) => String.fromCodePoint(code);
  await post(env, note(`Format${hidden(0x202e)} list${hidden(0x200b)} is${hidden(0x7)} cut off\r\nin the middle`));
  assertEquals((await pending(env))[0].text, "Format list is cut off\nin the middle");
});

Deno.test("an oversized body is refused, even with no or a false length", async () => {
  const env = await service();
  const big = JSON.stringify(note("x y z ".repeat(5000)));
  assertEquals((await post(env, big)).status, 413);
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(big));
      controller.close();
    },
  });
  const request = new Request("https://feedback.test/v1/notes", {
    method: "POST",
    headers: { "content-type": "application/json", "cf-connecting-ip": "203.0.113.2", "content-length": "10" },
    body: stream,
    duplex: "half",
  });
  assertEquals((await handle(request, env, NOW)).status, 413);
});

Deno.test("a sender is limited, invalid requests count, others are not affected", async () => {
  const env = await service();
  for (let i = 0; i < 5; i++) assertEquals((await post(env, "not json")).status, 400);
  const limited = await post(env, note(GOOD));
  assertEquals(limited.status, 429);
  assertEquals(Number(limited.headers.get("retry-after")) > 0, true);
  assertEquals((await post(env, note(GOOD), { ip: "203.0.113.9" })).status, 202);
  assertEquals((await post(env, note(GOOD + " again"), { now: NOW + 601 })).status, 202);
});

Deno.test("a repeat merges, and one sender cannot inflate its count", async () => {
  const env = await service();
  await post(env, note(GOOD));
  await post(env, note(GOOD.toUpperCase() + "!!"));
  assertEquals((await pending(env))[0].reports, 1);
  assertEquals((await post(env, note(GOOD), { ip: "203.0.113.3" })).status, 202);
  const notes = await pending(env);
  assertEquals(notes.length, 1);
  assertEquals(notes[0].reports, 2);
});

Deno.test("a rejected text does not come back and its content is gone", async () => {
  const env = await service();
  await post(env, note(GOOD));
  const [{ id }] = await pending(env);
  assertEquals((await adminRequest(env, `/v1/admin/notes/${id}/reject`, "POST")).status, 200);
  assertEquals((await post(env, note(GOOD), { ip: "203.0.113.4" })).status, 202);
  assertEquals((await pending(env)).length, 0);
  assertEquals(JSON.stringify(env.DB.dump()).includes("codec"), false);
});

Deno.test("blocking drops the sender's notes and looks like success to them", async () => {
  const env = await service();
  await post(env, note(GOOD));
  await post(env, note("A second thing from the same sender"));
  await post(env, note("An unrelated report from someone else"), { ip: "203.0.113.5" });
  const notes = await pending(env);
  const id = notes.find((n) => n.text === GOOD).id;
  assertEquals((await adminRequest(env, `/v1/admin/notes/${id}/block`, "POST")).status, 200);
  assertEquals((await pending(env)).map((n) => n.text), ["An unrelated report from someone else"]);
  assertEquals((await post(env, note("Trying again after being blocked"))).status, 202);
  assertEquals((await pending(env)).length, 1);
});

Deno.test("the daily cap and the pending cap stop new notes but not repeats", async () => {
  const env = await service();
  const fill = (i) => post(env, note(`Distinct report number ${i} about the app`), { ip: `10.0.${i >> 8}.${i & 255}` });
  for (let i = 0; i < 200; i++) assertEquals((await fill(i)).status, 202);
  const over = await fill(200);
  assertEquals(over.status, 503);
  assertEquals(await code(over), "busy");
  assertEquals((await post(env, note("Distinct report number 0 about the app"), { ip: "10.9.9.9" })).status, 202);

  const full = await service();
  full.DB.exec(`WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000)
    INSERT INTO notes (id, hash, sender, tag, text, context, created)
    SELECT 'id' || i, 'hash' || i, 's', 'bug', 't', '{}', ${NOW} FROM n`);
  assertEquals((await post(full, note(GOOD))).status, 503);
});

Deno.test("the Inbox needs the token, and fails closed without a good one", async () => {
  const env = await service();
  assertEquals((await adminRequest(env, "/v1/admin/notes", "GET", "wrong")).status, 401);
  const noHeader = new Request("https://feedback.test/v1/admin/notes");
  assertEquals((await handle(noHeader, env, NOW)).status, 401);
  for (const token of [undefined, "", "short"]) {
    const weak = { ...env, ADMIN_TOKEN: token };
    assertEquals((await adminRequest(weak, "/v1/admin/notes", "GET", token ?? "")).status, 401);
  }
  assertEquals((await adminRequest(env, "/v1/admin/notes/not-an-id/reject", "POST")).status, 404);
  assertEquals((await adminRequest(env, "/v1/admin/notes/00000000-0000-4000-8000-000000000000/reject", "POST")).status, 404);
});

Deno.test("nothing is taken from a browser page, and no secret means no service", async () => {
  const env = await service();
  const response = await post(env, note(GOOD));
  assertEquals(response.headers.get("access-control-allow-origin"), null);
  assertEquals((await post(env, GOOD, { headers: { "content-type": "text/plain" } })).status, 415);
  assertEquals((await post({ ...env, SENDER_KEY: undefined }, note(GOOD))).status, 503);
  const anonymous = new Request("https://feedback.test/v1/notes", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(note(GOOD)),
  });
  assertEquals((await handle(anonymous, env, NOW)).status, 400);
});

Deno.test("the edge limiter stops a flood before the database", async () => {
  const env = await service();
  env.LIMITER = { limit: () => Promise.resolve({ success: false }) };
  assertEquals((await post(env, note(GOOD))).status, 429);
  assertEquals(env.DB.dump().counters.length, 0);
});

Deno.test("old notes, counters and blocks are deleted", async () => {
  const env = await service();
  await post(env, note(GOOD));
  await cleanUp(env.DB, NOW + 91 * 86400);
  const left = env.DB.dump();
  assertEquals([left.notes.length, left.reports.length, left.counters.length], [0, 0, 0]);
});
